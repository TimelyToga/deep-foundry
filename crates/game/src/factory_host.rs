//! The factory and the robot on the simulation thread (the normal game mode).
//!
//! `FactoryHost` owns the `Factory` and the robot. The simulation thread gives it the
//! `FactoryCommand`s from the main thread and calls `tick` after each cell update. After each
//! loop it publishes a `FactoryFrame` (the views the UI needs) to a `FactoryMailbox`.
//!
//! Views are sent only for what the UI shows:
//! - always: the robot, the inventory, the hand crafting queue, the current research, the ghost,
//!   the building under the mouse, and status marks for buildings in the view that do not work;
//! - while it is open: the building window;
//! - while the research window is open (every `TECH_PERIOD` ticks): all technologies;
//! - every `GUIDE_PERIOD` ticks, and when the guide window opens: the guide goals.
//!
//! The save of the normal mode is a side file next to the world file (`<name>.dfgame`, RON text).

use crate::player::{MoveInput, Robot};
use crate::tools;

#[path = "host_build.rs"]
mod build;
#[path = "factory_activity.rs"]
mod activity;
use foundry_content::{Content, ItemRef, Layer, Stack};
use foundry_core::{BuildingId, BuildingKindId, CellPos, CellRect, Command, MaterialId, PartId, RecipeId, TILE_SIZE, TechId, TilePos};
use foundry_factory::progress::{Discovered, LockReason, MilestoneView, ResearchStatus, TechView};
use foundry_factory::{
    BuildingView, Click, CraftJobView, Factory, FactoryEvent, FactorySave, GoalView, Guide, InventoryView, PartStack, PortView,
    ProgressEvent, RobotSlot, Status,
};
use foundry_sim::{AnchorId, SimEvent, Simulation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// File extension of the normal-mode side file.
pub const GAME_EXTENSION: &str = "dfgame";
/// Version of the side file format.
const SAVE_VERSION: u32 = 1;
/// Ticks between two guide views.
pub const GUIDE_PERIOD: u64 = 30;
/// Ticks between two technology lists while the research window is open.
pub const TECH_PERIOD: u64 = 10;
/// The same notice is not shown again within this many ticks.
const NOTICE_REPEAT: u64 = 120;
/// Quickbar slots.
pub const HOTBAR_SLOTS: usize = 20;
/// The HUD shows "Tanks full" for this many ticks after the dig tool found no room.
const TANKS_FULL_TICKS: u64 = 90;
/// The message when the dig tool finds no room in the tanks.
pub use foundry_ui::TANKS_FULL;
/// The building window closes when the building is this many cells farther than the reach.
const CLOSE_SLACK: i32 = 8;
/// Start inventory of a new game: materials in the tank, then parts.
const START_MATERIALS: [(&str, u32); 1] = [("wood", 30)];
const START_PARTS: [(&str, u32); 1] = [("crate", 1)];

/// A command for the simulation thread: a cell world command or a factory command.
#[derive(Debug, Clone)]
pub enum GameCommand {
    Sim(Command),
    Factory(FactoryCommand),
}

impl From<Command> for GameCommand {
    fn from(c: Command) -> Self {
        GameCommand::Sim(c)
    }
}

impl From<FactoryCommand> for GameCommand {
    fn from(c: FactoryCommand) -> Self {
        GameCommand::Factory(c)
    }
}

/// A building to show as a ghost at the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GhostRequest {
    pub kind: BuildingKindId,
    /// Top-left tile.
    pub at: TilePos,
    pub rotation: u8,
    pub flip: bool,
}

/// A building to place from the hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub kind: BuildingKindId,
    /// Top-left tile.
    pub at: TilePos,
    pub rotation: u8,
    pub flip: bool,
    /// Set this recipe after the placement (from the pipette).
    pub recipe: Option<RecipeId>,
    /// The player action (one mouse press) for undo. One drag line is one action. After a failed
    /// placement, the other placements of the same action are ignored, so a drag line stops
    /// there. 0: a new action of its own.
    pub action: u32,
}

impl Placement {
    pub fn new(kind: BuildingKindId, at: TilePos, rotation: u8) -> Self {
        Self { kind, at, rotation, flip: false, recipe: None, action: 0 }
    }
}

/// How to turn a placed building.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    Clockwise,
    CounterClockwise,
    To { rotation: u8, flip: bool },
}

/// The keys and the mouse of the player. The main thread sends it when it changes.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlayerInput {
    pub movement: MoveInput,
    /// The cell under the mouse.
    pub aim: CellPos,
    /// The dig button is down.
    pub dig: bool,
    /// The spray button is down, with this material.
    pub spray: Option<MaterialId>,
    /// The scan key is down.
    pub scan: bool,
    /// The building in the hand, at the mouse.
    pub ghost: Option<GhostRequest>,
    /// The cells on the screen (for the status marks).
    pub view: CellRect,
    /// The remove button is down (it went down on a building): remove the buildings under the
    /// path of the mouse, one after another. The number of that press (for undo).
    pub remove: Option<u32>,
    /// Alt mode: the frame has recipe icons and belt directions for the buildings in the view.
    pub alt: bool,
}

/// A slot group of a building window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotGroup {
    Input,
    Output,
    Fuel,
}

/// A slot that the player clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotTarget {
    /// A part slot of the player.
    Inventory(usize),
    /// A material tank of the player.
    Tank(usize),
    /// A slot of a building window.
    Building { id: BuildingId, group: SlotGroup, index: usize },
}

/// What the main thread asks the factory for.
#[derive(Debug, Clone, PartialEq)]
pub enum FactoryCommand {
    Input(PlayerInput),
    /// A click on a slot. Shift and Ctrl clicks move items between the player and the open
    /// building.
    Click { target: SlotTarget, click: Click },
    Craft { recipe: RecipeId, count: u32 },
    /// Cancel the hand crafting job at this place in the queue (its whole request).
    CancelCraft { index: usize },
    SetRecipe { building: BuildingId, recipe: Option<RecipeId> },
    /// Research this technology now (or queue it after the technologies it needs).
    StartResearch(TechId),
    /// Put the stack in the hand back into the inventory.
    ClearCursor,
    /// Take a stack of this part from the inventory into the hand.
    PickToCursor(PartId),
    /// Place the building in the hand.
    Place(Placement),
    /// Place the building in the hand at the first free place right of the robot (for tests and
    /// the smoke test).
    PlaceNear,
    /// Turn the placed building at this cell. `action` is for undo (0: an action of its own).
    Turn { at: CellPos, turn: Turn, action: u32 },
    /// Take back the last build, remove or turn action of the player.
    Undo,
    /// Do again the last action that undo took back.
    Redo,
    /// Remember the settings (the recipe) of the building at this cell.
    CopySettings(CellPos),
    /// Give the remembered settings to the building at this cell (the same building kind).
    PasteSettings(CellPos),
    /// Open the window of the building at this cell.
    OpenAt(CellPos),
    CloseBuilding,
    /// Which windows are open, so the host sends their views.
    Windows { research: bool, guide: bool },
    SetHotbar { index: usize, item: Option<ItemRef> },
    /// Delete the material in this robot tank (the trash button).
    EmptyTank(usize),
    /// Keep (true) or drop (false) this material when the robot digs.
    SetKeep { material: MaterialId, keep: bool },
    /// Fill the tanks, so that the tank of `material` has `room` units of room and no other
    /// tank has room (for the smoke test: it does not dig for minutes).
    FillTanks { material: MaterialId, room: u32 },
}

/// The ghost of the building in the hand, with the placement check.
#[derive(Debug, Clone, PartialEq)]
pub struct GhostView {
    pub request: GhostRequest,
    /// Size in tiles after the rotation.
    pub size: (u8, u8),
    /// `None`: it can be placed here. Else the reason, for the red ghost.
    pub error: Option<String>,
    pub ports: Vec<PortView>,
}

/// The building under the mouse.
#[derive(Debug, Clone, PartialEq)]
pub struct HoverBuilding {
    pub id: BuildingId,
    pub kind: BuildingKindId,
    pub rect: CellRect,
    pub rotation: u8,
    pub flip: bool,
    pub status: Status,
    /// Why it does not work (for the hover box).
    pub reason: String,
    /// Hit points now and at most.
    pub hit_points: (u32, u32),
    pub recipe: Option<RecipeId>,
    pub progress: f32,
    pub temperature: i16,
}

/// A building in the view that does not work (for the status icon over it), or, in the alt
/// mode, any building with a recipe or a direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildingMark {
    pub rect: CellRect,
    pub status: Status,
    /// Alt mode: the first product of its recipe.
    pub output: Option<ItemRef>,
    /// Alt mode: the belt direction (+1 right, -1 left), 0 for other buildings.
    pub belt: i8,
    /// An arm: the step from the building it takes from to the one it gives to. (0, 0) for other
    /// buildings.
    pub arm: (i8, i8),
    /// A back-layer building (a pipe): no outline.
    pub back: bool,
    /// The building's own icon, drawn faintly on it (not for walls, belts, pipes and arms).
    pub icon: Option<ItemRef>,
}

/// A drag line stopped: the placement at `at` failed for this reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragStop {
    pub action: u32,
    pub at: TilePos,
    pub reason: String,
}

/// The building that the remove button takes now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoveView {
    pub rect: CellRect,
    /// 0 to 1.
    pub progress: f32,
}

/// Everything the UI needs from the factory, made after each loop of the simulation thread.
#[derive(Debug, Clone, Default)]
pub struct FactoryFrame {
    pub tick: u64,
    pub robot: Option<Robot>,
    pub digging: bool,
    /// The material the tool dug in the last tick.
    pub dug_material: Option<MaterialId>,
    pub spraying: bool,
    /// The dig tool found no room in the tanks a moment ago.
    pub tanks_full: bool,
    /// The aim point of the tools, moved into reach.
    pub aim: CellPos,
    /// °C of the cell at the middle of the robot.
    pub robot_temperature: f32,
    pub inventory: InventoryView,
    /// The stack in the hand (the mouse cursor).
    pub cursor: Option<Stack>,
    pub hotbar: Vec<Option<ItemRef>>,
    pub crafting: Vec<CraftJobView>,
    pub craft_speed: f32,
    pub research: Option<ResearchStatus>,
    pub finished_techs: Vec<TechId>,
    pub discovery_points: u32,
    /// `Some` while the research window is open (not in every frame).
    pub techs: Option<Vec<TechView>>,
    /// `Some` when the guide was checked in this frame.
    pub guide: Option<Vec<GoalView>>,
    /// The open building window.
    pub building: Option<BuildingView>,
    /// The next Hub repair stage, when the open building is the Hub.
    pub milestone: Option<MilestoneView>,
    /// The Hub repair stages after the next one, when the open building is the Hub.
    pub later_milestones: Vec<MilestoneView>,
    pub ghost: Option<GhostView>,
    pub hover: Option<HoverBuilding>,
    pub marks: Vec<BuildingMark>,
    /// The building placed last and its cells (for tests; later for the build animation).
    pub last_placed: Option<(BuildingKindId, CellRect)>,
    /// Name tags over buildings in the view (the Hub and its next repair stage).
    pub labels: Vec<(CellRect, String)>,
    /// The last failed placement of a drag line (the main thread stops the line there).
    pub drag_stop: Option<DragStop>,
    /// The building that the remove button takes now, and the ones that wait after it.
    pub removing: Option<RemoveView>,
    pub remove_queue: Vec<CellRect>,
    /// The hardest material the drill head digs now (for the hover box).
    pub dig_limit: u8,
    /// Materials the player has discovered (scanned).
    pub discovered: Vec<MaterialId>,
    /// Dug materials and whether the robot keeps them (`Factory::dig_list`).
    pub dig_list: Vec<(MaterialId, bool)>,
    /// Messages for the player.
    pub notices: Vec<String>,
}

/// The newest `FactoryFrame`. Like `SnapshotMailbox`: the reader takes the newest one, and the
/// notices and the lists that are not in every frame are kept until it does.
#[derive(Default)]
pub struct FactoryMailbox {
    slot: Mutex<Option<FactoryFrame>>,
}

impl FactoryMailbox {
    pub fn publish(&self, mut frame: FactoryFrame) {
        let mut slot = self.slot.lock().unwrap();
        if let Some(mut old) = slot.take() {
            old.notices.append(&mut frame.notices);
            frame.notices = old.notices;
            if frame.guide.is_none() {
                frame.guide = old.guide;
            }
            if frame.techs.is_none() {
                frame.techs = old.techs;
            }
        }
        *slot = Some(frame);
    }

    pub fn take(&self) -> Option<FactoryFrame> {
        self.slot.lock().unwrap().take()
    }
}

/// An item in the save file (`ItemRef` has no serde).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum SavedItem {
    Material(MaterialId),
    Part(PartId),
}

impl From<ItemRef> for SavedItem {
    fn from(i: ItemRef) -> Self {
        match i {
            ItemRef::Material(m) => SavedItem::Material(m),
            ItemRef::Part(p) => SavedItem::Part(p),
        }
    }
}

impl From<SavedItem> for ItemRef {
    fn from(i: SavedItem) -> Self {
        match i {
            SavedItem::Material(m) => ItemRef::Material(m),
            SavedItem::Part(p) => ItemRef::Part(p),
        }
    }
}

/// The side file of a normal-mode save.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GameSave {
    version: u32,
    robot: Robot,
    hotbar: Vec<Option<SavedItem>>,
    factory: FactorySave,
}

/// The side file next to a world file.
pub fn side_file(world: &Path) -> PathBuf {
    world.with_extension(GAME_EXTENSION)
}

/// Buttons that went down in an input since the last tick. The main thread can send a press and
/// its release before one tick runs (a short tap, or a late tick). The tick then still acts on
/// the press, for one tick.
#[derive(Debug, Clone, Copy, Default)]
struct Taps {
    pub jump: bool,
    pub dig: bool,
    pub scan: bool,
    pub spray: Option<MaterialId>,
}

/// The factory, the robot and what the UI asked to see.
pub struct FactoryHost {
    pub factory: Factory,
    pub robot: Robot,
    pub hotbar: Vec<Option<ItemRef>>,
    input: PlayerInput,
    /// Presses since the last tick (see `Taps`).
    taps: Taps,
    /// The building whose window is open.
    open: Option<BuildingId>,
    research_open: bool,
    guide_open: bool,
    /// Send the guide in the next frame.
    guide_due: bool,
    /// The anchor that keeps the chunks around the robot in memory and awake.
    anchor: Option<AnchorId>,
    factory_activity: activity::FactoryActivity,
    /// The last material the scan tool looked at (so a held key reports it once).
    last_scan: Option<MaterialId>,
    notices: Vec<String>,
    /// When each notice text was last shown (tick), so a held tool does not repeat it every tick.
    shown: HashMap<String, u64>,
    ticks: u64,
    digging: bool,
    dug_material: Option<MaterialId>,
    spraying: bool,
    /// The dig tool found no room in the tanks; the HUD says so until this tick.
    tanks_full_until: u64,
    last_placed: Option<(BuildingKindId, CellRect)>,
    /// Buildings that were already put on the quickbar once (as in Factorio, a building goes to a
    /// free quickbar slot the first time the player gets it).
    on_quickbar: std::collections::BTreeSet<PartId>,
    /// The last ghost check: the request, its result and the tick of the check.
    ghost_check: Option<(GhostRequest, Option<String>, u64)>,
    /// Build, remove and turn actions of the player, for undo and redo.
    undo: build::UndoStack,
    /// The last failed placement. Other placements of its action are ignored.
    drag_stop: Option<DragStop>,
    /// The settings that Shift + right click copied: the building kind and its recipe.
    copied: Option<(BuildingKindId, Option<RecipeId>)>,
    /// The buildings that the remove button takes, in order, and the ticks spent on the first.
    removal: build::Removal,
    /// Numbers for actions that come without one (action 0). They count down from the top, so
    /// they do not meet the numbers of the main thread.
    own_actions: u32,
}

impl FactoryHost {
    fn with(factory: Factory, robot: Robot, hotbar: Vec<Option<ItemRef>>) -> Self {
        Self {
            factory,
            robot,
            hotbar,
            input: PlayerInput::default(),
            taps: Taps::default(),
            open: None,
            research_open: false,
            guide_open: false,
            guide_due: true,
            anchor: None,
            factory_activity: activity::FactoryActivity::default(),
            last_scan: None,
            notices: vec![],
            shown: HashMap::new(),
            ticks: 0,
            digging: false,
            dug_material: None,
            spraying: false,
            tanks_full_until: 0,
            last_placed: None,
            on_quickbar: Default::default(),
            ghost_check: None,
            undo: Default::default(),
            drag_stop: None,
            copied: None,
            removal: Default::default(),
            own_actions: u32::MAX,
        }
        .with_quickbar_seen()
    }

    /// Count the buildings on the quickbar and in the inventory as seen, so a loaded game does not
    /// put them on the quickbar again.
    fn with_quickbar_seen(mut self) -> Self {
        let contents = self.factory.player.contents();
        let parts = self.hotbar.iter().flatten().copied().chain(contents.iter().map(|s| s.item));
        let seen: Vec<PartId> = parts
            .filter_map(|i| match i {
                ItemRef::Part(p) => Some(p),
                ItemRef::Material(_) => None,
            })
            .collect();
        self.on_quickbar.extend(seen);
        self
    }

    /// Put new buildings from the inventory on a free quickbar slot.
    fn fill_quickbar(&mut self) {
        let content = self.factory.content.clone();
        let mut new = vec![];
        for s in self.factory.player.slots.iter().flatten() {
            if content.factory.part_def(s.part).building.is_some() && !self.on_quickbar.contains(&s.part) && !new.contains(&s.part) {
                new.push(s.part);
            }
        }
        for p in new {
            self.on_quickbar.insert(p);
            if self.hotbar.contains(&Some(ItemRef::Part(p))) {
                continue;
            }
            if let Some(slot) = self.hotbar.iter_mut().find(|h| h.is_none()) {
                *slot = Some(ItemRef::Part(p));
            }
        }
    }

    /// A new normal game: place the broken Hub near `spawn_x`, give the start inventory, and put
    /// the robot on the ground next to the Hub.
    pub fn new_game(content: Arc<Content>, guide: Arc<Guide>, sim: &mut Simulation, spawn_x: i32) -> Result<Self, String> {
        let mut factory = Factory::new(content.clone());
        factory.guide = guide;
        let hub = content.factory.building("hub").ok_or("the data has no building `hub`")?;
        let size = content.factory.building_def(hub).size;
        let (w, h) = (size.0 as i32 * TILE_SIZE, size.1 as i32 * TILE_SIZE);
        let x0 = (spawn_x - w / 2).div_euclid(TILE_SIZE) * TILE_SIZE;
        // The Hub stands on the ground: its bottom row is at the median ground level of its
        // columns (rounded to a tile). Ground in the footprint is removed; holes under it are
        // filled with dirt.
        let mut tops: Vec<i32> = (x0..x0 + w).map(|x| ground_top(sim, &content, x)).collect();
        tops.sort_unstable();
        let bottom = (tops[tops.len() / 2] as f32 / TILE_SIZE as f32).round() as i32 * TILE_SIZE;
        let at = TilePos::new(x0 / TILE_SIZE, (bottom - h) / TILE_SIZE);
        let dirt = content.material("dirt").unwrap_or(MaterialId::AIR);
        for x in x0..x0 + w {
            for y in bottom - h - 8..bottom {
                sim.set_cell(CellPos::new(x, y), MaterialId::AIR, None);
            }
            for y in bottom..bottom + 40 {
                let p = CellPos::new(x, y);
                if crate::player::blocks(&content, sim.cell(p).material) {
                    break;
                }
                sim.set_cell(p, dirt, None);
            }
        }
        factory.place(hub, at, 0, false, sim).map_err(|e| format!("cannot place the Hub: {e}"))?;
        for (id, n) in START_MATERIALS {
            if let Some(m) = content.material(id) {
                factory.player.insert(&content, ItemRef::Material(m), n);
            }
        }
        for (id, n) in START_PARTS {
            if let Some(p) = content.factory.part(id) {
                factory.player.insert(&content, ItemRef::Part(p), n);
            }
        }
        // The robot stands on the ground right of the Hub.
        let rx = x0 + w + 12;
        let feet = CellPos::new(rx, ground_top(sim, &content, rx));
        let robot = Robot::standing_at(feet);
        factory.player_pos = Some(robot.center_cell());
        let mut hotbar = vec![None; HOTBAR_SLOTS];
        if let Some(p) = content.factory.part("crate") {
            hotbar[0] = Some(ItemRef::Part(p));
        }
        let mut host = Self::with(factory, robot, hotbar);
        host.update_anchor(sim);
        Ok(host)
    }

    /// The save data of the factory and the robot, as RON text.
    pub fn save_text(&self) -> Result<String, String> {
        let save = GameSave {
            version: SAVE_VERSION,
            robot: self.robot,
            hotbar: self.hotbar.iter().map(|h| h.map(SavedItem::from)).collect(),
            factory: self.factory.save(),
        };
        ron::ser::to_string(&save).map_err(|e| e.to_string())
    }

    /// Read the save data from RON text.
    pub fn from_save_text(content: Arc<Content>, guide: Arc<Guide>, text: &str) -> Result<Self, String> {
        let save: GameSave = ron::from_str(text).map_err(|e| format!("the game file is damaged: {e}"))?;
        if save.version != SAVE_VERSION {
            return Err(format!("the game file has version {}; this game reads version {SAVE_VERSION}", save.version));
        }
        let mut factory = Factory::new(content);
        factory.guide = guide;
        factory.load(save.factory);
        let mut hotbar: Vec<Option<ItemRef>> = save.hotbar.into_iter().map(|h| h.map(ItemRef::from)).collect();
        hotbar.resize(HOTBAR_SLOTS, None);
        Ok(Self::with(factory, save.robot, hotbar))
    }

    /// Write the side file next to a world file. Returns an error message for the player.
    pub fn save_file(&self, world: &Path) -> Result<(), String> {
        let text = self.save_text()?;
        std::fs::write(side_file(world), text).map_err(|e| format!("Save failed: cannot write the game file: {e}"))
    }

    /// Read the side file next to a world file. `Ok(None)`: there is no side file (a sandbox save).
    pub fn load_file(content: Arc<Content>, guide: Arc<Guide>, world: &Path) -> Result<Option<Self>, String> {
        let path = side_file(world);
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Self::from_save_text(content, guide, &text).map(Some)
    }

    fn notice(&mut self, text: impl Into<String>) {
        let text = text.into();
        if let Some(&t) = self.shown.get(&text)
            && self.ticks < t + NOTICE_REPEAT
        {
            return;
        }
        self.shown.insert(text.clone(), self.ticks);
        self.notices.push(text);
    }

    fn update_anchor(&mut self, sim: &mut Simulation) {
        self.factory_activity.sync(&self.factory, sim);
        let area = self.robot.rect().expand(4 * TILE_SIZE);
        match self.anchor {
            Some(id) if sim.move_anchor(id, area) => {}
            _ => self.anchor = Some(sim.add_anchor(area)),
        }
    }

    // ------------------------------------------------------------ commands

    /// Apply one command from the main thread.
    pub fn apply(&mut self, cmd: FactoryCommand, sim: &mut Simulation) {
        match cmd {
            FactoryCommand::Input(i) => self.set_input(i),
            FactoryCommand::Click { target, click } => self.click(target, click),
            FactoryCommand::Craft { recipe, count } => {
                if let Err(e) = self.factory.craft(recipe, count) {
                    self.notice(format!("Cannot craft: {e}"));
                }
            }
            FactoryCommand::CancelCraft { index } => {
                if let Some(job) = self.factory.crafting_view().get(index) {
                    let left = self.factory.cancel_craft(job.request);
                    self.drop_near_robot(sim, &left);
                }
            }
            FactoryCommand::SetRecipe { building, recipe } => match self.factory.set_recipe(building, recipe) {
                Ok(left) => self.drop_near_robot(sim, &left),
                Err(e) => self.notice(e.to_string()),
            },
            FactoryCommand::StartResearch(tech) => self.start_research(tech),
            FactoryCommand::ClearCursor => self.clear_cursor(),
            FactoryCommand::PickToCursor(part) => self.pick_to_cursor(part),
            FactoryCommand::Place(p) => self.place(p, sim),
            FactoryCommand::PlaceNear => {
                let content = self.factory.content.clone();
                let kind = self.factory.cursor.and_then(|c| content.factory.part_def(c.part).building);
                match kind.and_then(|k| self.free_place(k, sim).map(|at| (k, at))) {
                    Some((kind, at)) => self.place(Placement::new(kind, at, 0), sim),
                    None => self.notice("No free place for the building in the hand"),
                }
            }
            FactoryCommand::Turn { at, turn, action } => self.turn_at(at, turn, action),
            FactoryCommand::Undo => self.undo(sim),
            FactoryCommand::Redo => self.redo(sim),
            FactoryCommand::CopySettings(p) => self.copy_settings(p),
            FactoryCommand::PasteSettings(p) => self.paste_settings(p, sim),
            FactoryCommand::OpenAt(p) => {
                if let Some(id) = self.building_at(p) {
                    match self.factory.buildings.get(id).map(|b| b.cell_rect()) {
                        Some(r) if self.out_of_reach(r) => self.notice("Out of reach"),
                        _ => self.open = Some(id),
                    }
                }
            }
            FactoryCommand::CloseBuilding => self.open = None,
            FactoryCommand::Windows { research, guide } => {
                self.research_open = research;
                if guide && !self.guide_open {
                    self.guide_due = true;
                }
                self.guide_open = guide;
            }
            FactoryCommand::SetHotbar { index, item } => {
                if let Some(slot) = self.hotbar.get_mut(index) {
                    *slot = item;
                }
            }
            FactoryCommand::FillTanks { material, room } => self.fill_tanks(material, room),
            FactoryCommand::SetKeep { material, keep } => self.factory.set_keep(material, keep),
            FactoryCommand::EmptyTank(tank) => {
                let content = self.factory.content.clone();
                let material = self.factory.player.tanks.get(tank).and_then(|t| t.material);
                let n = self.factory.empty_tank(tank);
                if let Some(m) = material.filter(|_| n > 0) {
                    self.notice(format!("Emptied a tank: {n} units of {} are gone", content.materials.names[m.index()]));
                }
            }
        }
    }

    /// See `FactoryCommand::FillTanks`. Empty tanks get materials that no tank has yet.
    fn fill_tanks(&mut self, material: MaterialId, room: u32) {
        let content = self.factory.content.clone();
        // Materials that no guide goal counts.
        let fillers = ["dirt", "gravel", "ash", "stone", "granite", "basalt", "snow", "peat", "leaves", "clay"];
        let mut fillers = fillers.iter().filter_map(|id| content.material(id));
        let tanks = &mut self.factory.player.tanks;
        for i in 0..tanks.len() {
            let m = match tanks[i].material {
                Some(m) => m,
                None => match fillers.find(|f| *f != material && !tanks.iter().any(|t| t.material == Some(*f))) {
                    Some(f) => f,
                    None => continue,
                },
            };
            tanks[i].material = Some(m);
            tanks[i].units = tanks[i].capacity;
        }
        if let Some(t) = tanks.iter_mut().find(|t| t.material == Some(material)) {
            t.units = t.capacity.saturating_sub(room);
        }
    }

    /// The building on the tile of a cell (the front layer first).
    pub fn building_at(&self, p: CellPos) -> Option<BuildingId> {
        let b = &self.factory.buildings;
        b.at_tile(p.tile(), Layer::Front).or_else(|| b.at_tile(p.tile(), Layer::Back))
    }

    fn click(&mut self, target: SlotTarget, click: Click) {
        let content = self.factory.content.clone();
        let open = self.open;
        // The rules are in `foundry_factory::transfer`.
        let result = match target {
            SlotTarget::Inventory(slot) => self.factory.click_robot_slot(RobotSlot::Part(slot), click, open),
            SlotTarget::Tank(tank) => self.factory.click_robot_slot(RobotSlot::Tank(tank), click, open),
            SlotTarget::Building { id, group: SlotGroup::Input, index } if self.factory.buildings.inventory(id).is_some() => {
                self.factory.click_storage_slot(id, index, click)
            }
            SlotTarget::Building { .. } => Ok(false),
        };
        if let Err(text) = result {
            self.notice(text);
        }
        // A machine window: the hand into an input slot, or an output slot to the robot.
        let SlotTarget::Building { id, group, index } = target else { return };
        if self.factory.buildings.inventory(id).is_some() {
            return;
        }
        let Some(view) = self.factory.building_view(id) else { return };
        match group {
            SlotGroup::Input => {
                let Some(buf) = view.inputs.get(index) else { return };
                self.input_slot_click(id, buf.item, buf.count, click);
            }
            SlotGroup::Output => {
                let Some(buf) = view.outputs.get(index) else { return };
                let room = self.factory.player.room_for(&content, buf.item, buf.count);
                let n = self.factory.buildings.take_output(&content, id, buf.item, room);
                self.factory.player.insert(&content, buf.item, n);
                if n < buf.count && room < buf.count {
                    self.notice("The inventory is full");
                }
            }
            SlotGroup::Fuel => {
                if let Err(text) = self.factory.fuel_to_robot(id, click) {
                    self.notice(text);
                }
            }
        }
    }

    /// A click on a machine input slot, as in Factorio:
    /// - parts in the hand: put them into the machine (a right click puts one);
    /// - an empty hand: take the parts into the hand (a right click takes half);
    /// - Shift or Ctrl: move the items of the slot into the inventory.
    ///
    /// Materials cannot be in the hand, so a click with an empty hand moves them into the tank.
    fn input_slot_click(&mut self, id: BuildingId, item: ItemRef, count: u32, click: Click) {
        let content = self.factory.content.clone();
        let empty_hand = self.factory.cursor.is_none();
        if matches!(click, Click::Shift | Click::Ctrl) || (empty_hand && matches!(item, ItemRef::Material(_))) {
            let room = self.factory.player.room_for(&content, item, count);
            let n = self.factory.buildings.take_input(&content, id, item, room);
            self.factory.player.insert(&content, item, n);
            if n < count && room < count {
                self.notice("The inventory is full");
            }
            return;
        }
        let Some(c) = self.factory.cursor else {
            let ItemRef::Part(part) = item else { return };
            let want = if click == Click::Right { count.div_ceil(2) } else { count };
            let want = want.min(foundry_factory::inventory::stack_size(&content, part));
            let n = self.factory.buildings.take_input(&content, id, item, want);
            if n > 0 {
                self.factory.cursor = Some(PartStack::new(part, n));
            }
            return;
        };
        if ItemRef::Part(c.part) != item {
            self.notice(format!("This slot takes {}", content.item_name(item)));
            return;
        }
        let want = if click == Click::Right { 1 } else { c.count };
        let n = want.min(self.factory.buildings.room_for(&content, id, item));
        let n = self.factory.buildings.insert(&content, id, item, n);
        let left = c.count - n;
        self.factory.cursor = (left > 0).then_some(PartStack::new(c.part, left));
    }

    fn start_research(&mut self, tech: TechId) {
        let content = self.factory.content.clone();
        let p = &mut self.factory.progress;
        let reasons = p.lock_reasons(&content, tech);
        let result = if reasons.is_empty() {
            p.start_research(&content, tech)
        } else if reasons.iter().any(|r| matches!(r, LockReason::NeedsTech(_))) {
            // As in Factorio: queue the technologies it needs, then this one. The queue starts
            // each one when it can (tier, points and discoveries are checked then).
            p.queue_research(&content, tech)
        } else {
            Err(reasons[0].clone())
        };
        if let Err(r) = result {
            self.notice(r.text(&content));
        }
    }

    fn clear_cursor(&mut self) {
        let content = self.factory.content.clone();
        let Some(c) = self.factory.cursor.take() else { return };
        let left = self.factory.player.insert(&content, ItemRef::Part(c.part), c.count);
        if left > 0 {
            self.factory.cursor = Some(PartStack::new(c.part, left));
            self.notice("The inventory is full");
        }
    }

    fn pick_to_cursor(&mut self, part: PartId) {
        let content = self.factory.content.clone();
        if self.factory.cursor.is_some_and(|c| c.part == part) {
            return;
        }
        self.clear_cursor();
        if self.factory.cursor.is_some() {
            return;
        }
        let size = foundry_factory::inventory::stack_size(&content, part);
        let n = self.factory.player.remove(ItemRef::Part(part), size);
        if n == 0 {
            self.notice(format!("No {} in the inventory", content.factory.part_def(part).name));
            return;
        }
        self.factory.cursor = Some(PartStack::new(part, n));
    }

    /// Too far from the robot for building and removing.
    fn out_of_reach(&self, r: CellRect) -> bool {
        let (cx, cy) = self.robot.center();
        let dx = (r.x0 as f32 - cx).max(cx - r.x1 as f32).max(0.0);
        let dy = (r.y0 as f32 - cy).max(cy - r.y1 as f32).max(0.0);
        (dx * dx + dy * dy).sqrt() > tools::REACH
    }

    /// Check a placement: the placement rules, reach, and the robot's own body.
    pub fn check_place(&self, kind: BuildingKindId, at: TilePos, rotation: u8, flip: bool, sim: &Simulation) -> Result<(), String> {
        self.check_place_reach(kind, at, rotation, flip, true, sim)
    }

    /// `check_place`; `reach` false skips the reach check (undo and redo work at any distance).
    fn check_place_reach(&self, kind: BuildingKindId, at: TilePos, rotation: u8, flip: bool, reach: bool, sim: &Simulation) -> Result<(), String> {
        let content = &self.factory.content;
        let Some(def) = content.factory.buildings.get(kind.0 as usize) else { return Err("Unknown building".into()) };
        self.factory.can_place(kind, at, rotation, flip, sim).map_err(|e| match e {
            // The robot cannot dig this material yet: say so, not "dig first".
            foundry_factory::PlaceError::Blocked { material, name, can_dig: true, .. }
                if content.materials.hardness[material.index()] > tools::dig_limit(&self.factory) =>
            {
                format!("Blocked by {}: too hard to dig with this drill head", name.to_lowercase())
            }
            e => e.to_string(),
        })?;
        let size = if rotation & 1 == 1 { (def.size.1, def.size.0) } else { def.size };
        let r = CellRect::new(
            at.x * TILE_SIZE,
            at.y * TILE_SIZE,
            (at.x + size.0 as i32) * TILE_SIZE,
            (at.y + size.1 as i32) * TILE_SIZE,
        );
        if reach && self.out_of_reach(r) {
            return Err("Out of reach".into());
        }
        if def.layer == Layer::Front && !r.intersect(&self.robot.rect()).is_empty() {
            return Err("The robot is in the way".into());
        }
        Ok(())
    }

    /// The first free place right of the robot for a building (rotation 0), near its feet: the
    /// lowest place where the building fits and at most a quarter of its cells are not empty
    /// (loose ground that the placement pushes away).
    pub fn free_place(&self, kind: BuildingKindId, sim: &Simulation) -> Option<TilePos> {
        let content = &self.factory.content;
        let r = self.robot.rect();
        let size = content.factory.building_def(kind).size;
        let empty = |at: TilePos| {
            let (x0, y0) = (at.x * TILE_SIZE, at.y * TILE_SIZE);
            let (w, h) = (size.0 as i32 * TILE_SIZE, size.1 as i32 * TILE_SIZE);
            let full = (y0..y0 + h)
                .flat_map(|y| (x0..x0 + w).map(move |x| CellPos::new(x, y)))
                .filter(|p| {
                    let m = sim.cell(*p).material;
                    !matches!(content.materials.phase[m.index()], foundry_content::Phase::Empty | foundry_content::Phase::Gas)
                })
                .count() as i32;
            full * 4 <= w * h
        };
        for dx in 1..12 {
            for dy in (-3..=3).rev() {
                let at = TilePos::new(r.x1.div_euclid(TILE_SIZE) + dx, r.y1.div_euclid(TILE_SIZE) - size.1 as i32 + dy);
                if empty(at) && self.check_place(kind, at, 0, false, sim).is_ok() {
                    return Some(at);
                }
            }
        }
        None
    }

    /// Items that did not fit into the inventory: materials become cells near the robot.
    fn drop_near_robot(&mut self, sim: &mut Simulation, stacks: &[Stack]) {
        if stacks.is_empty() {
            return;
        }
        let content = self.factory.content.clone();
        let left = foundry_factory::Buildings::drop_as_cells(&content, sim, self.robot.rect(), stacks);
        self.notice(if left.is_empty() { "The inventory is full: some items fell out" } else { "The inventory is full: some items are lost" });
    }

    // ------------------------------------------------------------ tick

    /// One tick after the cell update: the robot moves, the tools work, then the factory ticks.
    pub fn tick(&mut self, sim: &mut Simulation) {
        self.ticks += 1;
        let content = self.factory.content.clone();
        // The held buttons, plus the presses since the last tick (see `Taps`).
        let taps = std::mem::take(&mut self.taps);
        let movement = MoveInput { jump: self.input.movement.jump || taps.jump, ..self.input.movement };
        let (dig, scan, spray) = (self.input.dig || taps.dig, self.input.scan || taps.scan, self.input.spray.or(taps.spray));
        self.robot.step(movement, sim);
        self.update_anchor(sim);
        self.factory.player_pos = Some(self.robot.center_cell());

        self.digging = false;
        self.dug_material = None;
        self.spraying = false;
        if dig {
            let r = tools::dig(&mut self.factory, sim, &self.robot, self.input.aim);
            self.digging = r.dug > 0;
            if r.tank_full.is_some() {
                self.tanks_full_until = self.ticks + TANKS_FULL_TICKS;
            }
            self.dug_material = r.material;
            // Full tanks first: the player can do something about it now. (Dropped cells still
            // dig, so this can happen while other cells are dug.)
            if r.tank_full.is_some() {
                self.notice(TANKS_FULL);
            } else if r.dug == 0 {
                if let Some(m) = r.too_hard {
                    self.notice(format!("{} is too hard: research a better drill head", content.materials.names[m.index()]));
                } else if let Some(m) = r.too_hot {
                    self.notice(format!("{} is too hot for the tank", content.materials.names[m.index()]));
                }
            }
        }
        if let Some(m) = spray {
            self.spraying = tools::spray(&mut self.factory, sim, &self.robot, self.input.aim, m) > 0;
        }
        if scan {
            self.scan(sim);
        } else {
            self.last_scan = None;
        }
        self.tick_removal(sim);

        self.factory.tick(sim);
        // Reactions of the last simulation tick: the player discovers the ones near the robot.
        for e in sim.events() {
            if let SimEvent::Reaction { index, at } = *e {
                self.factory.observe_reaction(index, at);
            }
        }
        for e in self.factory.take_events() {
            match e {
                FactoryEvent::Broke { kind, .. } => {
                    self.notice(format!("{} broke", content.factory.building_def(kind).name));
                }
            }
        }
        let events: Vec<ProgressEvent> = self.factory.progress.drain_events().collect();
        for e in events {
            let text = progress_text(&content, &self.factory.guide, &e);
            self.notice(text);
            if matches!(e, ProgressEvent::GoalDone(_) | ProgressEvent::StageDone { .. } | ProgressEvent::TechDone(_)) {
                self.guide_due = true;
            }
        }
        if self.factory.buildings.now().is_multiple_of(GUIDE_PERIOD) {
            self.guide_due = true;
        }
        if self.ticks.is_multiple_of(15) {
            self.fill_quickbar();
        }
    }

    fn scan(&mut self, sim: &Simulation) {
        let content = self.factory.content.clone();
        let Some((m, found)) = tools::scan(&mut self.factory, sim, &self.robot, self.input.aim) else { return };
        if self.last_scan == Some(m) && found.is_empty() {
            return;
        }
        self.last_scan = Some(m);
        if found.is_empty() {
            self.notices.push(format!("Scan: {} (known)", content.materials.names[m.index()]));
        }
        // New discoveries are reported by the progress events.
    }

    // ------------------------------------------------------------ views

    /// The views for the UI. Call it after the commands and the tick of each loop.
    pub fn frame(&mut self, sim: &Simulation, tick: u64) -> FactoryFrame {
        let content = self.factory.content.clone();
        let f = &self.factory;
        let cursor = f.cursor.map(|c| c.to_stack());
        // As in Factorio, the building window closes when the robot walks out of reach.
        let in_reach = |id: BuildingId| f.buildings.get(id).is_some_and(|b| !self.out_of_reach(b.cell_rect().expand(CLOSE_SLACK)));
        let building = self.open.filter(|id| in_reach(*id)).and_then(|id| f.building_view(id));
        if building.is_none() {
            self.open = None;
        }
        let is_hub = building.as_ref().is_some_and(|b| content.factory.building_def(b.kind).kind == "hub");
        let milestone = if is_hub { f.progress.milestone_view(&content) } else { None };
        let later_milestones = if is_hub { f.progress.later_milestone_views(&content) } else { vec![] };
        let techs = (self.research_open && tick.is_multiple_of(TECH_PERIOD)).then(|| f.progress.tech_views(&content));
        let guide = self.guide_due.then(|| f.guide_view());
        self.guide_due = false;
        // The placement check can search many cells (loose cells to push away), so it runs when
        // the ghost moves and then only a few times per second.
        let ghost = self.input.ghost.map(|g| {
            let error = match &self.ghost_check {
                Some((r, e, t)) if *r == g && self.ticks < t + 10 => e.clone(),
                _ => {
                    let e = self.check_place(g.kind, g.at, g.rotation, g.flip, sim).err();
                    self.ghost_check = Some((g, e.clone(), self.ticks));
                    e
                }
            };
            let def = content.factory.building_def(g.kind);
            let size = if g.rotation & 1 == 1 { (def.size.1, def.size.0) } else { def.size };
            GhostView { request: g, size, error, ports: self.factory.ghost_ports(g.kind, g.at, g.rotation, g.flip) }
        });
        let f = &self.factory;
        let hover = self.building_at(self.input.aim).and_then(|id| {
            let b = f.buildings.get(id)?;
            let v = f.building_view(id)?;
            Some(HoverBuilding {
                id,
                kind: b.kind,
                rect: b.cell_rect(),
                rotation: b.transform.rotation,
                flip: b.transform.flip,
                reason: v.reason.clone(),
                hit_points: (v.hit_points, v.max_hit_points),
                status: b.status,
                recipe: v.recipe,
                progress: v.progress,
                temperature: v.temperature,
            })
        });
        let view = self.input.view;
        let alt = self.input.alt;
        let mut marks: Vec<BuildingMark> = f
            .buildings
            .iter()
            .filter(|(_, b)| !b.cell_rect().intersect(&view).is_empty())
            .map(|(id, b)| {
                let output = if alt { mark_output(f, id, b) } else { None };
                let belt = match &b.logic {
                    foundry_factory::Logic::Belt(_) if alt => foundry_factory::buildings::belt_direction(b.transform) as i8,
                    _ => 0,
                };
                let arm = match &b.logic {
                    foundry_factory::Logic::Arm(_) => {
                        let (dx, dy) = foundry_factory::geometry::side_step(b.transform.side(foundry_content::Side::Right));
                        (dx as i8, dy as i8)
                    }
                    _ => (0, 0),
                };
                let def = f.content.factory.building_def(b.kind);
                let plain = matches!(def.kind.as_str(), "wall" | "room_wall" | "belt" | "pipe" | "arm" | "hub");
                let icon = (!plain).then_some(ItemRef::Part(def.part));
                // Every building in the view: the overlay draws its outline.
                BuildingMark { rect: b.cell_rect(), status: b.status, output, belt, arm, back: b.layer == foundry_content::Layer::Back, icon }
            })
            .collect();
        // The hole or the wrong wall block of a room: a red mark on that tile.
        for (_, b) in f.buildings.iter() {
            let tile = b.room.as_deref().and_then(|r| r.problem.as_ref()).and_then(|p| p.tile());
            if let Some(t) = tile {
                let rect = CellRect::new(t.x * TILE_SIZE, t.y * TILE_SIZE, (t.x + 1) * TILE_SIZE, (t.y + 1) * TILE_SIZE);
                if !rect.intersect(&view).is_empty() {
                    marks.push(BuildingMark { rect, status: Status::NoRoom, output: None, belt: 0, arm: (0, 0), back: true, icon: None });
                }
            }
        }
        let labels = f
            .buildings
            .iter()
            .filter(|(_, b)| content.factory.building_def(b.kind).kind == "hub" && !b.cell_rect().intersect(&view).is_empty())
            .map(|(_, b)| {
                let text = match f.progress.next_milestone(&content) {
                    Some(m) => format!("Hub: needs repair stage {}", m.stage),
                    None => "Hub".to_string(),
                };
                (b.cell_rect(), text)
            })
            .collect();
        FactoryFrame {
            tick,
            robot: Some(self.robot),
            digging: self.digging,
            dug_material: self.dug_material,
            spraying: self.spraying,
            tanks_full: self.ticks < self.tanks_full_until,
            aim: tools::clamp_aim(&self.robot, self.input.aim),
            robot_temperature: sim.cell(self.robot.center_cell()).temperature as f32,
            inventory: f.player_view(),
            cursor,
            hotbar: self.hotbar.clone(),
            crafting: f.crafting_view(),
            craft_speed: f.player_pos.map_or(1.0, |p| f.buildings.hand_speed(&content, p)),
            research: f.progress.research_status(&content),
            finished_techs: f.progress.researched().collect(),
            discovery_points: f.progress.discovery_points(),
            techs,
            guide,
            building,
            milestone,
            later_milestones,
            ghost,
            hover,
            marks,
            last_placed: self.last_placed,
            labels,
            drag_stop: self.drag_stop.clone(),
            removing: self.removal_view(),
            remove_queue: self.removal_queue_rects(),
            dig_limit: tools::dig_limit(f),
            discovered: f.progress.discovered_materials().collect(),
            dig_list: f.dig_list(),
            notices: std::mem::take(&mut self.notices),
        }
    }
}

/// The first product of the recipe of a machine, for the alt mode icon.
fn mark_output(f: &Factory, id: BuildingId, b: &foundry_factory::Building) -> Option<ItemRef> {
    let foundry_factory::Logic::Machine(m) = &b.logic else { return None };
    let _ = id;
    let recipe = f.content.factory.recipe_def(m.recipe?);
    recipe.outputs.first().map(|s| s.item)
}

/// The top of the ground in a column: the first cell from the top that stops the robot.
pub(crate) fn ground_top(sim: &Simulation, content: &Content, x: i32) -> i32 {
    let (_, h) = sim.size_cells();
    (0..h).find(|&y| crate::player::blocks(content, sim.cell(CellPos::new(x, y)).material)).unwrap_or(h)
}

/// A message for a progress event.
fn progress_text(content: &Content, guide: &Guide, e: &ProgressEvent) -> String {
    let points = |n: u32| if n == 1 { "1 discovery point".to_string() } else { format!("{n} discovery points") };
    match e {
        ProgressEvent::TechDone(t) => format!("Research done: {}", content.factory.tech_def(*t).name),
        ProgressEvent::StageDone { stage, tier } => format!("Hub repair stage {stage} is done. Tier {tier} is open."),
        ProgressEvent::Discovery { found: Discovered::Material(m), points: n } => {
            format!("Discovered: {} (+{})", content.materials.names[m.index()], points(*n))
        }
        ProgressEvent::Discovery { found: Discovered::Reaction(key), points: n } => {
            format!("Discovered a reaction: {} (+{})", foundry_factory::progress::discovery_name(content, key), points(*n))
        }
        ProgressEvent::GoalDone(id) => {
            let title = guide.goals.iter().find(|g| &g.id == id).map_or(id.as_str(), |g| g.title.as_str());
            format!("Guide goal done: {title}")
        }
    }
}

/// After the world file is written: write the side file of a normal game, or remove an old side
/// file (a sandbox save must not load an old factory). Adds a message for the player on an error.
pub fn save_side(host: Option<&FactoryHost>, world: &Path, notices: &mut Vec<String>) {
    let result = match host {
        Some(h) => h.save_file(world),
        None => match std::fs::remove_file(side_file(world)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Save failed: cannot remove the old game file: {e}")),
            _ => Ok(()),
        },
    };
    if let Err(e) = result {
        notices.push(e);
    }
}

#[cfg(test)]
#[path = "factory_host_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tier0_tests.rs"]
mod tier0_tests;
