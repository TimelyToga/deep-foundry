//! The egui side panel: materials, brush, pause, and the stats (F3).

use egui::{Color32, CornerRadius, RichText, Stroke};
use foundry_content::Content;
use foundry_core::MaterialId;
use foundry_render::wgpu;

/// A material the brush can paint.
pub struct PaintMaterial {
    pub id: MaterialId,
    pub name: String,
    pub color: Color32,
}

/// All materials except air and bedrock, in data file order.
pub fn paint_materials(content: &Content) -> Vec<PaintMaterial> {
    let mats = &content.materials;
    mats.all()
        .filter(|&m| !m.is_air() && mats.ids[m.index()] != "bedrock")
        .map(|m| {
            let [r, g, b, a] = mats.colors[m.index()][0];
            // Show see-through colors over the dark panel color.
            let bg = [24u8, 26, 32];
            let mix = |c: u8, d: u8| ((c as u32 * a as u32 + d as u32 * (255 - a as u32)) / 255) as u8;
            PaintMaterial {
                id: m,
                name: mats.names[m.index()].clone(),
                color: Color32::from_rgb(mix(r, bg[0]), mix(g, bg[1]), mix(b, bg[2])),
            }
        })
        .collect()
}

/// Numbers for the stats panel.
#[derive(Debug, Clone, Default)]
pub struct StatsView {
    pub fps: f32,
    pub frame_ms: f32,
    pub cpu_ms: f32,
    pub tick: u64,
    pub tick_ms: f32,
    pub ticks_per_second: f32,
    pub awake_chunks: u32,
    pub loaded_chunks: u32,
    pub gpu_chunks: u32,
    pub gpu_capacity: u32,
    pub drawn_chunks: u32,
    pub zoom: f32,
    /// Cell under the mouse.
    pub cursor: Option<(i32, i32)>,
}

/// The panel's view of the game state. The panel changes the fields it owns.
pub struct PanelState<'a> {
    pub materials: &'a [PaintMaterial],
    pub selected: &'a mut usize,
    pub brush_radius: &'a mut u16,
    pub paused: bool,
    pub show_stats: bool,
    pub stats: &'a StatsView,
}

/// What the player asked for in the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelAction {
    TogglePause,
    Step,
}

pub const MAX_BRUSH: u16 = 40;
pub const PANEL_WIDTH: f32 = 200.0;

/// Colors, text sizes and spacing for a compact dark look.
pub fn apply_style(ctx: &egui::Context) {
    use egui::{FontFamily, FontId, TextStyle};
    ctx.set_visuals(egui::Visuals::dark());
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::new(14.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(10.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(11.0, FontFamily::Monospace)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(6.0, 3.0);
        style.spacing.button_padding = egui::vec2(5.0, 1.0);
        style.spacing.interact_size.y = 18.0;
        style.spacing.slider_width = 110.0;
        let v = &mut style.visuals;
        v.panel_fill = Color32::from_rgba_unmultiplied(16, 18, 24, 236);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_gray(44));
        v.selection.bg_fill = Color32::from_rgb(52, 84, 128);
        v.widgets.inactive.corner_radius = CornerRadius::same(3);
        v.widgets.hovered.corner_radius = CornerRadius::same(3);
        v.widgets.active.corner_radius = CornerRadius::same(3);
    });
}

pub fn draw(ui: &mut egui::Ui, s: PanelState<'_>, actions: &mut Vec<PanelAction>) {
    let frame = egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(10, 8));
    egui::Panel::left("tools").resizable(false).exact_size(PANEL_WIDTH).frame(frame).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| panel_contents(ui, s, actions));
    });
}

fn panel_contents(ui: &mut egui::Ui, s: PanelState<'_>, actions: &mut Vec<PanelAction>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("DEEP FOUNDRY").heading().strong().color(Color32::from_rgb(230, 180, 110)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (label, color) = if s.paused {
                ("PAUSED", Color32::from_rgb(240, 190, 80))
            } else {
                ("RUNNING", Color32::from_rgb(120, 210, 140))
            };
            ui.label(RichText::new(label).small().strong().color(color));
        });
    });
    ui.add_space(4.0);

    section(ui, "MATERIALS");
    egui::Grid::new("materials").num_columns(2).spacing(egui::vec2(4.0, 2.0)).show(ui, |ui| {
        for (i, m) in s.materials.iter().enumerate() {
            let selected = *s.selected == i;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(11.0, 11.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, CornerRadius::same(2), m.color);
                if selected {
                    let stroke = Stroke::new(1.0, Color32::WHITE);
                    ui.painter().rect_stroke(
                        rect.expand(1.0),
                        CornerRadius::same(3),
                        stroke,
                        egui::StrokeKind::Outside,
                    );
                }
                let text =
                    if i < 9 { RichText::new(format!("{} {}", i + 1, m.name)) } else { RichText::new(m.name.as_str()) };
                let button = egui::Button::selectable(selected, text).right_text("").min_size(egui::vec2(72.0, 16.0));
                if ui.add(button).clicked() {
                    *s.selected = i;
                }
            });
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });

    ui.add_space(6.0);
    section(ui, "BRUSH");
    let mut radius = *s.brush_radius as i32;
    ui.horizontal(|ui| {
        ui.add(egui::Slider::new(&mut radius, 0..=MAX_BRUSH as i32));
        ui.label(RichText::new("[ ]").small().weak());
    });
    *s.brush_radius = radius as u16;
    ui.label(RichText::new("Left mouse paints. Right mouse erases.").small().weak());

    ui.add_space(6.0);
    section(ui, "SIMULATION");
    ui.horizontal(|ui| {
        let text = if s.paused { "Resume" } else { "Pause" };
        if ui.button(text).on_hover_text("Space").clicked() {
            actions.push(PanelAction::TogglePause);
        }
        if ui.add_enabled(s.paused, egui::Button::new("Step")).on_hover_text("Period (.)").clicked() {
            actions.push(PanelAction::Step);
        }
    });

    if s.show_stats {
        ui.add_space(6.0);
        section(ui, "STATS (F3)");
        let st = s.stats;
        egui::Grid::new("stats").num_columns(2).spacing(egui::vec2(10.0, 1.0)).show(ui, |ui| {
            let mut row = |name: &str, value: String| {
                ui.label(RichText::new(name).small().weak());
                ui.label(RichText::new(value).monospace());
                ui.end_row();
            };
            row("FPS", format!("{:.0}", st.fps));
            row("frame", format!("{:.2} ms", st.frame_ms));
            row("CPU / frame", format!("{:.2} ms", st.cpu_ms));
            row("tick", format!("{}", st.tick));
            row("tick time", format!("{:.2} ms", st.tick_ms));
            row("ticks / s", format!("{:.1}", st.ticks_per_second));
            row("awake chunks", format!("{} / {}", st.awake_chunks, st.loaded_chunks));
            row("GPU chunks", format!("{} / {}", st.gpu_chunks, st.gpu_capacity));
            row("drawn chunks", format!("{}", st.drawn_chunks));
            row("zoom", format!("{:.2}", st.zoom));
            if let Some((x, y)) = st.cursor {
                row("cursor", format!("{x}, {y}"));
            }
        });
    }

    ui.add_space(8.0);
    section(ui, "KEYS");
    ui.label(
        RichText::new("WASD, arrows, middle drag: move\nWheel: zoom    1-9: material\nSpace: pause    Period: step\n[ ]: brush size    F3: stats")
            .small()
            .weak(),
    );
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).small().strong().color(Color32::from_gray(150)));
    ui.separator();
}

/// Draw egui on top of `target`. Returns command buffers that must be sent before the encoder's
/// (they are empty unless egui paint callbacks are used).
pub fn render_egui(
    renderer: &mut egui_wgpu::Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    jobs: &[egui::ClippedPrimitive],
    screen: &egui_wgpu::ScreenDescriptor,
) -> Vec<wgpu::CommandBuffer> {
    let cmds = renderer.update_buffers(device, queue, encoder, jobs, screen);
    let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("egui"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
        })],
        ..Default::default()
    });
    renderer.render(&mut pass.forget_lifetime(), jobs, screen);
    cmds
}
