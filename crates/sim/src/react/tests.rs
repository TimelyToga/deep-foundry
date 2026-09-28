//! Tests of reactions, burning, charring and timers. They use the real data files.

use super::burn::LIFE_BURNING;
use crate::chunk::FLAG_BURNING;
use super::*;
use crate::{SimConfig, Simulation};
use foundry_core::CellRect;
use std::sync::Arc;

fn content() -> Arc<Content> {
    Arc::new(Content::load_default().expect("data files load"))
}

/// A small test world: a finite box, all air, with a bedrock border.
struct World {
    s: Simulation,
}

impl World {
    fn new(w: i32, h: i32) -> World {
        World { s: Simulation::new(content(), SimConfig::finite(w, h, 7)) }
    }

    /// A test world where no heat flows between cells: every material has conductivity 0.
    /// The cells keep the temperatures the test gives them, so a test checks only the reaction
    /// rules. (Phase changes still happen; the scene tests check heat and reactions together.)
    fn without_heat_flow(w: i32, h: i32) -> World {
        let mut c = Content::load_default().expect("data files load");
        c.materials.conductivity.iter_mut().for_each(|k| *k = 0.0);
        World { s: Simulation::new(Arc::new(c), SimConfig::finite(w, h, 7)) }
    }

    fn mat(&self, name: &str) -> MaterialId {
        self.s.content().expect_material(name)
    }

    /// Fill the rectangle x0..x1, y0..y1 (`temp: None` = the material's own temperature).
    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, name: &str, temp: Option<i16>) {
        let m = self.mat(name);
        for y in y0..y1 {
            for x in x0..x1 {
                self.s.set_cell(CellPos::new(x, y), m, temp);
            }
        }
    }

    fn count(&self, name: &str) -> usize {
        let (w, h) = self.s.size_cells();
        self.s.count_material(CellRect::new(0, 0, w, h), self.mat(name))
    }

    fn count_in(&self, x0: i32, y0: i32, x1: i32, y1: i32, name: &str) -> usize {
        self.s.count_material(CellRect::new(x0, y0, x1, y1), self.mat(name))
    }

    /// Life byte and flags of a cell.
    fn life_flags(&self, x: i32, y: i32) -> (u8, u8) {
        let p = CellPos::new(x, y);
        let c = self.s.world().chunk(p.chunk()).expect("live chunk");
        (c.life[p.local_index()], c.flags[p.local_index()])
    }

    /// Cells that burn now (life bit), and how many of them have the burning flag.
    fn burning(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> (usize, usize) {
        let (mut n, mut flagged) = (0, 0);
        for y in y0..y1 {
            for x in x0..x1 {
                let m = self.s.cell(CellPos::new(x, y)).material;
                if self.s.content().materials.burn[m.index()].is_none() {
                    continue;
                }
                let (life, flags) = self.life_flags(x, y);
                if life & LIFE_BURNING != 0 {
                    n += 1;
                    flagged += (flags & FLAG_BURNING != 0) as usize;
                }
            }
        }
        (n, flagged)
    }

    fn ticks(&mut self, n: usize) {
        for _ in 0..n {
            self.s.tick();
        }
    }

    /// Run `n` ticks and collect the events of every tick.
    fn ticks_events(&mut self, n: usize) -> Vec<Vec<SimEvent>> {
        (0..n)
            .map(|_| {
                self.s.tick();
                self.s.events().to_vec()
            })
            .collect()
    }

    fn reaction_index(&self, a: &str, b: &str) -> u16 {
        let c = self.s.content();
        let (ma, mb) = (c.expect_material(a), c.expect_material(b));
        c.reactions.iter().position(|r| r.a.matches(ma, &c.materials) && r.b.matches(mb, &c.materials)).expect("reaction exists") as u16
    }

    /// A closed stone box with inside x0..x1, y0..y1 (walls 2 cells thick).
    fn stone_box(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.fill(x0 - 2, y0 - 2, x1 + 2, y1 + 2, "stone", None);
        self.fill(x0, y0, x1, y1, "air", None);
    }
}

#[test]
fn table_expands_tags_and_words() {
    let c = content();
    let t = ReactTable::new(&c);
    let m = |n: &str| c.expect_material(n);
    // "tag:metal + water" with "$freeze": molten metals only, from both sides.
    assert!(t.rule_count(m("molten_copper"), m("water")) >= 1);
    assert!(t.rule_count(m("water"), m("molten_copper")) >= 1);
    assert_eq!(t.rule_count(m("copper_block"), m("water")), 0, "a block has no freeze result");
    assert_eq!(t.rule_count(m("copper_dust"), m("water")), 0);
    // "brine + any": only the brine side looks.
    assert!(t.rule_count(m("brine"), m("stone")) >= 1);
    assert_eq!(t.rule_count(m("stone"), m("brine")), 0);
    // Nothing to do for sand and stone: the fast path.
    for n in ["sand", "stone", "bedrock", "gravel", "smoke"] {
        assert!(!t.has_work(m(n)), "{n} has no work");
    }
    for n in ["lava", "wood", "fire", "wet_concrete", "oil", "molten_copper", "salt", "dirt"] {
        assert!(t.has_work(m(n)), "{n} has work");
    }
    // Water has rules, but each partner (lava, fire, salt, ...) starts them.
    assert!(!t.has_work(m("water")), "water leaves its reactions to the partner");
    assert!(t.rule_count(m("water"), m("lava")) >= 1);
    assert_eq!(t.wake_temp(m("wood")), Some(300));
    assert_eq!(t.wake_temp(m("raw_malachite")), Some(1100));
    assert_eq!(t.wake_temp(m("sand")), None);
    assert_eq!(t.reaction_count(), c.reactions.len());
}

#[test]
fn product_temperature_stays_in_the_stable_range() {
    let c = content();
    let mats = &c.materials;
    let m = |n: &str| c.expect_material(n);
    // A block made from molten copper stays just below its melting point.
    let melt = mats.melt[m("copper_block").index()].unwrap().at;
    assert_eq!(product_temp(mats, m("copper_block"), 1150), melt - 1);
    // Steam made from cold water is at least steam's own temperature.
    assert_eq!(product_temp(mats, m("steam"), 20), mats.temperature[m("steam").index()]);
    // Stone keeps the temperature.
    assert_eq!(product_temp(mats, m("stone"), 300), 300);
}

#[test]
fn water_and_lava_make_obsidian_and_steam() {
    let mut w = World::new(2, 2);
    w.stone_box(10, 60, 118, 120);
    w.fill(10, 100, 60, 120, "lava", None);
    w.fill(60, 100, 118, 120, "water", None);
    let index = w.reaction_index("water", "lava");
    let events = w.ticks_events(200);
    assert!(w.count("obsidian") >= 10, "obsidian: {}", w.count("obsidian"));
    assert!(w.count("steam") >= 10, "steam: {}", w.count("steam"));
    let seen = events.iter().flatten().filter(|e| matches!(e, SimEvent::Reaction { index: i, .. } if *i == index)).count();
    assert!(seen > 0, "the reaction sends a Reaction event");
    for tick in &events {
        let n = tick.iter().filter(|e| matches!(e, SimEvent::Reaction { index: i, .. } if *i == index)).count();
        assert!(n <= 1, "at most one event per reaction per tick, got {n}");
    }
}

#[test]
fn reaction_events_are_the_same_for_any_thread_count() {
    let run = |threads: usize| {
        let mut w = World::new(4, 2);
        w.s.set_threads(threads);
        w.fill(4, 90, 252, 126, "lava", None);
        w.fill(4, 60, 252, 90, "water", None);
        w.fill(100, 20, 140, 40, "methane", None);
        w.fill(118, 42, 122, 44, "fire", None);
        let events = w.ticks_events(120);
        (events, w.s.world_hash())
    };
    let (a, ha) = run(1);
    let (b, hb) = run(4);
    assert_eq!(ha, hb, "world hash");
    assert_eq!(a, b, "events");
    assert!(a.iter().flatten().any(|e| matches!(e, SimEvent::Reaction { .. })));
}

#[test]
fn slow_reaction_keeps_going_until_it_happens() {
    // Rust: iron in water with air next to it, chance 0.001 per tick. The chunk must not fall
    // asleep while the reaction waits.
    let mut w = World::new(2, 2);
    w.stone_box(20, 60, 100, 120);
    w.fill(20, 100, 100, 120, "water", None);
    w.fill(50, 90, 70, 120, "pig_iron_block", None);
    w.ticks(300);
    assert!(w.s.stats().awake_chunks > 0, "the waiting reaction keeps the chunk awake");
    w.ticks(3000);
    assert!(w.count("rust") > 0, "iron at the water line rusts");
    // Only cells with air next to them rust: the iron under the water stays.
    assert_eq!(w.count_in(50, 105, 70, 120, "rust"), 0);
}

#[test]
fn chunks_sleep_when_nothing_can_react() {
    let mut w = World::new(2, 2);
    w.stone_box(10, 60, 118, 120);
    w.fill(10, 110, 118, 120, "water", None);
    w.fill(20, 100, 40, 110, "sand", None);
    // Cold wood, cold ore on fuel (the smelting rule needs 900 C): nothing happens.
    w.fill(60, 80, 70, 100, "wood", None);
    w.fill(80, 104, 90, 110, "crushed_cassiterite", None);
    w.fill(90, 104, 100, 110, "charcoal", None);
    w.ticks(1500);
    assert_eq!(w.s.stats().awake_chunks, 0, "all chunks sleep");
    assert_eq!(w.count("wood"), 200);
}

#[test]
fn hot_wood_in_air_burns_to_ash_and_smoke() {
    let mut w = World::new(2, 2);
    w.fill(4, 120, 124, 124, "stone", None);
    // A wooden beam; its left end is hot.
    w.fill(20, 100, 100, 104, "wood", None);
    w.fill(20, 100, 24, 104, "wood", Some(400));
    let wood = w.count("wood");
    let mut max_burning = 0;
    let mut smoke = 0;
    for _ in 0..30 {
        w.ticks(20);
        let (n, flagged) = w.burning(0, 0, 128, 128);
        assert_eq!(n, flagged, "every burning cell has the burning flag");
        max_burning = max_burning.max(n);
        smoke = smoke.max(w.count("smoke"));
    }
    assert!(max_burning > 5, "the beam burns: {max_burning}");
    assert!(smoke > 0, "burning wood makes smoke");
    assert!(w.count("ash") > 10, "ash: {}", w.count("ash"));
    assert!(w.count("wood") < wood);
    // The fire spread along the beam: the right half burned too.
    assert!(w.count_in(60, 90, 100, 104, "wood") < 160, "right half: {}", w.count_in(60, 90, 100, 104, "wood"));
    // Burnt out wood leaves no burning flags on other materials.
    for y in 90..124 {
        for x in 4..124 {
            let m = w.s.cell(CellPos::new(x, y)).material;
            if w.s.content().materials.burn[m.index()].is_none() && !m.is_air() {
                assert_eq!(w.life_flags(x, y).1 & FLAG_BURNING, 0, "no stale flag on {x},{y}");
            }
        }
    }
}

#[test]
fn fire_ignites_cold_wood() {
    let mut w = World::new(1, 1);
    w.fill(20, 40, 44, 44, "wood", None);
    w.fill(30, 44, 34, 46, "fire", None);
    let mut burned = false;
    for _ in 0..40 {
        w.ticks(1);
        burned |= w.burning(20, 40, 44, 44).0 > 0;
    }
    assert!(burned, "fire under the wood sets it on fire");
}

#[test]
fn water_puts_out_burning_wood() {
    let mut w = World::new(2, 2);
    w.stone_box(20, 60, 100, 120);
    w.fill(40, 90, 80, 120, "wood", Some(500));
    w.ticks(10);
    let (burning, _) = w.burning(40, 90, 80, 120);
    assert!(burning > 0, "the wood burns");
    // Flood the box: every cell that is not wood or ash becomes water.
    let (wood, ash, water) = (w.mat("wood"), w.mat("ash"), w.mat("water"));
    for y in 60..120 {
        for x in 20..100 {
            let m = w.s.cell(CellPos::new(x, y)).material;
            if m != wood && m != ash {
                w.s.set_cell(CellPos::new(x, y), water, None);
            }
        }
    }
    w.ticks(20);
    assert_eq!(w.burning(20, 60, 100, 120).0, 0, "the water put the fire out");
    assert!(w.count("steam") > 0, "water on the fire boils");
    w.ticks(300);
    assert_eq!(w.burning(20, 60, 100, 120).0, 0, "the wood does not start again");
}

#[test]
fn water_next_to_burning_wood_puts_it_out_and_cools_it() {
    // One hot wood cell on a stone floor, with air above it and a stone cup to its right.
    let mut w = World::new(1, 1);
    w.fill(10, 40, 30, 50, "stone", None);
    w.fill(20, 39, 21, 40, "stone", None);
    w.fill(18, 39, 19, 40, "wood", Some(500));
    w.ticks(2);
    assert_eq!(w.burning(18, 39, 19, 40).0, 1, "it burns");
    // Water into the cup, next to the burning wood.
    w.fill(19, 39, 20, 40, "water", None);
    w.ticks(2);
    assert_eq!(w.burning(18, 39, 19, 40).0, 0, "the water put it out");
    let cell = w.s.cell(CellPos::new(18, 39));
    assert_eq!(cell.material, w.mat("wood"));
    assert!(cell.temperature <= 100, "cooled: {} C", cell.temperature);
    assert_eq!((w.count("water"), w.count("steam")), (0, 1), "the water boiled");
    assert_eq!(w.life_flags(18, 39).1 & FLAG_BURNING, 0, "no burning flag");
    w.ticks(100);
    assert_eq!(w.burning(0, 0, 64, 64).0, 0, "cooled wood does not start again");
}

#[test]
fn carbon_dioxide_puts_out_burning_wood() {
    let mut w = World::new(2, 2);
    w.stone_box(20, 60, 100, 120);
    w.fill(40, 110, 80, 120, "wood", Some(500));
    w.ticks(20);
    assert!(w.burning(40, 110, 80, 120).0 > 0, "the wood burns");
    w.fill(20, 60, 100, 110, "carbon_dioxide", None);
    w.ticks(40);
    assert_eq!(w.burning(40, 110, 80, 120).0, 0, "carbon dioxide put the fire out");
    assert!(w.count("wood") > 0);
}

#[test]
fn wood_heated_without_air_becomes_charcoal() {
    let mut w = World::without_heat_flow(2, 2);
    // Wood fills a closed box: no air next to any wood cell.
    w.stone_box(40, 80, 60, 100);
    w.fill(40, 80, 60, 100, "wood", Some(450));
    w.ticks(300);
    assert_eq!(w.count("charcoal"), 0, "not yet after 5 seconds");
    assert_eq!(w.burning(40, 80, 60, 100).0, 0, "no air: it does not burn");
    w.ticks(700);
    assert!(w.count("charcoal") >= 380, "charcoal after about 10 seconds: {}", w.count("charcoal"));
}

#[test]
fn oil_burns_only_at_its_surface() {
    let mut w = World::new(2, 2);
    w.stone_box(20, 60, 100, 120);
    w.fill(20, 100, 100, 120, "oil", None);
    w.fill(20, 100, 100, 101, "oil", Some(300));
    w.ticks(10);
    let (top, _) = w.burning(20, 100, 100, 102);
    let (deep, _) = w.burning(20, 103, 100, 120);
    assert!(top > 20, "the surface burns: {top}");
    assert_eq!(deep, 0, "oil under the surface has no air and does not burn");
    let oil = w.count("oil");
    w.ticks(400);
    assert!(w.count("oil") < oil, "the oil burns away");
    assert!(w.count_in(20, 115, 100, 120, "oil") >= 390, "the bottom stays oil");
}

#[test]
fn methane_and_fire_make_an_explosion_event() {
    let mut w = World::new(1, 1);
    w.fill(10, 10, 50, 30, "methane", None);
    w.fill(28, 31, 32, 33, "fire", None);
    let events = w.ticks_events(60);
    let explosions: Vec<(CellPos, f32, i16)> = events
        .iter()
        .flatten()
        .filter_map(|e| if let SimEvent::Explosion { at, strength, heat } = *e { Some((at, strength, heat)) } else { None })
        .collect();
    assert!(!explosions.is_empty(), "methane touching fire explodes");
    let (at, strength, heat) = explosions[0];
    assert_eq!((strength, heat), EXPLOSION_SMALL);
    assert!((10..50).contains(&at.x) && (10..34).contains(&at.y), "at the methane: {at:?}");
}

#[test]
fn molten_metal_in_water_freezes_into_its_block() {
    let mut w = World::without_heat_flow(2, 2);
    w.stone_box(20, 60, 100, 120);
    w.fill(20, 100, 100, 120, "water", None);
    w.fill(50, 70, 70, 80, "molten_copper", None);
    w.fill(80, 70, 90, 75, "molten_tin", None);
    let events = w.ticks_events(200);
    assert!(w.count("copper_block") > 0, "copper freezes");
    assert!(w.count("tin_block") > 0, "tin freezes into tin, not copper");
    assert!(w.count("steam") > 0);
    assert!(events.iter().flatten().any(|e| matches!(e, SimEvent::Explosion { .. })), "a steam explosion");
    // The new block is hot, just below its melting point.
    let c = w.s.content().clone();
    let melt = c.materials.melt[c.expect_material("copper_block").index()].unwrap().at;
    let mut hot = false;
    for y in 60..120 {
        for x in 20..100 {
            let cell = w.s.cell(CellPos::new(x, y));
            if cell.material == c.expect_material("copper_block") {
                hot |= cell.temperature >= melt - 100;
            }
        }
    }
    assert!(hot, "a frozen block is still hot");
}

#[test]
fn wet_concrete_sets_after_about_30_seconds() {
    let mut w = World::new(2, 2);
    w.stone_box(20, 80, 100, 120);
    w.fill(20, 110, 100, 120, "wet_concrete", None);
    let total = w.count("wet_concrete");
    w.ticks(1200);
    assert!(w.count("concrete") < total / 20, "not yet after 20 seconds: {}", w.count("concrete"));
    w.ticks(1400);
    assert!(w.count("concrete") >= total - total / 20, "set after 43 seconds: {} of {total}", w.count("concrete"));
}

#[test]
fn smelting_puts_extra_slag_into_a_free_cell() {
    let mut w = World::without_heat_flow(2, 2);
    w.stone_box(20, 60, 100, 120);
    // Raw tin ore on charcoal, hot, with air above.
    w.fill(30, 110, 90, 120, "charcoal", Some(1000));
    w.fill(30, 105, 90, 110, "raw_cassiterite", Some(1000));
    w.ticks(200);
    assert!(w.count("molten_tin") > 10, "tin: {}", w.count("molten_tin"));
    assert!(w.count("molten_slag") > 0, "slag from the raw ore");
}

#[test]
fn brine_boils_into_steam_and_sometimes_salt() {
    let mut w = World::without_heat_flow(2, 2);
    w.stone_box(20, 40, 100, 120);
    w.fill(20, 90, 100, 120, "brine", Some(110));
    let total = w.count("brine");
    w.ticks(300);
    let (steam, salt) = (w.count("steam"), w.count("salt"));
    assert_eq!(w.count("brine"), 0, "all brine boiled");
    let share = salt as f32 / (steam + salt) as f32;
    assert!((0.22..0.38).contains(&share), "salt share {share} (steam {steam}, salt {salt}, brine {total})");
}

#[test]
fn dedupe_keeps_the_first_event_of_each_reaction() {
    let at = |x| CellPos::new(x, 0);
    let mut events = vec![
        SimEvent::Reaction { index: 3, at: at(1) },
        SimEvent::Explosion { at: at(2), strength: 1.0, heat: 0 },
        SimEvent::Reaction { index: 3, at: at(3) },
        SimEvent::Reaction { index: 70, at: at(4) },
        SimEvent::Reaction { index: 70, at: at(5) },
    ];
    dedupe_reaction_events(&mut events);
    assert_eq!(
        events,
        vec![
            SimEvent::Reaction { index: 3, at: at(1) },
            SimEvent::Explosion { at: at(2), strength: 1.0, heat: 0 },
            SimEvent::Reaction { index: 70, at: at(4) },
        ]
    );
}
