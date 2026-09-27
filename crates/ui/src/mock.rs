//! Test data: a realistic `UiModel` for Tier 0 and Tier 1 (from `docs/design/02-content.md`),
//! and [`MockGame`], which applies `UiAction`s to the model the way the game will.
//!
//! The preview example and the snapshot tests use this. The real item and recipe data comes
//! in Milestone 3-4; then the game builds the `Catalog` from the data files instead.
//!
//! Bulk materials use the real material ids from `assets/data/materials/*.ron`.

use crate::action::{SettingChange, SlotRef, UiAction, WindowKind};
use crate::crafting::{Stock, craftable_count};
use crate::graph::{SAMPLES, TimeSeries};
use crate::item::*;
use crate::model::*;
use crate::slots::{self, SlotList};
use foundry_content::{Content, Phase};
use foundry_core::{BuildingId, BuildingKindId, CellPos, MaterialId, RecipeId};

/// Part ids of the mock catalog.
pub mod part {
    use crate::item::{ItemId, PartId};
    const fn p(n: u16) -> ItemId {
        ItemId::Part(PartId(n))
    }
    pub const RAW_CLAY_BRICK: ItemId = p(1);
    pub const CLAY_BRICK: ItemId = p(2);
    pub const RAW_FIREBRICK: ItemId = p(3);
    pub const FIREBRICK: ItemId = p(4);
    pub const TIN_INGOT: ItemId = p(5);
    pub const COPPER_INGOT: ItemId = p(6);
    pub const BRONZE_INGOT: ItemId = p(7);
    pub const TIN_PLATE: ItemId = p(8);
    pub const COPPER_PLATE: ItemId = p(9);
    pub const BRONZE_PLATE: ItemId = p(10);
    pub const STEEL_PLATE: ItemId = p(11);
    pub const BRONZE_GEAR: ItemId = p(12);
    pub const STEEL_GEAR: ItemId = p(13);
    pub const COPPER_ROD: ItemId = p(14);
    pub const BRONZE_ROD: ItemId = p(15);
    pub const STEEL_ROD: ItemId = p(16);
    pub const COPPER_WIRE: ItemId = p(17);
    pub const TIN_WIRE: ItemId = p(18);
    pub const BRONZE_PIPE: ItemId = p(19);
    pub const IRON_PIPE: ItemId = p(20);
    pub const STEEL_BOLT: ItemId = p(21);
    pub const GLASS_PANE: ItemId = p(22);
    pub const GLASS_VIAL: ItemId = p(23);
    pub const GLASS_TUBE: ItemId = p(24);
    pub const RUBBER_SHEET: ItemId = p(25);
    pub const CIRCUIT_BOARD: ItemId = p(26);
    pub const VACUUM_TUBE: ItemId = p(27);
    pub const BRONZE_KIT: ItemId = p(28);
    pub const STEAM_KIT: ItemId = p(29);
}

/// Building types of the mock catalog.
pub mod bld {
    use foundry_core::BuildingKindId as K;
    pub const WORKBENCH: K = K(1);
    pub const CAMPFIRE: K = K(2);
    pub const WOOD_BLOCK: K = K(3);
    pub const LADDER: K = K(4);
    pub const BRICK_WALL: K = K(5);
    pub const KILN_CONTROLLER: K = K(6);
    pub const KILN_HATCH: K = K(7);
    pub const BELLOWS: K = K(8);
    pub const CRUCIBLE: K = K(9);
    pub const INGOT_MOLD: K = K(10);
    pub const PLATE_MOLD: K = K(11);
    pub const STAMP_MILL: K = K(12);
    pub const SLUICE: K = K(13);
    pub const HOPPER: K = K(14);
    pub const WOOD_BELT: K = K(15);
    pub const BARREL: K = K(16);
    pub const CRATE: K = K(17);
    pub const BASIC_LAB: K = K(18);
    pub const SMALL_BOILER: K = K(20);
    pub const STEAM_CRUSHER: K = K(21);
    pub const STEAM_HAMMER: K = K(22);
    pub const STEAM_WIRE_DRAWER: K = K(23);
    pub const STEAM_FURNACE: K = K(24);
    pub const STEAM_ALLOY_SMELTER: K = K(25);
    pub const STEAM_EXTRACTOR: K = K(26);
    pub const STEAM_PRESS: K = K(27);
    pub const STEAM_WASHER: K = K(28);
    pub const STEAM_ASSEMBLER: K = K(29);
    pub const STEAM_PUMP: K = K(30);
    pub const STEAM_DRILL: K = K(31);
    pub const STEAM_LAB: K = K(32);
    pub const IRON_BELT: K = K(33);
    pub const ARM: K = K(34);
    pub const IRON_CRATE: K = K(35);
    pub const BRONZE_PIPE: K = K(36);
    pub const IRON_PIPE: K = K(37);
    pub const TANK: K = K(38);
    pub const MAGNET: K = K(39);
    pub const FAN: K = K(40);
    pub const STEAM_TURBINE: K = K(41);
    pub const COMBUSTION_GENERATOR: K = K(42);
    pub const SOLAR_PANEL: K = K(43);
    pub const BATTERY_BOX: K = K(44);
    pub const ELECTRIC_FURNACE: K = K(45);
    pub const MACERATOR: K = K(46);
    pub const ASSEMBLER: K = K(47);
    pub const LV_LAB: K = K(48);
    pub const ELECTRIC_PUMP: K = K(49);
    pub const WIRE_MILL: K = K(50);
    pub const TIN_CABLE: K = K(51);
    pub const WATER_TURBINE: K = K(52);
}

/// Recipe ids of the mock catalog that tests and the preview use.
pub mod recipe {
    use foundry_core::RecipeId as R;
    pub const BRONZE_GEAR: R = R(1);
    pub const BRONZE_KIT: R = R(2);
    pub const WOOD_BELT: R = R(3);
    pub const RAW_CLAY_BRICK: R = R(4);
    pub const STEAM_KIT: R = R(5);
    pub const CLAY_BRICK: R = R(6);
    pub const CRUSH_MALACHITE: R = R(7);
}

/// A building as an item.
pub fn b(kind: BuildingKindId) -> ItemId {
    ItemId::Building(kind)
}

const WOOD: [u8; 3] = [150, 104, 64];
const BRICK: [u8; 3] = [168, 82, 58];
const CLAY: [u8; 3] = [150, 128, 104];
const TIN: [u8; 3] = [196, 202, 210];
const COPPER: [u8; 3] = [220, 124, 74];
const BRONZE: [u8; 3] = [200, 146, 76];
const STEEL: [u8; 3] = [150, 158, 172];
const IRON: [u8; 3] = [124, 128, 138];
const GLASS: [u8; 3] = [178, 222, 236];

fn c(rgb: [u8; 3]) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255]
}

fn fact(label: &str, value: &str) -> Fact {
    Fact { label: label.into(), value: value.into() }
}

struct PartDef {
    item: ItemId,
    key: &'static str,
    name: &'static str,
    shape: IconShape,
    color: [u8; 3],
    tier: u8,
    stack: u32,
    desc: &'static str,
}

#[rustfmt::skip]
fn part_defs() -> Vec<PartDef> {
    use IconShape::*;
    use part::*;
    let d = |item, key, name, shape, color, tier, stack, desc| PartDef { item, key, name, shape, color, tier, stack, desc };
    vec![
        d(RAW_CLAY_BRICK, "raw_clay_brick", "Raw clay brick", Brick, CLAY, 0, 100, "Unfired clay. Bake it in a kiln at 900 °C to make a clay brick."),
        d(CLAY_BRICK, "clay_brick", "Clay brick", Brick, BRICK, 0, 100, "A fired brick. Kiln walls, crucibles and molds are made of it."),
        d(RAW_FIREBRICK, "raw_firebrick", "Raw firebrick", Brick, [196, 176, 142], 0, 100, "Clay and sand. Bake it at 1200 °C to make a firebrick."),
        d(FIREBRICK, "firebrick", "Firebrick", Brick, [222, 196, 150], 1, 100, "A brick that stays strong up to 1800 °C. Furnace walls."),
        d(TIN_INGOT, "tin_ingot", "Tin ingot", Ingot, TIN, 0, 100, "16 units of tin, cast in an ingot mold."),
        d(COPPER_INGOT, "copper_ingot", "Copper ingot", Ingot, COPPER, 0, 100, "16 units of copper, cast in an ingot mold."),
        d(BRONZE_INGOT, "bronze_ingot", "Bronze ingot", Ingot, BRONZE, 0, 100, "16 units of bronze, cast in an ingot mold."),
        d(TIN_PLATE, "tin_plate", "Tin plate", Plate, TIN, 0, 100, "16 units of tin in a flat plate."),
        d(COPPER_PLATE, "copper_plate", "Copper plate", Plate, COPPER, 0, 100, "16 units of copper in a flat plate."),
        d(BRONZE_PLATE, "bronze_plate", "Bronze plate", Plate, BRONZE, 0, 100, "16 units of bronze in a flat plate. Most Tier 1 machines need it."),
        d(STEEL_PLATE, "steel_plate", "Steel plate", Plate, STEEL, 1, 100, "16 units of steel in a flat plate."),
        d(BRONZE_GEAR, "bronze_gear", "Bronze gear", Gear, BRONZE, 0, 100, "A gear made of 48 units of bronze. Machines need gears."),
        d(STEEL_GEAR, "steel_gear", "Steel gear", Gear, STEEL, 1, 100, "A gear made of 48 units of steel."),
        d(COPPER_ROD, "copper_rod", "Copper rod", Rod, COPPER, 0, 100, "8 units of copper. A wire drawer makes wire from it."),
        d(BRONZE_ROD, "bronze_rod", "Bronze rod", Rod, BRONZE, 0, 100, "8 units of bronze. Frames and axles."),
        d(STEEL_ROD, "steel_rod", "Steel rod", Rod, STEEL, 1, 100, "8 units of steel. Bolts and frames."),
        d(COPPER_WIRE, "copper_wire", "Copper wire", WireCoil, COPPER, 0, 200, "4 units of copper. Cables and circuits."),
        d(TIN_WIRE, "tin_wire", "Tin wire", WireCoil, TIN, 0, 200, "4 units of tin. Bare cables."),
        d(BRONZE_PIPE, "bronze_pipe_section", "Bronze pipe section", Pipe, BRONZE, 0, 100, "48 units of bronze. Boilers and pipes need it."),
        d(IRON_PIPE, "iron_pipe_section", "Iron pipe section", Pipe, IRON, 1, 100, "48 units of iron. Gas-tight pipes."),
        d(STEEL_BOLT, "steel_bolt", "Steel bolt", Bolt, STEEL, 1, 200, "2 units of steel."),
        d(GLASS_PANE, "glass_pane", "Glass pane", Pane, GLASS, 1, 100, "16 units of glass. Labs and windows."),
        d(GLASS_VIAL, "glass_vial", "Glass vial", Vial, GLASS, 1, 100, "8 units of glass. Research kits need vials."),
        d(GLASS_TUBE, "glass_tube", "Glass tube", Vial, [200, 230, 240], 1, 100, "8 units of glass. Vacuum tubes need it."),
        d(RUBBER_SHEET, "rubber_sheet", "Rubber sheet", Sheet, [58, 54, 52], 1, 100, "16 units of rubber. Insulation and belts."),
        d(CIRCUIT_BOARD, "circuit_board", "Circuit board", Circuit, [62, 120, 64], 1, 100, "Wood chips and resin, pressed flat. The base of every circuit."),
        d(VACUUM_TUBE, "vacuum_tube", "Vacuum tube", VacuumTube, COPPER, 1, 100, "A glass tube with copper wire and a steel bolt. Basic circuits need it."),
        d(BRONZE_KIT, "bronze_kit", "Bronze kit", Kit, [226, 150, 60], 0, 200, "A research kit for Tier 0. Labs use it to research."),
        d(STEAM_KIT, "steam_kit", "Steam kit", Kit, [110, 170, 230], 1, 200, "A research kit for Tier 1. Labs use it to research."),
    ]
}

struct BuildingDef {
    kind: BuildingKindId,
    key: &'static str,
    name: &'static str,
    shape: IconShape,
    color: [u8; 3],
    tier: u8,
    size: &'static str,
    desc: &'static str,
    extra: Vec<(&'static str, &'static str)>,
}

#[rustfmt::skip]
fn building_defs() -> Vec<BuildingDef> {
    use IconShape::*;
    use MachineGlyph as G;
    use bld::*;
    let d = |kind, key, name, shape, color, tier, size, desc| BuildingDef { kind, key, name, shape, color, tier, size, desc, extra: vec![] };
    let mut v = vec![
        d(WORKBENCH, "workbench", "Workbench", Workbench, WOOD, 0, "2 × 2", "Hand crafting is 2 times faster near it."),
        d(CAMPFIRE, "campfire", "Campfire", Campfire, WOOD, 0, "1 × 1", "Burns fuel from its small hopper. Heats what is above it."),
        d(WOOD_BLOCK, "wood_block", "Wood block", Block, [158, 112, 70], 0, "1 × 1", "A wall block. It burns at 300 °C."),
        d(LADDER, "ladder", "Ladder", Ladder, WOOD, 0, "1 × 1", "The robot can climb it."),
        d(BRICK_WALL, "clay_brick_wall", "Clay brick wall", Wall, BRICK, 0, "1 × 1", "A wall block for kiln rooms. It holds up to 1200 °C."),
        d(KILN_CONTROLLER, "kiln_controller", "Kiln controller", Machine(G::Flame), BRICK, 0, "1 × 1", "Put it in the wall of a closed clay brick room. The room becomes a kiln."),
        d(KILN_HATCH, "kiln_hatch", "Kiln hatch", Machine(G::Arrow), BRICK, 0, "1 × 1", "An input for a kiln room."),
        d(BELLOWS, "bellows", "Bellows", Machine(G::Fan), WOOD, 0, "1 × 1", "Water turns its wheel. It makes the fire in its room up to 300 °C hotter."),
        d(CRUCIBLE, "crucible", "Crucible", Crucible, BRICK, 0, "1 × 1", "Holds 200 units of molten metal. Takes heat from the cells below it."),
        d(INGOT_MOLD, "ingot_mold", "Ingot mold", Mold, BRICK, 0, "1 × 1", "Molten metal in the mold becomes an ingot when it cools."),
        d(PLATE_MOLD, "plate_mold", "Plate mold", Mold, [150, 90, 70], 0, "1 × 1", "Molten metal in the mold becomes a plate when it cools."),
        d(STAMP_MILL, "stamp_mill", "Stamp mill", Machine(G::Hammer), WOOD, 0, "2 × 3", "A crusher that water drives. Water must flow through its wheel."),
        d(SLUICE, "sluice", "Sluice", Machine(G::Drop), WOOD, 0, "3 × 1", "Water flows through it. Heavy powder stays. Light powder washes out."),
        d(HOPPER, "hopper", "Hopper", Hopper, WOOD, 0, "1 × 1", "Collects powder and releases it at a set rate. It can filter."),
        d(WOOD_BELT, "wood_belt", "Wood belt", Belt, WOOD, 0, "1 × 1", "Moves powder and parts at 8 cells per second. It can burn."),
        d(BARREL, "barrel", "Barrel", Barrel, WOOD, 0, "1 × 1", "Holds 500 units of liquid."),
        d(CRATE, "crate", "Crate", Crate, WOOD, 0, "1 × 1", "Holds 8 stacks of parts."),
        d(BASIC_LAB, "basic_lab", "Basic lab", Machine(G::Flask), WOOD, 0, "2 × 2", "Researches with bronze kits."),
        d(SMALL_BOILER, "small_boiler", "Small boiler", Tank, BRONZE, 1, "2 × 2", "Burns fuel to boil water. Makes 6 steam per second. It explodes if it gets water while it is dry and hot."),
        d(STEAM_CRUSHER, "steam_crusher", "Steam crusher", Machine(G::Crusher), BRONZE, 1, "2 × 2", "Crushes ore. Uses 2 steam per second."),
        d(STEAM_HAMMER, "steam_hammer", "Steam hammer", Machine(G::Hammer), BRONZE, 1, "2 × 2", "Hammers ingots into plates."),
        d(STEAM_WIRE_DRAWER, "steam_wire_drawer", "Steam wire drawer", Machine(G::Wire), BRONZE, 1, "2 × 1", "Draws one rod into two wires."),
        d(STEAM_FURNACE, "steam_furnace", "Steam furnace", Machine(G::Flame), BRONZE, 1, "2 × 2", "Smelts dust and crushed ore up to 1300 °C."),
        d(STEAM_ALLOY_SMELTER, "steam_alloy_smelter", "Steam alloy smelter", Machine(G::Plus), BRONZE, 1, "2 × 2", "Melts two metals together into an alloy."),
        d(STEAM_EXTRACTOR, "steam_extractor", "Steam extractor", Machine(G::Drop), BRONZE, 1, "2 × 2", "Gets resin out of rubber tree wood."),
        d(STEAM_PRESS, "steam_press", "Steam press", Machine(G::Hammer), [184, 132, 70], 1, "2 × 2", "Presses rubber into sheets, and wood chips with resin into circuit boards."),
        d(STEAM_WASHER, "steam_washer", "Steam washer", Machine(G::Drop), [184, 132, 70], 1, "3 × 2", "Washes crushed ore with water. Light waste leaves with dirty water."),
        d(STEAM_ASSEMBLER, "steam_assembler", "Steam assembler", Machine(G::Gear), BRONZE, 1, "3 × 2", "Makes parts from parts. Uses 4 steam per second."),
        d(STEAM_PUMP, "steam_pump", "Steam pump", Machine(G::Arrow), BRONZE, 1, "1 × 2", "Takes fluid from the world into a pipe."),
        d(STEAM_DRILL, "steam_drill", "Steam drill", Machine(G::Drill), BRONZE, 1, "3 × 3", "Mines the cells in front of it and puts them out."),
        d(STEAM_LAB, "steam_lab", "Steam lab", Machine(G::Flask), BRONZE, 1, "2 × 2", "Researches with bronze kits and steam kits."),
        d(IRON_BELT, "iron_belt", "Iron belt", Belt, IRON, 1, "1 × 1", "Moves powder and parts at 16 cells per second."),
        d(ARM, "arm", "Arm", Machine(G::Arrow), IRON, 1, "1 × 1", "Picks up parts in one place and puts them in another place."),
        d(IRON_CRATE, "iron_crate", "Iron crate", Crate, [118, 110, 100], 1, "1 × 1", "Holds 16 stacks of parts."),
        d(BRONZE_PIPE, "bronze_pipe", "Bronze pipe", Pipe, BRONZE, 1, "1 × 1", "A back layer pipe. Up to 600 °C. Not gas-tight."),
        d(IRON_PIPE, "iron_pipe", "Iron pipe", Pipe, IRON, 1, "1 × 1", "A back layer pipe. Up to 800 °C. Gas-tight."),
        d(TANK, "tank", "Tank", Tank, IRON, 1, "2 × 2", "Holds 4000 units of fluid."),
        d(MAGNET, "magnet", "Magnet", Machine(G::Magnet), IRON, 1, "1 × 1", "Pulls magnetic powder to it."),
        d(FAN, "fan", "Fan", Machine(G::Fan), IRON, 1, "1 × 1", "Pushes gas and light powder."),
        d(STEAM_TURBINE, "steam_turbine", "Steam turbine", Machine(G::Fan), STEEL, 2, "3 × 2", "Makes electric power from steam. Up to 20 kW."),
        d(COMBUSTION_GENERATOR, "combustion_generator", "Combustion generator", Machine(G::Flame), STEEL, 2, "2 × 2", "Burns fuel to make electric power. Up to 16 kW."),
        d(SOLAR_PANEL, "solar_panel", "Solar panel", SolarPanel, [44, 76, 150], 2, "2 × 1", "Makes up to 2 kW in daylight. Smoke and smog make it weaker."),
        d(BATTERY_BOX, "battery_box", "Battery box", Battery, [78, 80, 88], 2, "1 × 1", "Stores 2 MJ of electric energy."),
        d(ELECTRIC_FURNACE, "electric_furnace", "Electric furnace", Machine(G::Flame), STEEL, 2, "2 × 2", "Smelts with electric heat up to 1600 °C. Uses 9 kW."),
        d(MACERATOR, "macerator", "Macerator", Machine(G::Crusher), STEEL, 2, "2 × 2", "Grinds ore into crushed ore and dust. Uses 6 kW."),
        d(ASSEMBLER, "assembler", "Assembler", Machine(G::Gear), STEEL, 2, "3 × 2", "Makes parts from parts. Uses 3.5 kW."),
        d(LV_LAB, "lv_lab", "LV lab", Machine(G::Flask), STEEL, 2, "2 × 2", "Researches with kits up to the electric kit."),
        d(ELECTRIC_PUMP, "electric_pump", "Electric pump", Machine(G::Arrow), STEEL, 2, "1 × 2", "Takes fluid from the world into a pipe. Uses 3 kW."),
        d(WIRE_MILL, "wire_mill", "Wire mill", Machine(G::Wire), STEEL, 2, "2 × 1", "Draws rods into wires. Uses 4 kW."),
        d(TIN_CABLE, "tin_cable", "Tin cable", Cable, TIN, 2, "1 × 1", "A bare LV cable. Limit: 32 V and 32 A. Water makes it short-circuit."),
        d(WATER_TURBINE, "water_turbine", "Water turbine", Machine(G::Drop), STEEL, 2, "2 × 2", "Makes electric power from flowing water. Up to 4 kW."),
    ];
    for def in v.iter_mut() {
        match def.kind {
            SMALL_BOILER => def.extra = vec![("Fuel", "1 coal every 4 s"), ("Water", "6 units per second")],
            STEAM_CRUSHER => def.extra = vec![("Steam use", "2 units per second")],
            STEAM_ASSEMBLER => def.extra = vec![("Steam use", "4 units per second"), ("Max temperature", "400 °C")],
            ELECTRIC_FURNACE => def.extra = vec![("Power", "9 kW (LV)"), ("Max temperature", "1600 °C")],
            _ => {}
        }
    }
    v
}

fn material_info(content: &Content, id: MaterialId) -> ItemInfo {
    let t = &content.materials;
    let i = id.index();
    let phase = t.phase[i];
    let (kind, shape, desc) = match phase {
        Phase::Powder => (ItemKind::Powder, IconShape::Pile, "A powder. It falls and makes piles."),
        Phase::Liquid => (ItemKind::Liquid, IconShape::Drop, "A liquid. It flows and finds its level."),
        Phase::Gas => (ItemKind::Gas, IconShape::Gas, "A gas. It rises or sinks by its density and spreads out."),
        Phase::Fire => (ItemKind::Fire, IconShape::Flame, "Fire. It rises, heats and ignites what it touches."),
        Phase::Solid | Phase::Empty => (ItemKind::Solid, IconShape::Block, "A solid material. It does not move."),
    };
    let mut facts = vec![];
    if t.density[i] > 0.0 {
        facts.push(fact("Density", &format!("{} kg/m³", t.density[i].round())));
    }
    if let Some(m) = t.melt[i] {
        facts.push(fact("Melts at", &format!("{} °C", m.at)));
    }
    if let Some(m) = t.freeze[i] {
        facts.push(fact("Freezes below", &format!("{} °C", m.at)));
    }
    if let Some(m) = t.boil[i] {
        facts.push(fact("Boils at", &format!("{} °C", m.at)));
    }
    if let Some(b) = t.burn[i] {
        facts.push(fact("Burns at", &format!("{} °C", b.ignite_at)));
    }
    let colors: Vec<[u8; 4]> = t.colors[i].iter().map(|c| [c[0], c[1], c[2], 255]).collect();
    ItemInfo {
        id: ItemId::Material(id),
        key: t.ids[i].clone(),
        name: t.names[i].clone(),
        description: desc.into(),
        kind,
        icon: IconSpec { shape, colors, tier: 0 },
        stack_size: 2000,
        tier: 0,
        facts,
    }
}

/// The mock catalog: all materials of the content, and the Tier 0-2 parts, buildings and recipes.
pub fn catalog(content: &Content) -> Catalog {
    let mut cat = Catalog::new();
    cat.revision = 1;
    for id in content.materials.all().skip(1) {
        cat.add_item(material_info(content, id));
    }
    for d in part_defs() {
        cat.add_item(ItemInfo {
            id: d.item,
            key: d.key.into(),
            name: d.name.into(),
            description: d.desc.into(),
            kind: ItemKind::Part,
            icon: IconSpec::new(d.shape, c(d.color)).with_tier(d.tier),
            stack_size: d.stack,
            tier: d.tier,
            facts: vec![],
        });
    }
    for d in building_defs() {
        let mut facts = vec![fact("Size", &format!("{} tiles", d.size)), fact("Tier", &d.tier.to_string())];
        facts.extend(d.extra.iter().map(|(l, v)| fact(l, v)));
        cat.add_item(ItemInfo {
            id: b(d.kind),
            key: d.key.into(),
            name: d.name.into(),
            description: d.desc.into(),
            kind: ItemKind::Building,
            icon: IconSpec::new(d.shape, c(d.color)).with_tier(d.tier),
            stack_size: 50,
            tier: d.tier,
            facts,
        });
    }
    add_recipes(&mut cat, content);
    cat
}

fn add_recipes(cat: &mut Catalog, content: &Content) {
    use CraftGroup::*;
    use Maker::Hand;
    use bld::*;
    use part::*;
    let m = |s: &str| ItemId::Material(content.expect_material(s));
    let mut next = 100u16;
    let mut add = |id: Option<RecipeId>, name: &str, group: CraftGroup, row: u8, ings: &[(ItemId, u32)], res: &[(ItemId, u32)], time: f32, made_in: &[Maker], unlocked: bool| {
        let id = id.unwrap_or_else(|| {
            next += 1;
            RecipeId(next)
        });
        cat.add_recipe(RecipeView {
            id,
            name: name.into(),
            group,
            row,
            ingredients: ings.iter().map(|&(i, a)| ItemAmount::new(i, a)).collect(),
            results: res.iter().map(|&(i, a)| ItemAmount::new(i, a)).collect(),
            time,
            made_in: made_in.to_vec(),
            unlocked,
            min_temperature: None,
        });
    };
    let bm = Maker::Building;
    let hand_bench = [Hand, bm(WORKBENCH)];
    let hand_asm = [Hand, bm(WORKBENCH), bm(STEAM_ASSEMBLER)];
    let wood = m("wood");

    // Logistics
    add(Some(recipe::WOOD_BELT), "Wood belt", Logistics, 0, &[(wood, 4)], &[(b(WOOD_BELT), 1)], 0.5, &hand_bench, true);
    add(None, "Hopper", Logistics, 0, &[(wood, 6)], &[(b(HOPPER), 1)], 1.0, &hand_bench, true);
    add(None, "Crate", Logistics, 1, &[(wood, 8)], &[(b(CRATE), 1)], 1.0, &hand_bench, true);
    add(None, "Barrel", Logistics, 1, &[(wood, 8)], &[(b(BARREL), 1)], 1.0, &hand_bench, true);
    add(None, "Iron belt", Logistics, 0, &[(STEEL_PLATE, 1), (BRONZE_GEAR, 1)], &[(b(IRON_BELT), 2)], 1.0, &hand_asm, true);
    add(None, "Iron crate", Logistics, 1, &[(STEEL_PLATE, 8)], &[(b(IRON_CRATE), 1)], 1.0, &hand_asm, true);
    add(None, "Arm", Logistics, 2, &[(STEEL_PLATE, 2), (BRONZE_GEAR, 2), (COPPER_WIRE, 4)], &[(b(ARM), 1)], 2.0, &hand_asm, true);
    add(None, "Magnet", Logistics, 2, &[(STEEL_PLATE, 2), (COPPER_WIRE, 10)], &[(b(MAGNET), 1)], 2.0, &hand_asm, true);
    add(None, "Fan", Logistics, 2, &[(STEEL_PLATE, 2), (BRONZE_GEAR, 1)], &[(b(FAN), 1)], 1.5, &hand_asm, true);
    add(None, "Bronze pipe", Logistics, 3, &[(BRONZE_PIPE, 1)], &[(b(bld::BRONZE_PIPE), 1)], 0.5, &hand_bench, true);
    add(None, "Iron pipe", Logistics, 3, &[(part::IRON_PIPE, 1)], &[(b(bld::IRON_PIPE), 1)], 0.5, &hand_bench, true);
    add(None, "Tank", Logistics, 3, &[(STEEL_PLATE, 12), (part::IRON_PIPE, 2)], &[(b(TANK), 1)], 3.0, &hand_asm, true);
    add(None, "Ladder", Logistics, 4, &[(wood, 2)], &[(b(LADDER), 1)], 0.5, &hand_bench, true);
    add(None, "Wood block", Logistics, 4, &[(wood, 8)], &[(b(WOOD_BLOCK), 1)], 0.5, &hand_bench, true);
    add(None, "Clay brick wall", Logistics, 4, &[(CLAY_BRICK, 2)], &[(b(BRICK_WALL), 1)], 0.5, &hand_bench, true);

    // Production
    add(None, "Workbench", Production, 0, &[(wood, 20)], &[(b(WORKBENCH), 1)], 2.0, &[Hand], true);
    add(None, "Kiln controller", Production, 1, &[(CLAY_BRICK, 8), (wood, 2)], &[(b(KILN_CONTROLLER), 1)], 2.0, &hand_bench, true);
    add(None, "Kiln hatch", Production, 1, &[(CLAY_BRICK, 4), (wood, 2)], &[(b(KILN_HATCH), 1)], 1.0, &hand_bench, true);
    add(None, "Crucible", Production, 1, &[(CLAY_BRICK, 4)], &[(b(CRUCIBLE), 1)], 1.0, &hand_bench, true);
    add(None, "Ingot mold", Production, 1, &[(CLAY_BRICK, 4)], &[(b(INGOT_MOLD), 1)], 1.0, &hand_bench, true);
    add(None, "Plate mold", Production, 1, &[(CLAY_BRICK, 4)], &[(b(PLATE_MOLD), 1)], 1.0, &hand_bench, true);
    add(None, "Stamp mill", Production, 2, &[(wood, 30), (BRONZE_GEAR, 4)], &[(b(STAMP_MILL), 1)], 4.0, &hand_bench, true);
    add(None, "Sluice", Production, 2, &[(wood, 20)], &[(b(SLUICE), 1)], 2.0, &hand_bench, true);
    add(None, "Steam crusher", Production, 3, &[(BRONZE_PLATE, 8), (BRONZE_GEAR, 4), (BRONZE_PIPE, 2)], &[(b(STEAM_CRUSHER), 1)], 5.0, &hand_asm, true);
    add(None, "Steam hammer", Production, 3, &[(BRONZE_PLATE, 8), (BRONZE_GEAR, 2), (BRONZE_PIPE, 2)], &[(b(STEAM_HAMMER), 1)], 5.0, &hand_asm, true);
    add(None, "Steam furnace", Production, 3, &[(BRONZE_PLATE, 6), (CLAY_BRICK, 12), (BRONZE_PIPE, 2)], &[(b(STEAM_FURNACE), 1)], 5.0, &hand_asm, true);
    add(None, "Steam wire drawer", Production, 3, &[(BRONZE_PLATE, 6), (BRONZE_GEAR, 2), (BRONZE_PIPE, 1)], &[(b(STEAM_WIRE_DRAWER), 1)], 5.0, &hand_asm, true);
    add(None, "Steam alloy smelter", Production, 4, &[(BRONZE_PLATE, 8), (FIREBRICK, 8), (BRONZE_PIPE, 2)], &[(b(STEAM_ALLOY_SMELTER), 1)], 6.0, &hand_asm, true);
    add(None, "Steam washer", Production, 4, &[(BRONZE_PLATE, 10), (BRONZE_GEAR, 2), (BRONZE_PIPE, 4)], &[(b(STEAM_WASHER), 1)], 6.0, &hand_asm, true);
    add(None, "Steam extractor", Production, 4, &[(BRONZE_PLATE, 6), (BRONZE_PIPE, 2)], &[(b(STEAM_EXTRACTOR), 1)], 5.0, &hand_asm, true);
    add(None, "Steam press", Production, 4, &[(BRONZE_PLATE, 8), (BRONZE_GEAR, 4)], &[(b(STEAM_PRESS), 1)], 5.0, &hand_asm, true);
    add(None, "Steam assembler", Production, 4, &[(BRONZE_PLATE, 8), (BRONZE_GEAR, 6), (STEEL_PLATE, 4)], &[(b(STEAM_ASSEMBLER), 1)], 6.0, &hand_asm, true);
    add(None, "Steam pump", Production, 5, &[(BRONZE_PLATE, 4), (BRONZE_PIPE, 2), (BRONZE_GEAR, 2)], &[(b(STEAM_PUMP), 1)], 3.0, &hand_asm, true);
    add(None, "Steam drill", Production, 5, &[(BRONZE_PLATE, 12), (STEEL_PLATE, 8), (STEEL_GEAR, 4)], &[(b(STEAM_DRILL), 1)], 8.0, &hand_asm, true);
    add(None, "Assembler", Production, 6, &[(STEEL_PLATE, 8), (CIRCUIT_BOARD, 2), (STEEL_GEAR, 4)], &[(b(ASSEMBLER), 1)], 6.0, &hand_asm, false);
    add(None, "Electric furnace", Production, 6, &[(STEEL_PLATE, 10), (FIREBRICK, 10), (COPPER_WIRE, 16)], &[(b(ELECTRIC_FURNACE), 1)], 6.0, &hand_asm, false);
    add(None, "Macerator", Production, 6, &[(STEEL_PLATE, 10), (STEEL_GEAR, 4)], &[(b(MACERATOR), 1)], 6.0, &hand_asm, false);

    // Intermediate products
    add(Some(recipe::RAW_CLAY_BRICK), "Raw clay brick", Intermediate, 0, &[(m("clay"), 16)], &[(RAW_CLAY_BRICK, 1)], 1.0, &hand_bench, true);
    add(None, "Raw firebrick", Intermediate, 0, &[(m("clay"), 12), (m("sand"), 4)], &[(RAW_FIREBRICK, 1)], 1.0, &hand_bench, true);
    add(Some(recipe::CLAY_BRICK), "Clay brick", Intermediate, 0, &[(RAW_CLAY_BRICK, 1)], &[(CLAY_BRICK, 1)], 20.0, &[bm(KILN_CONTROLLER)], true);
    add(None, "Firebrick", Intermediate, 0, &[(RAW_FIREBRICK, 1)], &[(FIREBRICK, 1)], 30.0, &[bm(KILN_CONTROLLER)], true);
    add(None, "Bronze plate (hammer)", Intermediate, 1, &[(BRONZE_INGOT, 1)], &[(BRONZE_PLATE, 1)], 2.0, &[bm(STEAM_HAMMER)], true);
    add(None, "Tin plate (hammer)", Intermediate, 1, &[(TIN_INGOT, 1)], &[(TIN_PLATE, 1)], 1.5, &[bm(STEAM_HAMMER)], true);
    add(Some(recipe::BRONZE_GEAR), "Bronze gear", Intermediate, 2, &[(BRONZE_PLATE, 3)], &[(BRONZE_GEAR, 1)], 2.0, &hand_asm, true);
    add(None, "Steel gear", Intermediate, 2, &[(STEEL_PLATE, 3)], &[(STEEL_GEAR, 1)], 2.0, &hand_asm, true);
    add(None, "Bronze rod", Intermediate, 2, &[(BRONZE_PLATE, 1)], &[(BRONZE_ROD, 2)], 1.0, &hand_asm, true);
    add(None, "Copper rod", Intermediate, 2, &[(COPPER_PLATE, 1)], &[(COPPER_ROD, 2)], 1.0, &hand_asm, true);
    add(None, "Steel rod", Intermediate, 2, &[(STEEL_PLATE, 1)], &[(STEEL_ROD, 2)], 1.0, &hand_asm, true);
    add(None, "Copper wire", Intermediate, 3, &[(COPPER_ROD, 1)], &[(COPPER_WIRE, 2)], 1.0, &[Hand, bm(STEAM_WIRE_DRAWER)], true);
    add(None, "Tin wire", Intermediate, 3, &[(TIN_PLATE, 1)], &[(TIN_WIRE, 4)], 1.0, &[Hand, bm(STEAM_WIRE_DRAWER)], true);
    add(None, "Bronze pipe section", Intermediate, 3, &[(BRONZE_PLATE, 3)], &[(BRONZE_PIPE, 1)], 2.0, &hand_asm, true);
    add(None, "Iron pipe section", Intermediate, 3, &[(STEEL_PLATE, 3)], &[(part::IRON_PIPE, 1)], 2.0, &hand_asm, true);
    add(None, "Steel bolt", Intermediate, 3, &[(STEEL_ROD, 1)], &[(STEEL_BOLT, 4)], 1.0, &hand_asm, true);
    add(None, "Circuit board", Intermediate, 4, &[(m("wood_chips"), 16), (m("resin"), 8)], &[(CIRCUIT_BOARD, 1)], 4.0, &[bm(STEAM_PRESS)], true);
    add(None, "Rubber sheet", Intermediate, 4, &[(m("rubber"), 16)], &[(RUBBER_SHEET, 1)], 3.0, &[bm(STEAM_PRESS)], true);
    add(None, "Vacuum tube", Intermediate, 4, &[(GLASS_TUBE, 1), (COPPER_WIRE, 2), (STEEL_BOLT, 1)], &[(VACUUM_TUBE, 1)], 3.0, &[Hand, bm(STEAM_ASSEMBLER)], true);
    add(Some(recipe::CRUSH_MALACHITE), "Crushed malachite", Intermediate, 5, &[(m("raw_malachite"), 16)], &[(m("crushed_malachite"), 16)], 4.0, &[bm(STAMP_MILL), bm(STEAM_CRUSHER)], true);
    add(None, "Crushed cassiterite", Intermediate, 5, &[(m("raw_cassiterite"), 16)], &[(m("crushed_cassiterite"), 16)], 4.0, &[bm(STAMP_MILL), bm(STEAM_CRUSHER)], true);
    add(None, "Crushed magnetite", Intermediate, 5, &[(m("raw_magnetite"), 16)], &[(m("crushed_magnetite"), 16)], 4.0, &[bm(STEAM_CRUSHER)], true);
    add(None, "Crushed limestone", Intermediate, 5, &[(m("raw_limestone"), 16)], &[(m("crushed_limestone"), 16)], 4.0, &[bm(STEAM_CRUSHER)], true);

    // Power
    add(None, "Campfire", Power, 0, &[(wood, 10)], &[(b(CAMPFIRE), 1)], 1.0, &hand_bench, true);
    add(None, "Bellows", Power, 0, &[(wood, 10), (TIN_PLATE, 2)], &[(b(BELLOWS), 1)], 2.0, &hand_bench, true);
    add(None, "Small boiler", Power, 1, &[(BRONZE_PLATE, 10), (CLAY_BRICK, 8), (BRONZE_PIPE, 2)], &[(b(SMALL_BOILER), 1)], 5.0, &hand_asm, true);
    add(None, "Steam turbine", Power, 2, &[(STEEL_PLATE, 12), (STEEL_GEAR, 8), (COPPER_WIRE, 20)], &[(b(STEAM_TURBINE), 1)], 8.0, &hand_asm, false);
    add(None, "Solar panel", Power, 2, &[(GLASS_PANE, 4), (COPPER_WIRE, 8)], &[(b(SOLAR_PANEL), 1)], 6.0, &hand_asm, false);

    // Research
    add(None, "Basic lab", Research, 0, &[(wood, 20), (COPPER_PLATE, 4), (CLAY_BRICK, 4)], &[(b(BASIC_LAB), 1)], 3.0, &hand_bench, true);
    add(None, "Steam lab", Research, 0, &[(BRONZE_PLATE, 10), (GLASS_PANE, 4), (BRONZE_PIPE, 2)], &[(b(STEAM_LAB), 1)], 5.0, &hand_asm, true);
    add(Some(recipe::BRONZE_KIT), "Bronze kit", Research, 1, &[(BRONZE_GEAR, 1), (CLAY_BRICK, 1), (TIN_PLATE, 1)], &[(BRONZE_KIT, 1)], 5.0, &[Hand, bm(WORKBENCH), bm(STEAM_ASSEMBLER)], true);
    add(Some(recipe::STEAM_KIT), "Steam kit", Research, 1, &[(STEEL_PLATE, 1), (BRONZE_PIPE, 1), (GLASS_VIAL, 1)], &[(STEAM_KIT, 1)], 8.0, &[bm(STEAM_ASSEMBLER)], true);
}

fn stack(item: ItemId, n: u32) -> Option<ItemStack> {
    Some(ItemStack::new(item, n))
}

/// A smooth, repeatable wave with some noise, for graph test data.
fn wave(i: usize, seed: u32, base: f32, amp: f32, period: f32) -> f32 {
    let x = i as f32;
    let noise = ((i as u32).wrapping_mul(2_654_435_761).wrapping_add(seed.wrapping_mul(40_503)) >> 16) as f32 / 65_536.0;
    (base + amp * (x * std::f32::consts::TAU / period + seed as f32).sin() + amp * 0.35 * (noise - 0.5)).max(0.0)
}

/// Test time series around `base` for all ranges.
pub fn series(seed: u32, base: f32, amp: f32) -> TimeSeries {
    let mut s = TimeSeries::default();
    for (r, list) in s.ranges.iter_mut().enumerate() {
        let period = [90.0, 140.0, 70.0, 110.0, 60.0][r];
        list.extend((0..SAMPLES).map(|i| wave(i, seed + r as u32 * 17, base, amp, period)));
    }
    s
}

fn sum_series(parts: &[&TimeSeries]) -> TimeSeries {
    let mut s = TimeSeries::default();
    for r in 0..5 {
        let n = parts.iter().map(|p| p.ranges[r].len()).max().unwrap_or(0);
        s.ranges[r] = (0..n).map(|i| parts.iter().map(|p| p.ranges[r].get(i).copied().unwrap_or(0.0)).sum()).collect();
    }
    s
}

/// A steam assembler that makes bronze kits.
pub fn steam_assembler_view() -> BuildingView {
    use part::*;
    BuildingView {
        id: BuildingId { index: 17, generation: 1 },
        item: b(bld::STEAM_ASSEMBLER),
        status: MachineStatus::Working,
        status_detail: String::new(),
        recipe: Some(recipe::BRONZE_KIT),
        recipes: vec![recipe::BRONZE_KIT, recipe::STEAM_KIT, recipe::BRONZE_GEAR, RecipeId(137), RecipeId(121), RecipeId(122), RecipeId(133), RecipeId(135)],
        inputs: vec![
            BuildingSlot { stack: stack(BRONZE_GEAR, 3), filter: Some(BRONZE_GEAR) },
            BuildingSlot { stack: stack(CLAY_BRICK, 12), filter: Some(CLAY_BRICK) },
            BuildingSlot { stack: None, filter: Some(TIN_PLATE) },
        ],
        outputs: vec![BuildingSlot { stack: stack(BRONZE_KIT, 7), filter: None }],
        fuel: vec![],
        buffers: vec![],
        progress: 0.45,
        speed: 1.0,
        power: None,
        temperature: Some(96.0),
        max_temperature: Some(400.0),
    }
}

/// A small boiler with no fuel.
pub fn boiler_view(content: &Content) -> BuildingView {
    BuildingView {
        id: BuildingId { index: 4, generation: 2 },
        item: b(bld::SMALL_BOILER),
        status: MachineStatus::NoFuel,
        status_detail: "Put coal or charcoal in the fuel slot.".into(),
        recipe: None,
        recipes: vec![],
        inputs: vec![],
        outputs: vec![],
        fuel: vec![BuildingSlot { stack: None, filter: Some(ItemId::Material(content.expect_material("charcoal"))) }],
        buffers: vec![
            MaterialBuffer { label: "Water in".into(), material: Some(content.expect_material("water")), units: 180, capacity: 400, output: false },
            MaterialBuffer { label: "Steam out".into(), material: Some(content.expect_material("steam")), units: 12, capacity: 400, output: true },
        ],
        progress: 0.0,
        speed: 1.0,
        power: None,
        temperature: Some(212.0),
        max_temperature: Some(600.0),
    }
}

/// An electric furnace on a network that has too little power.
pub fn electric_furnace_view(content: &Content) -> BuildingView {
    BuildingView {
        id: BuildingId { index: 31, generation: 1 },
        item: b(bld::ELECTRIC_FURNACE),
        status: MachineStatus::LowPower,
        status_detail: "The network gives 86%.".into(),
        recipe: Some(RecipeId(9001)),
        recipes: vec![RecipeId(9001)],
        inputs: vec![BuildingSlot { stack: None, filter: None }],
        outputs: vec![],
        fuel: vec![],
        buffers: vec![
            MaterialBuffer { label: "Ore in".into(), material: Some(content.expect_material("crushed_cassiterite")), units: 96, capacity: 256, output: false },
            MaterialBuffer { label: "Metal out".into(), material: Some(content.expect_material("molten_tin")), units: 140, capacity: 200, output: true },
        ],
        progress: 0.7,
        speed: 1.0,
        power: Some(PowerUse { use_w: 7740.0, max_w: 9000.0, voltage: Voltage::Lv, network_voltage: Some(Voltage::Lv), satisfaction: 0.86 }),
        temperature: Some(1140.0),
        max_temperature: Some(1600.0),
    }
}

/// An LV network with some load problems.
pub fn power_view() -> PowerNetworkView {
    let e = |kind: BuildingKindId, count: u32, watts: f64, seed: u32| PowerEntry {
        item: b(kind),
        count,
        watts,
        history: series(seed, watts as f32, watts as f32 * 0.18),
    };
    let producers = vec![
        e(bld::STEAM_TURBINE, 2, 40_000.0, 1),
        e(bld::SOLAR_PANEL, 6, 9_600.0, 2),
        e(bld::BATTERY_BOX, 2, 6_400.0, 3),
        e(bld::WATER_TURBINE, 1, 3_200.0, 4),
    ];
    let consumers = vec![
        e(bld::ELECTRIC_FURNACE, 3, 23_200.0, 5),
        e(bld::MACERATOR, 2, 11_400.0, 6),
        e(bld::ASSEMBLER, 4, 12_600.0, 7),
        e(bld::WIRE_MILL, 2, 6_800.0, 8),
        e(bld::LV_LAB, 1, 3_900.0, 9),
        e(bld::ELECTRIC_PUMP, 1, 2_300.0, 10),
    ];
    let prod_refs: Vec<&TimeSeries> = producers.iter().map(|p| &p.history).collect();
    let cons_refs: Vec<&TimeSeries> = consumers.iter().map(|p| &p.history).collect();
    let production_history = sum_series(&prod_refs);
    let consumption_history = sum_series(&cons_refs);
    PowerNetworkView {
        id: 3,
        voltage: Voltage::Lv,
        satisfaction: 0.86,
        production_w: producers.iter().map(|p| p.watts).sum(),
        capacity_w: 62_000.0,
        consumption_w: consumers.iter().map(|p| p.watts).sum(),
        stored_j: 1_200_000.0,
        storage_capacity_j: 4_000_000.0,
        amps: 36.0,
        limit_amps: 32.0,
        producers,
        consumers,
        production_history,
        consumption_history,
        warnings: vec![
            PowerWarning::CableOverloaded { amps: 36.0, limit_amps: 32.0, cable: "tin cable".into() },
            PowerWarning::NotEnoughPower,
        ],
    }
}

fn stats_view(content: &Content) -> ProductionStatsView {
    let m = |s: &str| ItemId::Material(content.expect_material(s));
    let row = |item: ItemId, made: f32, used: f32, seed: u32| ProductionRow {
        item,
        made: if made > 0.0 { series(seed, made, made * 0.3) } else { TimeSeries::default() },
        used: if used > 0.0 { series(seed + 50, used, used * 0.3) } else { TimeSeries::default() },
    };
    ProductionStatsView {
        rows: vec![
            row(m("raw_malachite"), 960.0, 900.0, 1),
            row(m("crushed_malachite"), 880.0, 850.0, 2),
            row(m("clay"), 640.0, 480.0, 3),
            row(m("charcoal"), 420.0, 510.0, 4),
            row(m("molten_copper"), 540.0, 530.0, 5),
            row(m("molten_bronze"), 300.0, 290.0, 6),
            row(part::BRONZE_PLATE, 36.0, 30.0, 7),
            row(part::BRONZE_GEAR, 12.0, 11.0, 8),
            row(part::CLAY_BRICK, 24.0, 18.0, 9),
            row(part::BRONZE_KIT, 6.0, 5.5, 10),
            row(m("steam"), 360.0, 350.0, 11),
            row(m("water"), 380.0, 360.0, 12),
            row(b(bld::WOOD_BELT), 4.0, 3.0, 13),
        ],
    }
}

/// A realistic model: the player is in Tier 1, with a full inventory, a crafting queue, an open
/// steam assembler, alerts and research.
pub fn model(content: &Content) -> UiModel {
    use part::*;
    let m = |s: &str| content.expect_material(s);
    let catalog = catalog(content);
    let mut inventory = vec![
        stack(BRONZE_PLATE, 58),
        stack(BRONZE_GEAR, 17),
        stack(TIN_PLATE, 42),
        stack(COPPER_PLATE, 12),
        stack(CLAY_BRICK, 86),
        stack(RAW_CLAY_BRICK, 24),
        stack(BRONZE_ROD, 30),
        stack(COPPER_WIRE, 64),
        stack(BRONZE_PIPE, 6),
        stack(STEEL_PLATE, 8),
        stack(b(bld::WOOD_BELT), 50),
        stack(b(bld::WOOD_BELT), 48),
        stack(b(bld::HOPPER), 7),
        stack(b(bld::CRATE), 4),
        stack(b(bld::BARREL), 3),
        stack(b(bld::CRUCIBLE), 2),
        stack(b(bld::INGOT_MOLD), 3),
        stack(b(bld::PLATE_MOLD), 2),
        stack(b(bld::BRICK_WALL), 50),
        stack(b(bld::BRICK_WALL), 14),
        stack(b(bld::LADDER), 20),
        stack(b(bld::STEAM_CRUSHER), 2),
        stack(b(bld::SMALL_BOILER), 1),
        stack(b(bld::STEAM_ASSEMBLER), 1),
        stack(b(bld::BRONZE_PIPE), 36),
        stack(BRONZE_KIT, 12),
        stack(GLASS_VIAL, 9),
        stack(CIRCUIT_BOARD, 3),
        stack(b(bld::WORKBENCH), 1),
        stack(b(bld::KILN_CONTROLLER), 1),
    ];
    inventory.resize(60, None);
    let tank = vec![
        TankSlot { material: Some(m("clay")), units: 1240, capacity: 2000 },
        TankSlot { material: Some(m("raw_malachite")), units: 860, capacity: 2000 },
        TankSlot { material: Some(m("charcoal")), units: 1999, capacity: 2000 },
        TankSlot { material: Some(m("wood")), units: 420, capacity: 2000 },
    ];
    let mut hotbar = vec![
        Some(b(bld::WOOD_BELT)),
        Some(b(bld::HOPPER)),
        Some(b(bld::CRATE)),
        Some(b(bld::BRONZE_PIPE)),
        Some(b(bld::STEAM_CRUSHER)),
        Some(b(bld::SMALL_BOILER)),
        Some(b(bld::STEAM_ASSEMBLER)),
        Some(b(bld::BRICK_WALL)),
        Some(b(bld::LADDER)),
        None,
    ];
    hotbar.extend([Some(b(bld::CRUCIBLE)), Some(b(bld::INGOT_MOLD)), Some(b(bld::PLATE_MOLD)), Some(b(bld::KILN_CONTROLLER)), Some(b(bld::WORKBENCH)), Some(b(bld::STEAM_DRILL)), None, None, None, None]);
    UiModel {
        state: GameState::Playing,
        player: PlayerView {
            hull: 82.0,
            hull_max: 100.0,
            temperature: 41.0,
            heat_limit: 80.0,
            inventory,
            tank,
            hand: None,
            hotbar,
            selected_hotbar: Some(0),
            crafting: vec![
                CraftJobView { recipe: recipe::BRONZE_GEAR, count: 4, progress: 0.35 },
                CraftJobView { recipe: recipe::WOOD_BELT, count: 10, progress: 0.0 },
                CraftJobView { recipe: recipe::BRONZE_KIT, count: 2, progress: 0.0 },
            ],
            craft_speed: 2.0,
        },
        hover: Some(HoverView::Cell { pos: CellPos { x: 4133, y: 612 }, material: m("raw_malachite"), temperature: 24.0 }),
        research: Some(ResearchView { name: "Steam power".into(), icon: b(bld::SMALL_BOILER), progress: 0.62, kits: vec![ItemStack::new(BRONZE_KIT, 1)] }),
        alerts: vec![
            AlertView { id: 1, kind: AlertKind::MachineStopped, text: "Steam crusher stopped: output full".into(), count: 2, pos: Some(CellPos { x: 4200, y: 640 }) },
            AlertView { id: 2, kind: AlertKind::Fire, text: "Fire next to the kiln".into(), count: 1, pos: Some(CellPos { x: 4050, y: 590 }) },
            AlertView { id: 3, kind: AlertKind::Leak, text: "Steam leak: bronze pipe".into(), count: 3, pos: Some(CellPos { x: 4310, y: 700 }) },
        ],
        building: None,
        power: None,
        stats: stats_view(content),
        saves: vec![
            SaveInfo { id: "autosave-1".into(), name: "Autosave".into(), date: "2026-09-27 16:40".into(), play_time_s: 4 * 3600 + 23 * 60, world: "Seed 81234, 8192 × 8192".into() },
            SaveInfo { id: "copper-valley".into(), name: "Copper valley".into(), date: "2026-09-27 15:12".into(), play_time_s: 4 * 3600 + 2 * 60, world: "Seed 81234, 8192 × 8192".into() },
            SaveInfo { id: "first-kiln".into(), name: "First kiln".into(), date: "2026-09-26 21:05".into(), play_time_s: 58 * 60, world: "Seed 81234, 8192 × 8192".into() },
            SaveInfo { id: "desert-test".into(), name: "Desert test".into(), date: "2026-09-25 18:30".into(), play_time_s: 17 * 60, world: "Seed 5, 4096 × 4096".into() },
        ],
        settings: Settings { show_fps: true, ..Default::default() },
        fps: 120.0,
        message: String::new(),
        catalog,
    }
}

/// A hover view of a building (for the entity info panel).
pub fn hover_building() -> HoverView {
    HoverView::Building {
        id: BuildingId { index: 17, generation: 1 },
        item: b(bld::STEAM_CRUSHER),
        status: MachineStatus::OutputFull,
        recipe: Some(recipe::CRUSH_MALACHITE),
        progress: 1.0,
        temperature: Some((64.0, 400.0)),
        power_w: None,
    }
}

/// Applies `UiAction`s to a model, the way the game will. For the preview and for tests.
pub struct MockGame {
    pub model: UiModel,
    /// Set by `UiAction::QuitGame`.
    pub quit: bool,
    content: Content,
    save_counter: u32,
}

impl MockGame {
    pub fn new(content: Content) -> Self {
        let model = model(&content);
        Self { model, quit: false, content, save_counter: 0 }
    }

    pub fn content(&self) -> &Content {
        &self.content
    }

    /// Open a building window (the game does this when the player clicks a building).
    pub fn open_building(&mut self, view: BuildingView) {
        self.model.building = Some(view);
    }

    pub fn open_power(&mut self) {
        self.model.power = Some(power_view());
    }

    pub fn apply_all(&mut self, actions: Vec<UiAction>) {
        for a in actions {
            self.apply(a);
        }
    }

    pub fn apply(&mut self, action: UiAction) {
        let md = &mut self.model;
        match action {
            UiAction::OpenWindow(_) => {}
            UiAction::CloseWindow(WindowKind::Building) => md.building = None,
            UiAction::CloseWindow(WindowKind::PowerNetwork) => md.power = None,
            UiAction::CloseWindow(_) => {}
            UiAction::OpenPowerNetwork(_) => md.power = Some(power_view()),
            UiAction::ClickSlot { slot, click } => self.click_slot(slot, click),
            UiAction::SelectHotbar(i) => {
                if md.player.hotbar.get(i).copied().flatten().is_some() {
                    md.player.selected_hotbar = Some(i);
                }
            }
            UiAction::SetHotbar { index, item } => {
                if index < md.player.hotbar.len() {
                    md.player.hotbar[index] = item;
                }
            }
            UiAction::ClearHand => {
                if let Some(h) = md.player.hand.take() {
                    let limit = |_: usize, item: ItemId| if item.is_bulk() { 0 } else { 100 };
                    let mut list = SlotList::new(&mut md.player.inventory, &limit);
                    let put = slots::insert(&mut list, h.item, h.count);
                    if put < h.count {
                        md.player.hand = Some(ItemStack::new(h.item, h.count - put));
                    }
                }
            }
            UiAction::Craft { recipe, count } => self.craft(recipe, count),
            UiAction::CancelCraft { index, count } => self.cancel(index, count),
            UiAction::SetRecipe { recipe, .. } => {
                if let Some(bv) = md.building.as_mut() {
                    bv.recipe = recipe;
                    bv.status = if recipe.is_some() { MachineStatus::NoInput } else { MachineStatus::NoRecipe };
                    bv.progress = 0.0;
                    let filters: Vec<ItemId> = recipe.and_then(|r| md.catalog.recipe(r)).map(|r| r.ingredients.iter().map(|i| i.item).collect()).unwrap_or_default();
                    for (i, s) in bv.inputs.iter_mut().enumerate() {
                        s.filter = filters.get(i).copied();
                    }
                }
            }
            UiAction::ShowAlert(id) => md.message = format!("The camera moves to alert {id}."),
            UiAction::NewGame { seed, size } => {
                *md = model(&self.content);
                let (w, h) = size.cells();
                md.message = format!("New world: seed {seed}, {w} × {h} cells.");
            }
            UiAction::Continue => {
                md.state = GameState::Playing;
                md.message = "Loaded the newest save.".into();
            }
            UiAction::Pause => md.state = GameState::Paused,
            UiAction::Resume => md.state = GameState::Playing,
            UiAction::Save { name, overwrite } => {
                if overwrite {
                    md.saves.retain(|s| s.name != name);
                }
                self.save_counter += 1;
                md.saves.insert(
                    0,
                    SaveInfo {
                        id: format!("save-{}", self.save_counter),
                        name: name.clone(),
                        date: "2026-09-27 17:00".into(),
                        play_time_s: 4 * 3600 + 30 * 60,
                        world: "Seed 81234, 8192 × 8192".into(),
                    },
                );
                md.message = format!("Saved \"{name}\".");
            }
            UiAction::Load(id) => {
                let name = md.saves.iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or(id);
                md.state = GameState::Playing;
                md.message = format!("Loaded \"{name}\".");
            }
            UiAction::DeleteSave(id) => md.saves.retain(|s| s.id != id),
            UiAction::QuitToMenu => {
                md.state = GameState::MainMenu;
                md.building = None;
                md.power = None;
            }
            UiAction::QuitGame => self.quit = true,
            UiAction::ChangeSetting(s) => match s {
                SettingChange::UiScale(v) => md.settings.ui_scale = v,
                SettingChange::Vsync(v) => md.settings.vsync = v,
                SettingChange::ShowFps(v) => md.settings.show_fps = v,
            },
        }
    }

    /// Advance crafting by `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        let md = &mut self.model;
        if md.state != GameState::Playing {
            return;
        }
        if let Some(bv) = md.building.as_mut()
            && bv.status == MachineStatus::Working
        {
            bv.progress = (bv.progress + dt / 5.0).fract();
        }
        let Some(job) = md.player.crafting.first_mut() else { return };
        let Some(r) = md.catalog.recipe(job.recipe).cloned() else {
            md.player.crafting.remove(0);
            return;
        };
        job.progress += dt * md.player.craft_speed.max(0.1) / r.time.max(0.1);
        if job.progress >= 1.0 {
            job.progress = 0.0;
            job.count -= 1;
            if job.count == 0 {
                md.player.crafting.remove(0);
            }
            for res in &r.results {
                self.give(res.item, res.amount);
            }
        }
    }

    fn give(&mut self, item: ItemId, count: u32) {
        let pl = &mut self.model.player;
        if item.is_bulk() {
            let ItemId::Material(mat) = item else { return };
            let mut left = count;
            for t in pl.tank.iter_mut() {
                if left == 0 {
                    break;
                }
                if t.material == Some(mat) || t.material.is_none() {
                    let n = (t.capacity - t.units).min(left);
                    if n > 0 {
                        t.material = Some(mat);
                        t.units += n;
                        left -= n;
                    }
                }
            }
        } else {
            let cat = &self.model.catalog;
            let limit = |_: usize, it: ItemId| if it.is_bulk() { 0 } else { cat.stack_size(it) };
            let mut list = SlotList::new(&mut pl.inventory, &limit);
            slots::insert(&mut list, item, count);
        }
    }

    fn take(&mut self, item: ItemId, mut count: u32) {
        let pl = &mut self.model.player;
        if let ItemId::Material(mat) = item {
            for t in pl.tank.iter_mut().rev() {
                if t.material == Some(mat) {
                    let n = t.units.min(count);
                    t.units -= n;
                    count -= n;
                    if t.units == 0 {
                        t.material = None;
                    }
                }
            }
        } else {
            for s in pl.inventory.iter_mut().rev() {
                if let Some(st) = s
                    && st.item == item
                {
                    let n = st.count.min(count);
                    st.count -= n;
                    count -= n;
                    if st.count == 0 {
                        *s = None;
                    }
                }
            }
        }
    }

    fn craft(&mut self, recipe: RecipeId, count: u32) {
        let Some(r) = self.model.catalog.recipe(recipe).cloned() else { return };
        let stock = Stock::from_player(&self.model.player);
        let n = count.min(craftable_count(&r, &stock));
        if n == 0 {
            return;
        }
        for ing in &r.ingredients {
            self.take(ing.item, ing.amount * n);
        }
        let q = &mut self.model.player.crafting;
        match q.last_mut() {
            Some(last) if last.recipe == recipe && q.len() > 1 => last.count += n,
            _ => q.push(CraftJobView { recipe, count: n, progress: 0.0 }),
        }
    }

    fn cancel(&mut self, index: usize, count: u32) {
        let q = &mut self.model.player.crafting;
        let Some(job) = q.get_mut(index) else { return };
        let n = count.min(job.count);
        job.count -= n;
        let recipe = job.recipe;
        if job.count == 0 {
            q.remove(index);
        } else if index == 0 && n > 0 && job.count > 0 {
            // Keep the progress of the run in work.
        }
        if let Some(r) = self.model.catalog.recipe(recipe).cloned() {
            for ing in &r.ingredients {
                self.give(ing.item, ing.amount * n);
            }
        }
    }

    fn click_slot(&mut self, slot: SlotRef, click: crate::action::SlotClick) {
        let md = &mut self.model;
        let cat = &md.catalog;
        let inv_limit = |_: usize, it: ItemId| if it.is_bulk() { 0 } else { cat.stack_size(it) };
        let caps: Vec<u32> = md.player.tank.iter().map(|t| t.capacity).collect();
        let tank_limit = |i: usize, it: ItemId| if it.is_bulk() { caps.get(i).copied().unwrap_or(0) } else { 0 };
        let mut tank: Vec<Option<ItemStack>> =
            md.player.tank.iter().map(|t| t.material.map(|m| ItemStack::new(ItemId::Material(m), t.units))).collect();
        let hand = &mut md.player.hand;
        // Building slots, as plain lists.
        let (mut b_in, mut b_out, mut b_fuel, filters_in, filters_fuel) = match md.building.as_ref() {
            Some(bv) => (
                bv.inputs.iter().map(|s| s.stack).collect::<Vec<_>>(),
                bv.outputs.iter().map(|s| s.stack).collect::<Vec<_>>(),
                bv.fuel.iter().map(|s| s.stack).collect::<Vec<_>>(),
                bv.inputs.iter().map(|s| s.filter).collect::<Vec<_>>(),
                bv.fuel.iter().map(|s| s.filter).collect::<Vec<_>>(),
            ),
            None => (vec![], vec![], vec![], vec![], vec![]),
        };
        let in_limit = |i: usize, it: ItemId| match filters_in.get(i).copied().flatten() {
            Some(f) if f != it => 0,
            _ => cat.stack_size(it),
        };
        let fuel_limit = |i: usize, it: ItemId| match filters_fuel.get(i).copied().flatten() {
            Some(f) if f == it => if it.is_bulk() { 200 } else { cat.stack_size(it) },
            _ => 0,
        };
        let no_limit = |_: usize, _: ItemId| 0;
        let building_open = md.building.is_some();
        match slot {
            SlotRef::Inventory(i) => {
                let mut this = SlotList::new(&mut md.player.inventory, &inv_limit);
                if building_open {
                    let mut others = [SlotList::new(&mut b_fuel, &fuel_limit), SlotList::new(&mut b_in, &in_limit)];
                    slots::apply(hand, &mut this, i, &mut others, click);
                } else {
                    slots::apply(hand, &mut this, i, &mut [], click);
                }
            }
            SlotRef::Tank(i) => {
                let mut this = SlotList::new(&mut tank, &tank_limit);
                if building_open {
                    let mut others = [SlotList::new(&mut b_fuel, &fuel_limit), SlotList::new(&mut b_in, &in_limit)];
                    slots::apply(hand, &mut this, i, &mut others, click);
                } else {
                    slots::apply(hand, &mut this, i, &mut [], click);
                }
            }
            SlotRef::Building { group, index, .. } => {
                let (list, limit): (&mut Vec<Option<ItemStack>>, &dyn Fn(usize, ItemId) -> u32) = match group {
                    BuildingSlots::Input => (&mut b_in, &in_limit),
                    BuildingSlots::Output => (&mut b_out, &no_limit),
                    BuildingSlots::Fuel => (&mut b_fuel, &fuel_limit),
                };
                let mut this = SlotList::new(list, limit);
                let mut others = [SlotList::new(&mut md.player.inventory, &inv_limit), SlotList::new(&mut tank, &tank_limit)];
                slots::apply(hand, &mut this, index, &mut others, click);
            }
        }
        // Write the plain lists back.
        for (t, s) in md.player.tank.iter_mut().zip(tank) {
            match s {
                Some(ItemStack { item: ItemId::Material(m), count }) => {
                    t.material = Some(m);
                    t.units = count;
                }
                _ => {
                    t.material = None;
                    t.units = 0;
                }
            }
        }
        if let Some(bv) = md.building.as_mut() {
            for (s, v) in bv.inputs.iter_mut().zip(b_in) {
                s.stack = v;
            }
            for (s, v) in bv.outputs.iter_mut().zip(b_out) {
                s.stack = v;
            }
            for (s, v) in bv.fuel.iter_mut().zip(b_fuel) {
                s.stack = v;
            }
        }
    }
}

/// Paint a simple side view of the world behind the HUD, so screenshots show the UI on a
/// realistic background. Only for the preview and the tests.
pub fn paint_world(p: &egui::Painter, screen: egui::Rect) {
    use egui::{Color32, Mesh, Rect, Shape, pos2, vec2};
    let cell = 6.0;
    let surface = screen.top() + screen.height() * 0.46;
    let mut mesh = Mesh::default();
    // Sky.
    let top_c = Color32::from_rgb(98, 146, 196);
    let bot_c = Color32::from_rgb(176, 202, 222);
    let i0 = mesh.vertices.len() as u32;
    mesh.colored_vertex(screen.left_top(), top_c);
    mesh.colored_vertex(screen.right_top(), top_c);
    mesh.colored_vertex(pos2(screen.right(), surface + 40.0), bot_c);
    mesh.colored_vertex(pos2(screen.left(), surface + 40.0), bot_c);
    mesh.add_triangle(i0, i0 + 1, i0 + 2);
    mesh.add_triangle(i0, i0 + 2, i0 + 3);
    let hash = |x: i32, y: i32| -> u32 {
        let mut h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1);
        h ^= h >> 15;
        h = h.wrapping_mul(0x85eb_ca6b);
        h ^ (h >> 13)
    };
    let cols = (screen.width() / cell) as i32 + 1;
    let rows = (screen.height() / cell) as i32 + 1;
    for cx in 0..cols {
        let x = screen.left() + cx as f32 * cell;
        let ground = surface + ((cx as f32 * 0.05).sin() * 22.0 + (cx as f32 * 0.013).sin() * 40.0);
        for cy in 0..rows {
            let y = screen.top() + cy as f32 * cell;
            if y < ground {
                continue;
            }
            let d = y - ground;
            let n = hash(cx, cy);
            let v = (n % 24) as f32 / 100.0 + 0.88;
            let cave = ((cx as f32 - cols as f32 * 0.62).powi(2) / 900.0 + (cy as f32 - rows as f32 * 0.74).powi(2) / 60.0) < 1.0;
            let lava = cave && (cy as f32) > rows as f32 * 0.76;
            let base = if lava {
                Color32::from_rgb(255, 120, 30)
            } else if cave {
                Color32::from_rgb(28, 24, 26)
            } else if d < cell * 2.0 {
                Color32::from_rgb(84, 140, 60)
            } else if d < 90.0 {
                Color32::from_rgb(110, 78, 50)
            } else if (n % 97) < 3 {
                Color32::from_rgb(40, 130, 90) // malachite
            } else {
                Color32::from_rgb(104, 104, 110)
            };
            let c = crate::theme::shade(base, v * if lava { 1.0 } else { (1.0 - d / screen.height() * 0.6).max(0.35) });
            mesh.add_colored_rect(Rect::from_min_size(pos2(x, y), vec2(cell, cell)), c);
        }
    }
    p.add(Shape::mesh(mesh));
    // The robot.
    let robot = Rect::from_min_size(pos2(screen.center().x - 12.0, surface - 44.0), vec2(24.0, 48.0));
    p.rect_filled(robot, 3.0, Color32::from_rgb(220, 170, 60));
    p.rect_filled(Rect::from_min_size(robot.min + vec2(4.0, 6.0), vec2(16.0, 10.0)), 2.0, Color32::from_rgb(40, 60, 80));
}
