//! Player actions for the ore-processing guide goals.
//!
//! The factory integration test covers the autonomous hopper/belt/sluice line. This guide
//! script uses the player's real inventory, recipes, placements and building windows so the
//! goals are reachable through normal play.

use super::tier0_tests::Player;
use foundry_content::ItemRef;
use foundry_core::{BuildingId, CellPos};
use foundry_ui::{BuildingSlots, SlotClick, SlotRef, UiAction};

pub(super) fn process(player: &mut Player, wash: bool) -> Result<(), String> {
    if wash {
        wash_ore(player)
    } else {
        crush_ore(player)
    }
}

fn crush_ore(player: &mut Player) -> Result<(), String> {
    player.dig("malachite", |p| p.count("raw_malachite") >= 16)?;
    player.dig("wood", |p| p.count("wood") >= 40)?;
    player.walk_to(player.start.x)?;
    player.craft("stamp_mill", 1)?;
    player.craft("crate", 1)?;
    player.place("stamp_mill")?;
    let stamp = find_building(player, "stamp_mill")?;
    // A crate at the output port collects the crushed ore (its powder input is on its left).
    let out = player
        .host
        .factory
        .buildings
        .get(stamp)
        .and_then(|b| b.ports.iter().find(|p| p.kind == foundry_content::PortKind::BulkOut).copied())
        .ok_or("the stamp mill has no output port")?;
    let at = foundry_factory::geometry::neighbor_tile(out.tile, out.side);
    let crate_ = player.place_at("crate", at)?;
    let recipe = player.content.factory.recipe("crushed_malachite").ok_or("no recipe")?;
    player.open_id(stamp)?;
    player.ui(&[UiAction::SetRecipe { building: stamp, recipe: Some(recipe) }]);
    player.sync();
    let tank = tank_index(player, "raw_malachite")?;
    player.ui(&[UiAction::ClickSlot { slot: SlotRef::Tank(tank), click: SlotClick::CTRL_LEFT }]);
    let crushed = player.content.item("crushed_malachite").unwrap();
    for _ in 0..240 {
        if player.host.factory.buildings.inventory(crate_).is_some_and(|inv| inv.count(crushed) >= 16) {
            break;
        }
        player.ticks(30);
    }
    player.open_id(crate_)?;
    for index in 0..8 {
        player.ui(&[UiAction::ClickSlot { slot: SlotRef::Building { building: crate_, group: BuildingSlots::Input, index }, click: SlotClick::SHIFT_LEFT }]);
    }
    if player.count("crushed_malachite") >= 16 {
        Ok(())
    } else {
        Err(format!("the stamp mill made {} crushed malachite: {:?}", player.count("crushed_malachite"), player.host.factory.building_view(stamp)))
    }
}

/// A crate at the output port of a machine for `item`, when that port faces right and has
/// nothing in front of it (the crate's powder input is on its left): placed now if needed.
fn output_crate(player: &mut Player, id: BuildingId, item: ItemRef) -> Result<Option<BuildingId>, String> {
    let b = player.host.factory.buildings.get(id).ok_or("no machine")?;
    let def = player.content.factory.building_def(b.kind);
    let port = b.ports.iter().find(|p| {
        p.kind == foundry_content::PortKind::BulkOut
            && p.side == foundry_content::Side::Right
            && p.def.is_none_or(|i| def.ports[i as usize].filter.is_empty() || def.ports[i as usize].filter.contains(&item))
    });
    let Some(port) = port.copied() else { return Ok(None) };
    let at = foundry_factory::geometry::neighbor_tile(port.tile, port.side);
    if let Some(existing) = player.host.factory.buildings.at_tile(at, foundry_content::Layer::Front) {
        return Ok(player.host.factory.buildings.inventory(existing).map(|_| existing));
    }
    player.dig("wood", |p| p.count("wood") >= 8)?;
    player.craft("crate", 1)?;
    player.place_at("crate", at).map(Some)
}

fn tank_index(player: &Player, material: &str) -> Result<usize, String> {
    let m = ItemRef::Material(player.content.expect_material(material));
    player.normal.frame.inventory.tanks.iter().position(|t| t.item == Some(m)).ok_or_else(|| format!("no {material} in the player's tanks"))
}

fn wash_ore(player: &mut Player) -> Result<(), String> {
    if player.count("crushed_malachite") < 4 {
        return Err("the player needs crushed malachite from the Crush ore goal".into());
    }
    player.dig("water", |p| p.count("water") >= 1)?;
    let sluice = find_building(player, "sluice")?;
    run_machine(
        player,
        sluice,
        "washed_malachite",
        "crushed_malachite",
        Some("water"),
        "washed_malachite",
        3,
    )
}

fn find_building(player: &Player, name: &str) -> Result<BuildingId, String> {
    let kind = player
        .content
        .factory
        .building(name)
        .ok_or_else(|| format!("no {name}"))?;
    player
        .host
        .factory
        .buildings
        .iter()
        .find(|(_, building)| building.kind == kind)
        .map(|(id, _)| id)
        .ok_or_else(|| format!("no placed {name}"))
}

fn run_machine(
    player: &mut Player,
    id: BuildingId,
    recipe_name: &str,
    input_name: &str,
    extra_input: Option<&str>,
    output_name: &str,
    needed: u32,
) -> Result<(), String> {
    let recipe = player
        .content
        .factory
        .recipe(recipe_name)
        .ok_or_else(|| format!("no {recipe_name} recipe"))?;
    let building = player
        .host
        .factory
        .buildings
        .get(id)
        .ok_or("building disappeared")?;
    let rect = building.cell_rect();
    player.walk_to((rect.x0 + rect.x1) / 2)?;
    player.open(CellPos::new(
        (rect.x0 + rect.x1) / 2,
        (rect.y0 + rect.y1) / 2,
    ))?;

    let input = player.content.expect_material(input_name);
    let input_tank = player
        .normal
        .frame
        .inventory
        .tanks
        .iter()
        .position(|tank| tank.item == Some(ItemRef::Material(input)))
        .ok_or_else(|| format!("no {input_name} in the player's tanks"))?;
    player.ui(&[
        UiAction::SetRecipe {
            building: id,
            recipe: Some(recipe),
        },
        UiAction::ClickSlot {
            slot: SlotRef::Tank(input_tank),
            click: SlotClick::CTRL_LEFT,
        },
    ]);
    if let Some(extra_name) = extra_input {
        let extra = player.content.expect_material(extra_name);
        let tank = player
            .normal
            .frame
            .inventory
            .tanks
            .iter()
            .position(|tank| tank.item == Some(ItemRef::Material(extra)))
            .ok_or_else(|| format!("no {extra_name} in the player's tanks"))?;
        player.ui(&[UiAction::ClickSlot {
            slot: SlotRef::Tank(tank),
            click: SlotClick::CTRL_LEFT,
        }]);
    }

    let output = player.content.expect_material(output_name);
    let crate_ = output_crate(player, id, ItemRef::Material(output))?;
    let in_crate = |player: &Player| crate_.and_then(|c| player.host.factory.buildings.inventory(c)).map_or(0, |inv| inv.count(ItemRef::Material(output)));
    let mut made = 0;
    for _ in 0..240 {
        if let Some(view) = player.host.factory.building_view(id) {
            made = view
                .outputs
                .iter()
                .find(|slot| slot.item == ItemRef::Material(output))
                .map_or(0, |slot| slot.count)
                + in_crate(player);
            if made >= needed {
                break;
            }
        }
        player.ticks(30);
    }
    if let Some(c) = crate_ {
        player.open_id(c)?;
        for index in 0..8 {
            player.ui(&[UiAction::ClickSlot { slot: SlotRef::Building { building: c, group: BuildingSlots::Input, index }, click: SlotClick::SHIFT_LEFT }]);
        }
        player.open_id(id)?;
    }
    if made < needed {
        return Err(format!(
            "{recipe_name} made {made} {output_name}; machine={:?}",
            player.host.factory.building_view(id)
        ));
    }
    player.ui(&[UiAction::ClickSlot {
        slot: SlotRef::Building {
            building: id,
            group: BuildingSlots::Output,
            index: 0,
        },
        click: SlotClick::LEFT,
    }]);
    if player.count(output_name) >= needed {
        Ok(())
    } else {
        Err(format!("could not take {output_name} from the machine"))
    }
}
