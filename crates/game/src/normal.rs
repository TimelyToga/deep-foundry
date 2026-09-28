//! The normal game mode on the main thread: turn the factory views into the UI model, turn UI
//! actions and the mouse into factory commands, and make the player input.
//!
//! The factory runs on the simulation thread (`factory_host.rs`). This module only reads its
//! `FactoryFrame`s and sends `FactoryCommand`s.

use crate::construct::{BuildView, Construct, DragLine, LocalGhost, Mods, footprint_at, turned_size};
use crate::factory_host::{FactoryCommand, FactoryFrame, GameCommand, GhostRequest, Placement, PlayerInput, SlotGroup, SlotTarget, Turn};
use crate::player::MoveInput;
use foundry_content::{Content, ItemRef, Phase, Stack};
use foundry_core::{BuildingKindId, CellPos, CellRect, MaterialId, TechId};
use foundry_factory::progress::{GoalView, LockReason, TechState as FactoryTechState, TechView};
use foundry_factory::{Click, Status};
use foundry_ui::{
    BuildingSlot, BuildingSlots, BuildingView, ClickButton, CraftJobView, Delivery, DigRule, DigState, GuideGoal, HoverDetail, HoverView, MachineStatus, MaterialBuffer,
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
    /// Construction: rotation per building kind, drag line, remove button, alt mode.
    pub build: Construct,
    /// Messages for the player from the main thread (for example "cannot be flipped").
    pub messages: Vec<String>,
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
        // A drag line stops where the factory could not place.
        if let (Some(d), Some(s)) = (self.build.drag.as_mut(), frame.drag_stop.as_ref())
            && s.action == d.action
            && d.stop.is_none()
        {
            d.stop = Some((s.at, s.reason.clone()));
        }
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
        let (rotation, flip) = self.build.transform(kind);
        let size = turned_size(content.factory.building_def(kind), rotation);
        let at = footprint_at(mouse, size);
        Some(LocalGhost { request: GhostRequest { kind, at, rotation, flip }, size })
    }

    /// What the overlay draws for construction this frame. `mouse` is `None` when the mouse is
    /// over the UI or outside the window.
    pub fn build_view(&self, content: &Content, mouse: Option<CellPos>) -> BuildView {
        let dragging = self.build.drag.is_some();
        BuildView {
            ghost: if dragging { None } else { mouse.and_then(|m| self.ghost(content, m)) },
            drag: self.build.drag.clone(),
            grid: self.building_in_hand(content).is_some() || dragging,
            alt: self.build.alt,
            mouse,
            removing: self.build.removing.is_some(),
        }
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
            // While a line is dragged, its buildings are placed at once; there is no ghost to check.
            ghost: if self.build.drag.is_some() { None } else { self.ghost(content, mouse).map(|g| g.request) },
            view,
            remove: self.build.removing,
            alt: self.build.alt,
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
    /// - Left: Shift pastes the copied settings on the building under the mouse. Else a building
    ///   in the hand is placed and a drag line starts. Else an empty hand opens the building under
    ///   the mouse. Else the tool digs.
    /// - Right: Shift copies the settings of the building under the mouse. Else, on a building,
    ///   the remove button is down until the release. Else the tool sprays.
    pub fn press(&mut self, content: &Content, left: bool, mouse: CellPos, mods: Mods) -> Vec<GameCommand> {
        let mouse = self.aim_override.unwrap_or(mouse);
        let on_building = self.building_under(mouse);
        if left {
            if mods.shift && on_building {
                return vec![FactoryCommand::PasteSettings(mouse).into()];
            }
            if let Some(g) = self.ghost(content, mouse) {
                let r = g.request;
                let action = self.build.next_action();
                let mut line = DragLine::new(action, r.kind, content.factory.building_def(r.kind), r.at, r.rotation, r.flip);
                line.recipe = self.build.recipe_for(r.kind);
                let p = Placement { kind: r.kind, at: r.at, rotation: r.rotation, flip: r.flip, recipe: line.recipe, action };
                self.build.drag = Some(line);
                return vec![FactoryCommand::Place(p).into()];
            }
            if on_building && self.frame.cursor.is_none() {
                return vec![FactoryCommand::OpenAt(mouse).into()];
            }
            self.held.dig = true;
        } else {
            if mods.shift && on_building {
                return vec![FactoryCommand::CopySettings(mouse).into()];
            }
            if on_building {
                self.build.removing = Some(self.build.next_action());
                return vec![];
            }
            self.held.spray = true;
        }
        vec![]
    }

    /// A mouse button went up (also over the UI).
    pub fn release(&mut self, left: bool) {
        if left {
            self.held.dig = false;
            self.build.drag = None;
        } else {
            self.held.spray = false;
            self.build.removing = None;
        }
    }

    /// Each frame: grow the drag line to the mouse. Returns the placements (and the turn of the
    /// first belt when the drag direction is known).
    pub fn update(&mut self, mouse: CellPos) -> Vec<GameCommand> {
        let mouse = self.aim_override.unwrap_or(mouse);
        let Some(d) = self.build.drag.as_mut() else { return vec![] };
        let step = d.advance(footprint_at(mouse, d.size));
        let (kind, start, action, rotation, flip, recipe) = (d.kind, d.start, d.action, d.rotation, d.flip, d.recipe);
        let mut out: Vec<GameCommand> = vec![];
        if let Some((r, f)) = step.turn_start {
            // Belts face the drag direction; the next belt in the hand keeps it.
            self.build.set_transform(kind, r, f);
            out.push(FactoryCommand::Turn { at: start.origin(), turn: Turn::To { rotation: r, flip: f }, action }.into());
        }
        for at in step.place {
            out.push(FactoryCommand::Place(Placement { kind, at, rotation, flip, recipe, action }).into());
        }
        out
    }

    /// R (Shift + R: `back`): turn the building in the hand, else the building under the mouse.
    pub fn rotate(&mut self, content: &Content, mouse: CellPos, back: bool) -> Vec<GameCommand> {
        if let Some(kind) = self.building_in_hand(content) {
            self.build.rotate(content, kind, back);
            return vec![];
        }
        let mouse = self.aim_override.unwrap_or(mouse);
        if self.building_under(mouse) {
            let turn = if back { Turn::CounterClockwise } else { Turn::Clockwise };
            return vec![FactoryCommand::Turn { at: mouse, turn, action: self.build.next_action() }.into()];
        }
        vec![]
    }

    /// F with a building in the hand: flip it. Returns false with no building in the hand (then F
    /// is the scan key).
    pub fn flip(&mut self, content: &Content) -> bool {
        let Some(kind) = self.building_in_hand(content) else { return false };
        if !self.build.flip(content, kind) {
            self.messages.push(format!("{} cannot be flipped", content.factory.building_def(kind).name));
        }
        true
    }

    /// Q, the pipette (as in Factorio). On a building: take that building kind from the
    /// inventory into the hand, with the building's rotation, flip and recipe. Elsewhere: empty
    /// the hand.
    pub fn pipette(&mut self, content: &Content, mouse: CellPos) -> Vec<GameCommand> {
        let mouse = self.aim_override.unwrap_or(mouse);
        let mut out = vec![];
        match self.frame.hover.as_ref().filter(|h| h.rect.contains(mouse)) {
            Some(h) => {
                let def = content.factory.building_def(h.kind);
                self.build.set_transform(h.kind, h.rotation, h.flip);
                self.build.hand_recipe = h.recipe.map(|r| (h.kind, r));
                self.spray = None;
                out.push(FactoryCommand::PickToCursor(def.part).into());
            }
            None => {
                self.action(&UiAction::ClearHand, &mut out);
            }
        }
        out
    }

    /// Ctrl + Z and Ctrl + Y.
    pub fn undo(&mut self, redo: bool) -> Vec<GameCommand> {
        self.build.drag = None;
        vec![if redo { FactoryCommand::Redo } else { FactoryCommand::Undo }.into()]
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
                    self.build.hand_recipe = None;
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
                self.build.hand_recipe = None;
                out.push(FactoryCommand::ClearCursor.into());
            }
            UiAction::Craft { recipe, count } => out.push(FactoryCommand::Craft { recipe, count }.into()),
            UiAction::CancelCraft { index, .. } => out.push(FactoryCommand::CancelCraft { index }.into()),
            UiAction::SetRecipe { building, recipe } => out.push(FactoryCommand::SetRecipe { building, recipe }.into()),
            UiAction::StartResearch(t) => out.push(FactoryCommand::StartResearch(t).into()),
            UiAction::CloseWindow(WindowKind::Building) => out.push(FactoryCommand::CloseBuilding.into()),
            UiAction::EmptyTank(i) => out.push(FactoryCommand::EmptyTank(i).into()),
            UiAction::SetKeep { material, keep } => out.push(FactoryCommand::SetKeep { material, keep }.into()),
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
        pl.dig.clear();
        pl.dig.extend(f.dig_list.iter().map(|&(material, keep)| DigRule { material, keep }));
        model.finished_techs.clear();
        model.finished_techs.extend(f.finished_techs.iter().copied());
        model.research = f.research.as_ref().map(|r| ResearchView { tech: r.tech, progress: r.progress });
        model.discovery_points = f.discovery_points;
        model.techs.clear();
        model.techs.extend(self.techs.iter().map(tech_entry));
        // Guide texts name keys as `{key:ID}`: the player's keys go there.
        let goals: Vec<GuideGoal> = self
            .guide
            .iter()
            .map(|g| GuideGoal {
                id: g.id.clone(),
                tier: g.tier,
                title: g.title.clone(),
                text: model.settings.with_keys(&g.text),
                done: g.done,
                count: g.count,
                reward_points: g.reward_points,
                waits_for: g.waits_for.clone(),
            })
            .collect();
        model.guide.clear();
        model.guide.extend(goals);
        model.building = f.building.as_ref().map(|b| building_view(&content, b, f.milestone.as_ref(), &f.later_milestones));
    }

    /// More about the thing under the mouse for the hover box: the dig rules and the discovery of
    /// a cell, or the reason and hit points of a building.
    pub fn hover_detail(&self, content: &Content, hover: Option<&HoverView>) -> HoverDetail {
        let f = &self.frame;
        match hover {
            Some(HoverView::Cell { material, .. }) => HoverDetail {
                dig: dig_state(content, *material, f.dig_limit, &f.finished_techs),
                undiscovered: !material.is_air()
                    && content.materials.phase[material.index()] != Phase::Empty
                    && !f.discovered.contains(material),
                ..Default::default()
            },
            Some(HoverView::Building { id, .. }) => match f.hover.as_ref().filter(|h| h.id == *id) {
                Some(h) => HoverDetail { reason: h.reason.clone(), hit_points: Some(h.hit_points), ..Default::default() },
                None => HoverDetail::default(),
            },
            None => HoverDetail::default(),
        }
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
        Status::NoFuel => MachineStatus::NoFuel,
        Status::TooCold => MachineStatus::TooCold,
        Status::TooHot => MachineStatus::TooHot,
        Status::Broken => MachineStatus::Broken,
    }
}

pub(crate) fn tech_entry(t: &TechView) -> TechEntry {
    let state = match t.state {
        FactoryTechState::Done => TechState::Done,
        FactoryTechState::Researching { .. } => TechState::Researching,
        FactoryTechState::Available => TechState::Available,
        FactoryTechState::Locked(_) => TechState::Locked,
    };
    // Locked by earlier technologies (and not by the tier): the chain can be queued.
    let can_queue = match &t.state {
        FactoryTechState::Locked(r) => {
            t.queue_position.is_none()
                && r.iter().any(|x| matches!(x, LockReason::NeedsTech(_)))
                && !r.iter().any(|x| matches!(x, LockReason::NeedsTier { .. }))
        }
        _ => false,
    };
    TechEntry { id: t.id, state, progress: t.progress, reasons: t.reasons.clone(), queue_position: t.queue_position, can_queue }
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
        // The fuel slot of a campfire: a material slot with a fill bar.
        fuel: b
            .fuel
            .iter()
            .map(|f| BuildingSlot {
                stack: f.material.filter(|_| f.units > 0).map(|m| Stack { item: ItemRef::Material(m), count: f.units }),
                filter: f.hint.map(ItemRef::Material),
                capacity: f.capacity,
            })
            .collect(),
        buffers,
        progress: b.progress,
        speed: 1.0,
        power: None,
        temperature: Some(b.temperature as f32),
        milestone: milestone.map(milestone_view),
        later_stages: later.iter().map(milestone_view).collect(),
    }
}

/// Can the robot dig a material now, by the dig rules of `tools::dig` (for the hover box).
/// `None` for air, gas and fire. `limit` is the hardest material the drill head digs; `done` the
/// finished technologies (the next drill head technology is named when it is too hard).
pub fn dig_state(content: &Content, m: MaterialId, limit: u8, done: &[TechId]) -> Option<DigState> {
    let phase = content.materials.phase[m.index()];
    if m.is_air() || matches!(phase, Phase::Gas | Phase::Fire | Phase::Empty) {
        return None;
    }
    let h = content.materials.hardness[m.index()];
    if h == u8::MAX {
        return Some(DigState::Never);
    }
    if h <= limit {
        return Some(DigState::CanDig);
    }
    let levels = &crate::tools::DIG_HARDNESS;
    let (Some(need), Some(have)) = (levels.iter().position(|&x| x >= h), levels.iter().position(|&x| x >= limit)) else {
        return Some(DigState::Never);
    };
    // The drill head technologies that are not done yet, in data order; the one that reaches the
    // needed level.
    let techs: Vec<&str> = content
        .factory
        .techs
        .iter()
        .enumerate()
        .filter(|(i, t)| t.effects.iter().any(|(e, _)| e == "dig_hardness") && !done.contains(&TechId(*i as u16)))
        .map(|(_, t)| t.name.as_str())
        .collect();
    let needs = techs.get(need.saturating_sub(have + 1)).map_or("a better drill head".to_string(), |n| n.to_string());
    Some(DigState::TooHard { needs })
}

#[cfg(test)]
#[path = "normal_tests.rs"]
mod tests;
