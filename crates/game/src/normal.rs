//! The normal game mode on the main thread: turn the factory views into the UI model, turn UI
//! actions and the mouse into factory commands, and make the player input.
//!
//! The factory runs on the simulation thread (`factory_host.rs`). This module only reads its
//! `FactoryFrame`s and sends `FactoryCommand`s.

use crate::factory_host::{FactoryCommand, FactoryFrame, GameCommand, GhostRequest, PlayerInput, SlotGroup, SlotTarget};
use crate::overlay::LocalGhost;
use crate::player::MoveInput;
use foundry_content::{Content, ItemRef, Stack};
use foundry_core::{BuildingKindId, CellPos, CellRect, MaterialId, TILE_SIZE, TilePos};
use foundry_factory::progress::{GoalView, TechState as FactoryTechState, TechView};
use foundry_factory::{Click, Status};
use foundry_ui::{
    BuildingSlot, BuildingSlots, BuildingView, ClickButton, CraftJobView, Delivery, GuideGoal, HoverView, MachineStatus, MaterialBuffer,
    MilestoneView, ResearchView, SlotClick, SlotRef, TankSlot, TechEntry, TechState, UiAction, UiModel, WindowKind,
};
use std::time::Instant;

/// The keys and buttons of the normal mode that are held down.
#[derive(Debug, Clone, Copy, Default)]
pub struct NormalHeld {
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub dig: bool,
    pub spray: bool,
    pub scan: bool,
}

/// The state of the normal mode on the main thread.
#[derive(Default)]
pub struct NormalMode {
    /// The newest factory views.
    pub frame: FactoryFrame,
    /// The newest guide and technology lists (they are not in every frame).
    pub guide: Vec<GoalView>,
    pub techs: Vec<TechView>,
    /// Rotation of the building in the hand (0 to 3).
    pub rotation: u8,
    /// The material the spray tool puts out. `None`: the first tank that has material.
    pub spray: Option<MaterialId>,
    pub held: NormalHeld,
    /// Aim here instead of at the mouse (for the smoke test).
    pub aim_override: Option<CellPos>,
    last_input: Option<PlayerInput>,
    last_windows: Option<(bool, bool)>,
    /// When the frame with a new tick arrived, and the robot position (top left, in cells) of the
    /// tick before. The robot is drawn between the two, so it moves smoothly on fast displays.
    pub frame_time: Option<Instant>,
    prev_robot: Option<(f32, f32)>,
}

impl NormalMode {
    pub fn new() -> Self {
        Self::default()
    }

    /// Use a new frame from the simulation thread. Returns its notices.
    pub fn take_frame(&mut self, mut frame: FactoryFrame, now: Instant) -> Vec<String> {
        if let Some(g) = frame.guide.take() {
            self.guide = g;
        }
        if let Some(t) = frame.techs.take() {
            self.techs = t;
        }
        let notices = std::mem::take(&mut frame.notices);
        if frame.tick != self.frame.tick || self.frame_time.is_none() {
            self.prev_robot = self.frame.robot.map(|r| top_left(&r));
            self.frame_time = Some(now);
        }
        self.frame = frame;
        notices
    }

    /// The robot's top-left corner (in cells) to draw now: between the last two ticks.
    pub fn robot_pos(&self, now: Instant) -> Option<(f32, f32)> {
        let cur = top_left(self.frame.robot.as_ref()?);
        let (Some(prev), Some(at)) = (self.prev_robot, self.frame_time) else { return Some(cur) };
        if (cur.0 - prev.0).abs() + (cur.1 - prev.1).abs() > 16.0 {
            return Some(cur);
        }
        let t = (now.saturating_duration_since(at).as_secs_f32() / foundry_core::TICK_SECONDS as f32).clamp(0.0, 1.0);
        Some((prev.0 + (cur.0 - prev.0) * t, prev.1 + (cur.1 - prev.1) * t))
    }

    /// The building kind of the part in the hand, if it is a building.
    pub fn building_in_hand(&self, content: &Content) -> Option<BuildingKindId> {
        match self.frame.cursor?.item {
            ItemRef::Part(p) => content.factory.part_def(p).building,
            ItemRef::Material(_) => None,
        }
    }

    /// The ghost for the building in the hand, centered on the mouse cell.
    pub fn ghost(&self, content: &Content, mouse: CellPos) -> Option<LocalGhost> {
        let kind = self.building_in_hand(content)?;
        let def = content.factory.building_def(kind);
        let size = if self.rotation & 1 == 1 { (def.size.1, def.size.0) } else { def.size };
        let t = TILE_SIZE as f32;
        let x = (mouse.x as f32 / t - size.0 as f32 * 0.5 + 0.5).floor() as i32;
        let y = (mouse.y as f32 / t - size.1 as f32 * 0.5 + 0.5).floor() as i32;
        Some(LocalGhost { request: GhostRequest { kind, at: TilePos::new(x, y), rotation: self.rotation }, size })
    }

    /// The material the spray tool puts out now.
    pub fn spray_material(&self) -> Option<MaterialId> {
        let has = |m: MaterialId| self.frame.inventory.tanks.iter().any(|t| t.item == Some(ItemRef::Material(m)) && t.units > 0);
        match self.spray {
            Some(m) if has(m) => Some(m),
            Some(_) => None,
            None => self.frame.inventory.tanks.iter().find(|t| t.units > 0).and_then(|t| match t.item {
                Some(ItemRef::Material(m)) => Some(m),
                _ => None,
            }),
        }
    }

    /// The player input for this frame. Returns a command only when it changed.
    pub fn input(&mut self, content: &Content, mouse: CellPos, view: CellRect) -> Option<GameCommand> {
        let h = self.held;
        let mouse = self.aim_override.unwrap_or(mouse);
        let input = PlayerInput {
            movement: MoveInput { x: h.right as i8 - h.left as i8, jump: h.jump },
            aim: mouse,
            dig: h.dig,
            spray: if h.spray { self.spray_material() } else { None },
            scan: h.scan,
            ghost: self.ghost(content, mouse).map(|g| g.request),
            view,
        };
        if self.last_input == Some(input) {
            return None;
        }
        self.last_input = Some(input);
        Some(FactoryCommand::Input(input).into())
    }

    /// Tell the host which windows are open, when that changed.
    pub fn windows(&mut self, research: bool, guide: bool) -> Option<GameCommand> {
        if self.last_windows == Some((research, guide)) {
            return None;
        }
        self.last_windows = Some((research, guide));
        Some(FactoryCommand::Windows { research, guide }.into())
    }

    /// The building under the mouse, if the mouse is on it.
    pub fn building_under(&self, mouse: CellPos) -> bool {
        self.frame.hover.as_ref().is_some_and(|h| h.rect.contains(mouse))
    }

    /// A mouse button went down in the world. Returns the commands.
    /// Left: place the building in the hand, else open the building under the mouse, else dig.
    /// Right: take the building under the mouse, else spray.
    pub fn press(&mut self, content: &Content, left: bool, mouse: CellPos) -> Vec<GameCommand> {
        let on_building = self.building_under(mouse);
        if left {
            if let Some(g) = self.ghost(content, mouse) {
                return vec![FactoryCommand::Place { kind: g.request.kind, at: g.request.at, rotation: g.request.rotation }.into()];
            }
            if on_building && self.frame.cursor.is_none() {
                return vec![FactoryCommand::OpenAt(mouse).into()];
            }
            self.held.dig = true;
        } else {
            if on_building {
                return vec![FactoryCommand::RemoveAt(mouse).into()];
            }
            self.held.spray = true;
        }
        vec![]
    }

    /// Turn a UI action into factory commands. Returns false if it is not a factory action.
    pub fn action(&mut self, action: &UiAction, out: &mut Vec<GameCommand>) -> bool {
        match *action {
            UiAction::ClickSlot { slot, click } => {
                let c = click_of(click);
                match slot {
                    SlotRef::Inventory(i) => out.push(FactoryCommand::Click { target: SlotTarget::Inventory(i), click: c }.into()),
                    SlotRef::Tank(i) => {
                        if self.frame.building.is_some() || c.is_move() {
                            // A building window is open: the click moves the material into it.
                            out.push(FactoryCommand::Click { target: SlotTarget::Tank(i), click: c }.into());
                        } else if let Some(ItemRef::Material(m)) = self.frame.inventory.tanks.get(i).and_then(|t| t.item) {
                            // A click on a tank selects its material for the spray tool.
                            self.spray = Some(m);
                            out.push(FactoryCommand::ClearCursor.into());
                        }
                    }
                    SlotRef::Building { building, group, index } => {
                        let group = match group {
                            BuildingSlots::Input => SlotGroup::Input,
                            BuildingSlots::Output => SlotGroup::Output,
                            BuildingSlots::Fuel => SlotGroup::Fuel,
                        };
                        out.push(FactoryCommand::Click { target: SlotTarget::Building { id: building, group, index }, click: c }.into());
                    }
                }
            }
            UiAction::SelectHotbar(i) => match self.frame.hotbar.get(i).copied().flatten() {
                Some(ItemRef::Part(p)) => {
                    self.spray = None;
                    out.push(FactoryCommand::PickToCursor(p).into());
                }
                Some(ItemRef::Material(m)) => {
                    self.spray = Some(m);
                    out.push(FactoryCommand::ClearCursor.into());
                }
                None => {}
            },
            UiAction::SetHotbar { index, item } => out.push(FactoryCommand::SetHotbar { index, item }.into()),
            UiAction::ClearHand => {
                self.spray = None;
                out.push(FactoryCommand::ClearCursor.into());
            }
            UiAction::Craft { recipe, count } => out.push(FactoryCommand::Craft { recipe, count }.into()),
            UiAction::CancelCraft { index, .. } => out.push(FactoryCommand::CancelCraft { index }.into()),
            UiAction::SetRecipe { building, recipe } => out.push(FactoryCommand::SetRecipe { building, recipe }.into()),
            UiAction::StartResearch(t) => out.push(FactoryCommand::StartResearch(t).into()),
            UiAction::CloseWindow(WindowKind::Building) => out.push(FactoryCommand::CloseBuilding.into()),
            UiAction::EmptyTank(i) => out.push(FactoryCommand::EmptyTank(i).into()),
            _ => return false,
        }
        true
    }

    /// Fill the UI model from the newest frame.
    pub fn fill_model(&self, model: &mut UiModel) {
        let f = &self.frame;
        let content = model.content.clone();
        model.sandbox = None;
        let pl = &mut model.player;
        pl.hull = 100.0;
        pl.hull_max = 100.0;
        pl.temperature = f.robot_temperature;
        pl.heat_limit = 80.0;
        pl.inventory.clear();
        pl.inventory.extend(f.inventory.slots.iter().map(|s| s.as_ref().map(|s| Stack { item: s.item, count: s.count })));
        pl.tank.clear();
        pl.tank.extend(f.inventory.tanks.iter().map(|t| TankSlot {
            material: match t.item {
                Some(ItemRef::Material(m)) => Some(m),
                _ => None,
            },
            units: t.units,
            capacity: t.capacity,
        }));
        pl.spray = self.spray_material();
        pl.tanks_full = f.tanks_full;
        // The hand shows the parts on the cursor, else the chosen spray material.
        pl.hand = f.cursor.or_else(|| {
            let m = self.spray?;
            let units: u32 = pl.tank.iter().filter(|t| t.material == Some(m)).map(|t| t.units).sum();
            (units > 0).then_some(Stack { item: ItemRef::Material(m), count: units })
        });
        pl.hotbar.clear();
        pl.hotbar.extend(f.hotbar.iter().copied());
        pl.hotbar.resize(20, None);
        pl.selected_hotbar = pl.hand.and_then(|h| pl.hotbar.iter().position(|x| *x == Some(h.item)));
        pl.crafting.clear();
        pl.crafting.extend(f.crafting.iter().map(|j| CraftJobView { recipe: j.recipe, count: j.count, progress: j.progress }));
        pl.craft_speed = f.craft_speed;
        model.finished_techs.clear();
        model.finished_techs.extend(f.finished_techs.iter().copied());
        model.research = f.research.as_ref().map(|r| ResearchView { tech: r.tech, progress: r.progress });
        model.discovery_points = f.discovery_points;
        model.techs.clear();
        model.techs.extend(self.techs.iter().map(tech_entry));
        model.guide.clear();
        model.guide.extend(self.guide.iter().map(|g| GuideGoal {
            id: g.id.clone(),
            tier: g.tier,
            title: g.title.clone(),
            text: g.text.clone(),
            done: g.done,
            count: g.count,
            reward_points: g.reward_points,
            waits_for: g.waits_for.clone(),
        }));
        model.building = f.building.as_ref().map(|b| building_view(&content, b, f.milestone.as_ref(), &f.later_milestones));
    }

    /// The hover view for the building under the mouse, if there is one.
    pub fn hover(&self, mouse: CellPos) -> Option<HoverView> {
        let h = self.frame.hover.as_ref().filter(|h| h.rect.contains(mouse))?;
        Some(HoverView::Building {
            id: h.id,
            kind: h.kind,
            status: machine_status(h.status),
            recipe: h.recipe,
            progress: h.progress,
            temperature: Some(h.temperature as f32),
            power_w: None,
        })
    }
}

fn top_left(r: &crate::player::Robot) -> (f32, f32) {
    (r.left as f32 + r.rem.0, r.top as f32 + r.rem.1)
}

fn click_of(c: SlotClick) -> Click {
    let right = c.button == ClickButton::Right;
    match (c.shift, c.ctrl, right) {
        (true, _, false) => Click::Shift,
        (true, _, true) => Click::ShiftRight,
        (false, true, false) => Click::Ctrl,
        (false, true, true) => Click::CtrlRight,
        (false, false, true) => Click::Right,
        (false, false, false) => Click::Left,
    }
}

/// A factory milestone view as a UI milestone view.
fn milestone_view(m: &foundry_factory::progress::MilestoneView) -> MilestoneView {
    MilestoneView {
        stage: m.stage,
        name: m.name.clone(),
        description: m.description.clone(),
        items: m.items.iter().map(|d| Delivery { item: d.item, delivered: d.delivered, need: d.need }).collect(),
    }
}

/// The UI status of a factory status.
pub fn machine_status(s: Status) -> MachineStatus {
    match s {
        Status::Idle => MachineStatus::Idle,
        Status::Working => MachineStatus::Working,
        Status::NoRecipe => MachineStatus::NoRecipe,
        Status::NoInput => MachineStatus::NoInput,
        Status::OutputFull => MachineStatus::OutputFull,
        Status::OutputBlocked => MachineStatus::OutputBlocked,
        Status::NoPower => MachineStatus::NoPower,
        Status::TooCold => MachineStatus::TooCold,
        Status::TooHot => MachineStatus::TooHot,
        Status::Broken => MachineStatus::Broken,
    }
}

fn tech_entry(t: &TechView) -> TechEntry {
    let state = match t.state {
        FactoryTechState::Done => TechState::Done,
        FactoryTechState::Researching { .. } => TechState::Researching,
        FactoryTechState::Available => TechState::Available,
        FactoryTechState::Locked(_) => TechState::Locked,
    };
    TechEntry { id: t.id, state, progress: t.progress, reasons: t.reasons.clone(), queue_position: t.queue_position }
}

/// The UI building window from the factory building view.
fn building_view(
    content: &Content,
    b: &foundry_factory::BuildingView,
    milestone: Option<&foundry_factory::progress::MilestoneView>,
    later: &[foundry_factory::progress::MilestoneView],
) -> BuildingView {
    let slot = |item: ItemRef, count: u32, filter: bool| BuildingSlot {
        stack: (count > 0).then_some(Stack { item, count }),
        filter: filter.then_some(item),
        capacity: 0,
    };
    let (inputs, buffers) = match &b.inventory {
        // Storage and the Hub: each slot (a part stack or a material) is an input slot. The
        // slot order is the factory's `Inventory::place` order, so clicks find the right slot.
        Some(inv) => (
            inv.places()
                .iter()
                .map(|p| BuildingSlot { stack: p.item.map(|item| Stack { item, count: p.count }), filter: None, capacity: p.capacity })
                .collect(),
            Vec::<MaterialBuffer>::new(),
        ),
        None => (b.inputs.iter().map(|x| slot(x.item, x.count, true)).collect(), vec![]),
    };
    let _ = content;
    let status = machine_status(b.status);
    // The reason repeats the status for simple states ("Idle"): show it only when it says more.
    let mut status_detail = if b.reason.eq_ignore_ascii_case(status.label()) { String::new() } else { b.reason.clone() };
    if let Some(m) = milestone
        && status_detail.is_empty()
    {
        status_detail = format!("Needs repair stage {}", m.stage);
    }
    BuildingView {
        id: b.id,
        kind: b.kind,
        status,
        status_detail,
        recipe: b.recipe,
        inputs,
        outputs: b.outputs.iter().map(|x| slot(x.item, x.count, true)).collect(),
        fuel: vec![],
        buffers,
        progress: b.progress,
        speed: 1.0,
        power: None,
        temperature: Some(b.temperature as f32),
        milestone: milestone.map(milestone_view),
        later_stages: later.iter().map(milestone_view).collect(),
    }
}

/// The keys of the normal mode, for the settings screen.
pub fn key_bindings() -> Vec<(String, String)> {
    [
        ("Move left / right", "A / D"),
        ("Jump, swim up", "W or Space"),
        ("Dig (hold)", "Left mouse"),
        ("Spray material from the tank (hold)", "Right mouse"),
        ("Choose the spray material", "Click a tank slot"),
        ("Scan the material under the mouse", "F (hold)"),
        ("Place the building in the hand", "Left mouse"),
        ("Rotate the building in the hand", "R"),
        ("Open a building", "Left mouse on it (empty hand)"),
        ("Take a building back", "Right mouse on it"),
        ("Empty the hand", "Q"),
        ("Character screen", "E"),
        ("Research", "T"),
        ("Guide", "G"),
        ("Production statistics", "P"),
        ("Quickbar slot 1-10", "1 - 0"),
        ("Quickbar slot 11-20", "Shift + 1 - 0"),
        ("Pause menu / close window", "Esc"),
        ("Zoom", "Mouse wheel"),
        ("Debug panel", "F3"),
    ]
    .iter()
    .map(|(a, k)| (a.to_string(), k.to_string()))
    .collect()
}

#[cfg(test)]
#[path = "normal_tests.rs"]
mod tests;
