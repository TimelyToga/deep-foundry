//! The game UI: the `foundry_ui` screens and their model.
//!
//! Two modes:
//! - **Sandbox** (like the Factorio cheat mode): the inventory has every material with no limit,
//!   the stack in the hand is the paint brush, and the quickbar holds brush materials.
//!   `SandboxUi` applies these actions itself.
//! - **Normal**: the model comes from the factory (`normal.rs` fills it each frame).
//!
//! `SandboxUi` owns the `UiModel`. The window code (`app.rs`) handles the world actions (new
//! game, save, load, pause).

use crate::saves;
use foundry_content::{Content, ItemRef, Stack};
use foundry_core::MaterialId;
use foundry_render::wgpu;
use foundry_ui::{FoundryUi, GameMode, GameState, SandboxView, SlotRef, UiAction, UiModel};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Largest brush radius in cells.
pub const MAX_BRUSH: u16 = 40;

/// The quickbar at the start. The bottom row (keys 1 to 0) has the main materials and air as
/// the eraser. Ids that the data files do not have are left out.
const HOTBAR_BOTTOM: [&str; 10] = ["sand", "water", "oil", "lava", "stone", "wood", "fire", "steam", "methane", "air"];
const HOTBAR_TOP: [&str; 10] =
    ["dirt", "gravel", "clay", "ice", "snow", "sulfuric_acid", "molten_copper", "charcoal", "raw_coal", "smoke"];

/// How long a message stays on the screen.
const MESSAGE_TIME: Duration = Duration::from_secs(4);

pub struct SandboxUi {
    pub ui: FoundryUi,
    pub model: UiModel,
    pub saves_dir: PathBuf,
    message_until: Option<Instant>,
    /// Every message since the start, for `--smoke-test`.
    pub message_log: Vec<String>,
}

impl SandboxUi {
    pub fn new(ctx: &egui::Context, content: Arc<Content>, saves_dir: PathBuf) -> Self {
        let ui = FoundryUi::new(ctx);
        let mut model = UiModel::new(content);
        model.settings.show_fps = true;
        model.settings.simulation = sim_sliders(&foundry_sim::SimSettings::default());
        model.saves = saves::list(&saves_dir);
        let mut s = Self { ui, model, saves_dir, message_until: None, message_log: Vec::new() };
        s.set_mode(GameMode::Sandbox);
        s
    }

    /// Set up the model for a mode: the sandbox inventory and brush, or an empty model that the
    /// normal mode fills from the factory.
    pub fn set_mode(&mut self, mode: GameMode) {
        let content = self.model.content.clone();
        let model = &mut self.model;
        model.player = Default::default();
        model.building = None;
        model.research = None;
        model.techs.clear();
        model.guide.clear();
        model.finished_techs.clear();
        if mode == GameMode::Normal {
            model.sandbox = None;
            return;
        }
        model.player.inventory = content
            .materials
            .all()
            .filter(|m| content.materials.ids[m.index()] != "bedrock")
            .map(|m| Some(Stack { item: ItemRef::Material(m), count: 1 }))
            .collect();
        let slot = |id: &str| content.material(id).map(ItemRef::Material);
        model.player.hotbar = HOTBAR_BOTTOM.iter().chain(HOTBAR_TOP.iter()).map(|id| slot(id)).collect();
        model.player.selected_hotbar = Some(0);
        model.player.hand = model.player.hotbar[0].map(|item| Stack { item, count: 1 });
        model.player.craft_speed = 1.0;
        model.sandbox = Some(SandboxView { brush_radius: 6, sim_paused: false });
    }

    /// `Command::SetSimSetting` for every simulation slider, to give a new simulation the
    /// player's values.
    pub fn sim_setting_commands(&self) -> Vec<foundry_core::Command> {
        self.model.settings.simulation.iter().map(|s| foundry_core::Command::SetSimSetting { key: s.key.clone(), value: s.value }).collect()
    }

    /// The material that the brush paints: the material in the hand.
    pub fn brush_material(&self) -> Option<MaterialId> {
        match self.model.player.hand?.item {
            ItemRef::Material(m) => Some(m),
            ItemRef::Part(_) => None,
        }
    }

    pub fn brush_radius(&self) -> u16 {
        self.model.sandbox.map_or(6, |s| s.brush_radius)
    }

    pub fn set_brush_radius(&mut self, r: u16) {
        if let Some(s) = self.model.sandbox.as_mut() {
            s.brush_radius = r.min(MAX_BRUSH);
        }
    }

    pub fn state(&self) -> GameState {
        self.model.state
    }

    /// Show a short message at the top of the screen.
    pub fn message(&mut self, text: impl Into<String>, now: Instant) {
        self.model.message = text.into();
        self.message_log.push(self.model.message.clone());
        self.message_until = Some(now + MESSAGE_TIME);
    }

    /// Remove the message when its time is over.
    pub fn update_message(&mut self, now: Instant) {
        if self.message_until.is_some_and(|t| now >= t) {
            self.model.message.clear();
            self.message_until = None;
        }
    }

    /// Read the save folder again.
    pub fn refresh_saves(&mut self) {
        self.model.saves = saves::list(&self.saves_dir);
    }

    /// Apply the actions that change only the sandbox hand and quickbar.
    /// Returns false for actions the window code must handle.
    pub fn sandbox_action(&mut self, action: &UiAction) -> bool {
        let p = &mut self.model.player;
        match *action {
            UiAction::ClickSlot { slot: SlotRef::Inventory(i), .. } => {
                // The stacks have no limit: a click puts the material in the hand, and the
                // inventory does not change.
                if let Some(Some(stack)) = p.inventory.get(i) {
                    p.hand = Some(Stack { item: stack.item, count: 1 });
                    p.selected_hotbar = p.hotbar.iter().position(|h| *h == Some(stack.item));
                }
                true
            }
            UiAction::ClickSlot { .. } => true,
            UiAction::SelectHotbar(i) => {
                if let Some(Some(item)) = p.hotbar.get(i) {
                    p.hand = Some(Stack { item: *item, count: 1 });
                    p.selected_hotbar = Some(i);
                }
                true
            }
            UiAction::SetHotbar { index, item } => {
                if let Some(slot) = p.hotbar.get_mut(index) {
                    *slot = item;
                    if item.is_some() && item == p.hand.map(|h| h.item) {
                        p.selected_hotbar = Some(index);
                    } else if p.selected_hotbar == Some(index) && item.is_none() {
                        p.selected_hotbar = None;
                    }
                }
                true
            }
            UiAction::ClearHand => {
                p.hand = None;
                p.selected_hotbar = None;
                true
            }
            _ => false,
        }
    }
}

/// Draw egui on top of `target`. With `clear`, fill the target with that color first
/// (when there is no world under the UI). Returns command buffers that must be sent before the
/// encoder's (they are empty unless egui paint callbacks are used).
#[allow(clippy::too_many_arguments)]
pub fn render_egui(
    renderer: &mut egui_wgpu::Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    jobs: &[egui::ClippedPrimitive],
    screen: &egui_wgpu::ScreenDescriptor,
    clear: Option<wgpu::Color>,
) -> Vec<wgpu::CommandBuffer> {
    let cmds = renderer.update_buffers(device, queue, encoder, jobs, screen);
    let load = match clear {
        Some(c) => wgpu::LoadOp::Clear(c),
        None => wgpu::LoadOp::Load,
    };
    let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("egui"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
        })],
        ..Default::default()
    });
    renderer.render(&mut pass.forget_lifetime(), jobs, screen);
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> SandboxUi {
        let ctx = egui::Context::default();
        let content = Arc::new(Content::load_default().unwrap());
        SandboxUi::new(&ctx, content, std::env::temp_dir().join("deep-foundry-no-saves-here"))
    }

    #[test]
    fn starts_with_sand_in_the_hand_and_air_on_key_0() {
        let s = sandbox();
        let c = s.model.content.clone();
        assert_eq!(s.brush_material(), c.material("sand"));
        assert_eq!(s.model.player.hotbar[9], Some(ItemRef::Material(MaterialId::AIR)));
        // Every material except bedrock is in the inventory.
        assert_eq!(s.model.player.inventory.len(), c.materials.len() - 1);
    }

    #[test]
    fn inventory_click_and_quickbar_select_set_the_brush() {
        let mut s = sandbox();
        let c = s.model.content.clone();
        let water = ItemRef::Material(c.expect_material("water"));
        let i = s.model.player.inventory.iter().position(|x| x.map(|x| x.item) == Some(water)).unwrap();
        assert!(s.sandbox_action(&UiAction::ClickSlot { slot: SlotRef::Inventory(i), click: foundry_ui::SlotClick::LEFT }));
        assert_eq!(s.brush_material(), c.material("water"));
        assert_eq!(s.model.player.selected_hotbar, Some(1));
        s.sandbox_action(&UiAction::SelectHotbar(9));
        assert_eq!(s.brush_material(), Some(MaterialId::AIR));
        s.sandbox_action(&UiAction::ClearHand);
        assert_eq!(s.brush_material(), None);
        assert!(!s.sandbox_action(&UiAction::Pause));
    }
}

/// The simulation sliders of the settings screen.
fn sim_sliders(settings: &foundry_sim::SimSettings) -> Vec<foundry_ui::SimSetting> {
    settings
        .sliders()
        .into_iter()
        .map(|s| foundry_ui::SimSetting {
            key: s.key.into(),
            label: s.label.into(),
            help: s.help.into(),
            value: s.value,
            min: s.min,
            max: s.max,
            step: s.step,
        })
        .collect()
}
