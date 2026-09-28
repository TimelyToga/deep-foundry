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
    player.dig("wood", |p| p.count("wood") >= 30)?;
    player.walk_to(player.start.x)?;
    player.craft("stamp_mill", 1)?;
    let kind = player
        .content
        .factory
        .building("stamp_mill")
        .ok_or("no stamp mill")?;
    let before = player.host.factory.buildings.count_of(kind);
    player.place("stamp_mill")?;
    if player.host.factory.buildings.count_of(kind) <= before {
        return Err("the stamp mill was not placed".into());
    }
    let stamp = find_building(player, "stamp_mill")?;
    run_machine(
        player,
        stamp,
        "crushed_malachite",
        "raw_malachite",
        None,
        "crushed_malachite",
        16,
    )
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
            click: SlotClick::LEFT,
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
            click: SlotClick::LEFT,
        }]);
    }

    let output = player.content.expect_material(output_name);
    let mut made = 0;
    for _ in 0..240 {
        if let Some(view) = player.host.factory.building_view(id) {
            made = view
                .outputs
                .iter()
                .find(|slot| slot.item == ItemRef::Material(output))
                .map_or(0, |slot| slot.count);
            if made >= needed {
                break;
            }
        }
        player.ticks(30);
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
