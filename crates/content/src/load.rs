//! Loading and checking the data files.

use crate::defs::{MaterialDef, Phase, PhaseChange, ReactionDef};
use crate::factory::FactoryContent;
use crate::factory_defs::{BuildingDef, MilestoneDef, PartDef, RecipeDef, TechDef};
use crate::defs::REACTION_EVENTS;
use crate::table::{Alt, Burn, Change, MAX_BURN_GASES, Matcher, MaterialTable, OwnChange, Reaction, TagTable, Timer};
use crate::Content;
use foundry_core::{DEFAULT_TEMPERATURE, MaterialId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ContentError {
    #[error("cannot read {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("the data files have {} problem(s):\n{}", .0.len(), .0.join("\n"))]
    Invalid(Vec<String>),
}

/// Find `assets/`: the `FOUNDRY_ASSETS` variable, or the first folder with `assets/data`
/// found upward from the current folder or from the program's folder.
pub fn default_assets_dir() -> PathBuf {
    if let Ok(p) = std::env::var("FOUNDRY_ASSETS") {
        return PathBuf::from(p);
    }
    let mut starts = vec![];
    if let Ok(d) = std::env::current_dir() {
        starts.push(d);
    }
    if let Ok(e) = std::env::current_exe()
        && let Some(d) = e.parent() {
            starts.push(d.to_path_buf());
        }
    starts.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    for start in starts {
        for dir in start.ancestors() {
            let candidate = dir.join("assets");
            if candidate.join("data").is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from("assets")
}

impl Content {
    /// Load `<assets>/data/materials/*.ron` and `<assets>/data/reactions/*.ron` (files in name order).
    pub fn load(assets_dir: &Path) -> Result<Content, ContentError> {
        let data = assets_dir.join("data");
        let mut materials = vec![];
        for (path, text) in read_ron_files(&data.join("materials"))? {
            let list: Vec<MaterialDef> =
                ron::from_str(&text).map_err(|e| ContentError::Parse { path: path.clone(), message: e.to_string() })?;
            materials.extend(list);
        }
        let reactions: Vec<ReactionDef> = read_list(&data.join("reactions"))?;
        let mut content = Content::build(materials, reactions)?;
        let mut errors = vec![];
        content.factory = FactoryContent::build(
            &content.materials,
            read_list::<PartDef>(&data.join("parts"))?,
            read_list::<BuildingDef>(&data.join("buildings"))?,
            read_list::<RecipeDef>(&data.join("recipes"))?,
            read_list::<TechDef>(&data.join("tech"))?,
            read_list::<MilestoneDef>(&data.join("milestones"))?,
            &mut errors,
        );
        if errors.is_empty() { Ok(content) } else { Err(ContentError::Invalid(errors)) }
    }

    /// Load the default assets folder. See `default_assets_dir`.
    pub fn load_default() -> Result<Content, ContentError> {
        Content::load(&default_assets_dir())
    }

    /// Build content from RON text. Each string is a list of `Material(...)` or `Reaction(...)`. For tests.
    pub fn from_ron(materials: &[&str], reactions: &[&str]) -> Result<Content, ContentError> {
        let mut m = vec![];
        for (i, t) in materials.iter().enumerate() {
            let list: Vec<MaterialDef> = ron::from_str(t)
                .map_err(|e| ContentError::Parse { path: format!("<materials {i}>").into(), message: e.to_string() })?;
            m.extend(list);
        }
        let mut r = vec![];
        for (i, t) in reactions.iter().enumerate() {
            let list: Vec<ReactionDef> = ron::from_str(t)
                .map_err(|e| ContentError::Parse { path: format!("<reactions {i}>").into(), message: e.to_string() })?;
            r.extend(list);
        }
        Content::build(m, r)
    }

    /// Check the definitions and build the flat tables. Air always gets id 0.
    pub fn build(mut defs: Vec<MaterialDef>, reaction_defs: Vec<ReactionDef>) -> Result<Content, ContentError> {
        let mut errors = vec![];

        // Air first, then the file order.
        match defs.iter().position(|d| d.id == "air") {
            Some(i) => {
                let air = defs.remove(i);
                if air.phase != Phase::Empty {
                    errors.push("`air` must have phase Empty".to_string());
                }
                defs.insert(0, air);
            }
            None => errors.push("there is no material with id `air`".to_string()),
        }
        if defs.len() > u16::MAX as usize {
            errors.push("too many materials".to_string());
        }

        let mut by_id = HashMap::new();
        for (i, d) in defs.iter().enumerate() {
            if by_id.insert(d.id.clone(), MaterialId(i as u16)).is_some() {
                errors.push(format!("material id `{}` is used twice", d.id));
            }
            if d.id.is_empty() || !d.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
                errors.push(format!("material id `{}`: use only a-z, 0-9 and _", d.id));
            }
            if i > 0 && d.phase == Phase::Empty {
                errors.push(format!("`{}`: only air can have phase Empty", d.id));
            }
            if matches!(d.phase, Phase::Powder | Phase::Liquid | Phase::Gas) && d.density <= 0.0 {
                errors.push(format!("`{}`: a {:?} needs a density above 0", d.id, d.phase));
            }
            if d.phase == Phase::Liquid && d.flow == 0 {
                errors.push(format!("`{}`: a liquid needs a flow of 1 or more", d.id));
            }
            if [d.friction, d.conductivity, d.momentum, d.splash, d.viscosity].iter().any(|v| !(0.0..=1.0).contains(v)) {
                errors.push(format!("`{}`: friction, conductivity, momentum, splash and viscosity must be in 0..=1", d.id));
            }
            if d.heat_capacity <= 0.0 {
                errors.push(format!("`{}`: heat_capacity must be above 0", d.id));
            }
            if let Some((lo, hi)) = d.life
                && (lo > hi || hi == 0) {
                    errors.push(format!("`{}`: life must be (min, max) with 0 < max and min <= max", d.id));
                }
            if d.colors.is_empty() {
                errors.push(format!("`{}`: needs at least one color", d.id));
            }
            // The reaction code keeps the burning state, the charring time and the timer in the
            // cell's life byte, so these cannot be used together with `life`.
            if d.life.is_some() && (d.burn.is_some() || d.timer.is_some()) {
                errors.push(format!("`{}`: a material with `life` cannot have `burn` or `timer`", d.id));
            }
            if let Some(b) = &d.burn {
                if b.smoke.iter().count() + b.gases.len() > MAX_BURN_GASES {
                    errors.push(format!("`{}`: burn makes at most {MAX_BURN_GASES} gases (smoke included)", d.id));
                }
                if b.gases.iter().any(|(_, c)| !(0.0..=1.0).contains(c)) || !(0.0..=1.0).contains(&b.chance) {
                    errors.push(format!("`{}`: burn chances must be in 0..=1", d.id));
                }
                if b.char_into.is_some() && (b.char_ticks == 0 || d.timer.is_some()) {
                    errors.push(format!("`{}`: charring needs char_ticks above 0 and no timer", d.id));
                }
            }
            if let Some(t) = &d.timer
                && t.ticks == 0 {
                    errors.push(format!("`{}`: timer ticks must be above 0", d.id));
                }
            for c in &d.colors {
                if parse_color(c).is_none() {
                    errors.push(format!("`{}`: color `{c}` is not #rrggbb or #rrggbbaa", d.id));
                }
            }
        }

        // Tags and behaviors.
        let mut tags = TagTable::default();
        let mut behavior_names = vec![String::new()];
        for d in &defs {
            for t in &d.tags {
                if tags.bit(t).is_none() {
                    tags.names.push(t.clone());
                }
            }
            if let Some(b) = &d.behavior
                && !behavior_names.contains(b) {
                    behavior_names.push(b.clone());
                }
        }
        for r in &reaction_defs {
            for s in [&r.a, &r.b] {
                if let Some(t) = s.strip_prefix("tag:")
                    && tags.bit(t).is_none() {
                        errors.push(format!("reaction {} + {}: no material has tag `{t}`", r.a, r.b));
                    }
            }
        }
        if tags.names.len() > 64 {
            errors.push(format!("{} tags; at most 64 are allowed", tags.names.len()));
        }

        let resolve = |name: &str, owner: &str, errors: &mut Vec<String>| -> MaterialId {
            match by_id.get(name) {
                Some(&id) => id,
                None => {
                    errors.push(format!("`{owner}` refers to material `{name}`, which does not exist"));
                    MaterialId::AIR
                }
            }
        };
        let change = |c: &Option<PhaseChange>, owner: &str, errors: &mut Vec<String>| {
            c.as_ref().map(|c| Change { at: c.at, into: resolve(&c.into, owner, errors) })
        };

        let mut t = MaterialTable { behavior_names: behavior_names.clone(), ..Default::default() };
        for (i, d) in defs.iter().enumerate() {
            let own = MaterialId(i as u16);
            t.ids.push(d.id.clone());
            t.names.push(d.name.clone());
            t.phase.push(d.phase);
            t.density.push(if d.phase == Phase::Empty && d.density == 0.0 { 1.2 } else { d.density });
            t.flow.push(d.flow);
            t.momentum.push((d.momentum * 65535.0) as u16);
            t.splash.push(d.splash);
            t.viscosity.push(d.viscosity);
            t.friction.push(d.friction);
            t.grain.push(d.grain);
            t.hardness.push(d.hardness);
            t.heat_capacity.push(d.heat_capacity);
            t.conductivity.push(d.conductivity);
            t.temperature.push(d.temperature.unwrap_or(DEFAULT_TEMPERATURE));
            t.melt.push(change(&d.melt, &d.id, &mut errors));
            t.freeze.push(change(&d.freeze, &d.id, &mut errors));
            t.boil.push(change(&d.boil, &d.id, &mut errors));
            t.condense.push(change(&d.condense, &d.id, &mut errors));
            t.burn.push(d.burn.as_ref().map(|b| {
                let smoke = b.smoke.as_deref().map(|n| resolve(n, &d.id, &mut errors));
                let more: Vec<(MaterialId, f32)> = b.gases.iter().map(|(n, c)| (resolve(n, &d.id, &mut errors), *c)).collect();
                let mut gases = [None; MAX_BURN_GASES];
                for (slot, g) in gases.iter_mut().zip(smoke.map(|m| (m, b.smoke_chance)).into_iter().chain(more)) {
                    *slot = Some(g);
                }
                Burn {
                    ignite_at: b.ignite_at,
                    needs_air: b.needs_air,
                    fire_temp: b.fire_temp,
                    chance: b.chance,
                    into: b.into.as_deref().map_or(MaterialId::AIR, |n| resolve(n, &d.id, &mut errors)),
                    fire: resolve(b.fire.as_deref().unwrap_or("fire"), &d.id, &mut errors),
                    smoke,
                    smoke_chance: b.smoke_chance,
                    gases,
                    char_into: b.char_into.as_deref().map(|n| resolve(n, &d.id, &mut errors)),
                    char_ticks: b.char_ticks,
                }
            }));
            t.broken_into.push(d.broken_into.as_deref().map_or(own, |n| resolve(n, &d.id, &mut errors)));
            t.life.push(d.life);
            t.decay_into.push(d.decay_into.as_deref().map_or(MaterialId::AIR, |n| resolve(n, &d.id, &mut errors)));
            t.timer.push(d.timer.as_ref().map(|tm| Timer {
                ticks: tm.ticks,
                into: resolve(&tm.into, &d.id, &mut errors),
                needs_air: tm.needs_air,
            }));
            t.drag_limit.push(d.drag_limit);
            t.glow.push(d.glow);
            t.tags.push(d.tags.iter().filter_map(|n| tags.bit(n)).fold(0u64, |acc, b| acc | (1u64 << b)));
            t.behavior.push(d.behavior.as_ref().map_or(0, |b| behavior_names.iter().position(|n| n == b).unwrap() as u16));
            let colors: Vec<[u8; 4]> = d.colors.iter().filter_map(|c| parse_color(c)).collect();
            t.colors.push(if colors.is_empty() { vec![[255, 0, 255, 255]] } else { colors });
        }
        t.by_id = by_id.clone();

        let matcher = |s: &str, errors: &mut Vec<String>| -> Matcher {
            if s == "any" {
                Matcher::Any
            } else if let Some(tag) = s.strip_prefix("tag:") {
                Matcher::Tag(tags.bit(tag).unwrap_or(0))
            } else {
                Matcher::Material(resolve(s, "reaction", errors))
            }
        };
        let mut reactions = vec![];
        for r in &reaction_defs {
            let owner = format!("reaction {} + {}", r.a, r.b);
            if !(0.0..=1.0).contains(&r.chance) {
                errors.push(format!("{owner}: chance must be in 0..=1"));
            }
            if let Some(e) = &r.event
                && !REACTION_EVENTS.contains(&e.as_str()) {
                    errors.push(format!("{owner}: unknown event `{e}` (use one of {REACTION_EVENTS:?})"));
                }
            if r.extra.iter().any(|(_, c)| !(0.0..=1.0).contains(c)) || r.alt.as_ref().is_some_and(|a| !(0.0..=1.0).contains(&a.chance)) {
                errors.push(format!("{owner}: extra and alt chances must be in 0..=1"));
            }
            // A result is a material id, or a word such as "$freeze" (see `OwnChange`).
            let output = |s: &Option<String>, errors: &mut Vec<String>| -> (Option<MaterialId>, Option<OwnChange>) {
                match s.as_deref() {
                    None => (None, None),
                    Some(w) if w.starts_with('$') => match OwnChange::from_word(w) {
                        Some(o) => (None, Some(o)),
                        None => {
                            errors.push(format!("{owner}: unknown result word `{w}`"));
                            (None, None)
                        }
                    },
                    Some(n) => (Some(resolve(n, &owner, errors)), None),
                }
            };
            let (into_a, into_a_own) = output(&r.into_a, &mut errors);
            let (into_b, into_b_own) = output(&r.into_b, &mut errors);
            let alt = r.alt.as_ref().map(|a| Alt {
                chance: a.chance,
                into_a: a.into_a.as_deref().map(|n| resolve(n, &owner, &mut errors)),
                into_b: a.into_b.as_deref().map(|n| resolve(n, &owner, &mut errors)),
            });
            reactions.push(Reaction {
                a: matcher(&r.a, &mut errors),
                b: matcher(&r.b, &mut errors),
                chance: r.chance,
                min_temp: r.min_temp.unwrap_or(i16::MIN),
                max_temp: r.max_temp.unwrap_or(i16::MAX),
                into_a,
                into_b,
                into_a_own,
                into_b_own,
                heat: r.heat,
                needs_air: r.needs_air,
                event: r.event.clone(),
                extra: r.extra.iter().map(|(n, c)| (resolve(n, &owner, &mut errors), *c)).collect(),
                alt,
            });
        }
        if reactions.len() > u16::MAX as usize {
            errors.push("too many reactions".to_string());
        }

        if errors.is_empty() {
            Ok(Content { materials: t, reactions, tags, factory: FactoryContent::default() })
        } else {
            Err(ContentError::Invalid(errors))
        }
    }
}

/// Read every `.ron` file in a folder (in name order) as a list of `T`.
fn read_list<T: serde::de::DeserializeOwned>(dir: &Path) -> Result<Vec<T>, ContentError> {
    let mut out = vec![];
    for (path, text) in read_ron_files(dir)? {
        let list: Vec<T> = ron::from_str(&text).map_err(|e| ContentError::Parse { path: path.clone(), message: e.to_string() })?;
        out.extend(list);
    }
    Ok(out)
}

fn read_ron_files(dir: &Path) -> Result<Vec<(PathBuf, String)>, ContentError> {
    if !dir.is_dir() {
        return Ok(vec![]);
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|source| ContentError::Io { path: dir.to_path_buf(), source })?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "ron"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            std::fs::read_to_string(&p).map(|t| (p.clone(), t)).map_err(|source| ContentError::Io { path: p, source })
        })
        .collect()
}

/// Parse "#rrggbb" or "#rrggbbaa".
pub fn parse_color(s: &str) -> Option<[u8; 4]> {
    let h = s.strip_prefix('#')?;
    if !(h.len() == 6 || h.len() == 8) || !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?, if h.len() == 8 { byte(6)? } else { 255 }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_assets_load_and_air_is_zero() {
        let c = Content::load(&default_assets_dir()).expect("assets load");
        assert_eq!(c.material("air"), Some(MaterialId::AIR));
        assert!(c.materials.len() >= 6);
    }

    #[test]
    fn bad_reference_is_reported() {
        let err = Content::from_ron(
            &[r##"[Material(id: "air", name: "Air", phase: Empty, colors: ["#000000"]),
                  Material(id: "ice", name: "Ice", phase: Solid, colors: ["#ffffff"], melt: Some((at: 0, into: "wter")))]"##],
            &[],
        )
        .unwrap_err();
        assert!(err.to_string().contains("wter"), "{err}");
    }

    #[test]
    fn new_reaction_fields_load() {
        let mats = r##"[
            Material(id: "air", name: "Air", phase: Empty, colors: ["#000000"]),
            Material(id: "water", name: "Water", phase: Liquid, colors: ["#0000ff"], density: 1000, flow: 4),
            Material(id: "steam", name: "Steam", phase: Gas, colors: ["#ffffff"], density: 0.6),
            Material(id: "slag", name: "Slag", phase: Powder, colors: ["#555555"], density: 2000),
            Material(id: "block", name: "Block", phase: Solid, colors: ["#aaaaaa"], melt: Some((at: 1000, into: "molten"))),
            Material(id: "molten", name: "Molten", phase: Liquid, colors: ["#ff8800"], density: 7000, flow: 3,
                freeze: Some((at: 990, into: "block")), tags: ["metal"]),
            Material(id: "wood", name: "Wood", phase: Solid, colors: ["#885522"],
                burn: Some((ignite_at: 300, fire_temp: 800, smoke: Some("steam"), smoke_chance: 0.1,
                    gases: [("slag", 0.05)], char_into: Some("slag")))),
            Material(id: "wet", name: "Wet", phase: Liquid, colors: ["#888888"], density: 2000, flow: 1,
                timer: Some((ticks: 1800, into: "block"))),
            Material(id: "fire", name: "Fire", phase: Fire, colors: ["#ff0000"], density: 0.5, life: Some((5, 10))),
        ]"##;
        let reactions = r##"[
            Reaction(a: "tag:metal", b: "water", into_a: Some("$freeze"), into_b: Some("steam"), event: Some("explosion_small"),
                extra: [("slag", 0.5)], alt: Some((chance: 0.25, into_b: Some("water")))),
        ]"##;
        let c = Content::from_ron(&[mats], &[reactions]).expect("loads");
        let r = &c.reactions[0];
        assert_eq!(r.into_a, None);
        assert_eq!(r.into_a_own, Some(OwnChange::Freeze));
        assert_eq!(OwnChange::Freeze.of(c.expect_material("molten"), &c.materials), Some(c.expect_material("block")));
        assert_eq!(OwnChange::Freeze.of(c.expect_material("block"), &c.materials), None);
        assert_eq!(r.extra, vec![(c.expect_material("slag"), 0.5)]);
        assert_eq!(r.alt.unwrap().into_b, Some(c.expect_material("water")));
        let b = c.materials.burn[c.expect_material("wood").index()].unwrap();
        assert_eq!(b.gases[0], Some((c.expect_material("steam"), 0.1)));
        assert_eq!(b.gases[1], Some((c.expect_material("slag"), 0.05)));
        assert_eq!(b.gases[2], None);
        assert_eq!((b.char_into, b.char_ticks), (Some(c.expect_material("slag")), 600));
        let t = c.materials.timer[c.expect_material("wet").index()].unwrap();
        assert_eq!((t.ticks, t.into, t.needs_air), (1800, c.expect_material("block"), false));
    }

    #[test]
    fn bad_reaction_words_are_reported() {
        let mats = r##"[Material(id: "air", name: "Air", phase: Empty, colors: ["#000000"]),
                        Material(id: "fire", name: "Fire", phase: Fire, colors: ["#ff0000"], density: 0.5, life: Some((5, 10)),
                            burn: Some((ignite_at: 1, fire_temp: 1)))]"##;
        let err = Content::from_ron(&[mats], &[r#"[Reaction(a: "fire", b: "air", into_a: Some("$frieze"), event: Some("boom"))]"#])
            .unwrap_err()
            .to_string();
        for needle in ["$frieze", "boom", "cannot have `burn`"] {
            assert!(err.contains(needle), "missing `{needle}` in: {err}");
        }
    }

    #[test]
    fn colors_parse() {
        assert_eq!(parse_color("#ff8000"), Some([255, 128, 0, 255]));
        assert_eq!(parse_color("#ff800080"), Some([255, 128, 0, 128]));
        assert_eq!(parse_color("ff8000"), None);
    }
}

#[cfg(test)]
mod factory_tests {
    use super::*;
    use crate::factory::ItemRef;

    #[test]
    fn default_factory_content_loads() {
        let c = Content::load(&default_assets_dir()).expect("assets load");
        let f = &c.factory;
        let lab = f.building("basic_lab").expect("lab");
        let part = f.building_def(lab).part;
        assert_eq!(f.part_def(part).building, Some(lab), "each building is also a part");
        assert_eq!(c.item("basic_lab"), Some(ItemRef::Part(part)));
        assert!(matches!(c.item("clay"), Some(ItemRef::Material(_))));
        let alloy = f.recipe("bronze_alloy").unwrap();
        assert_eq!(f.recipe_def(alloy).unlocked_by, f.tech("bronze"));
        assert_eq!(f.recipe_def(f.recipe("workbench").unwrap()).unlocked_by, None);
        let bronze = c.item("molten_bronze").unwrap();
        assert!(f.recipes_making(bronze).any(|r| r == alloy));
        assert!(!f.milestones.is_empty());
    }

    #[test]
    fn factory_errors_are_reported() {
        let base = Content::load(&default_assets_dir()).unwrap();
        let mut errors = vec![];
        let parts: Vec<PartDef> = ron::from_str(r#"[Part(id: "sand", name: "Sand part", category: "intermediate")]"#).unwrap();
        let recipes: Vec<RecipeDef> =
            ron::from_str(r#"[Recipe(id: "x", category: "nowhere", inputs: [("unobtainium", 1)], outputs: [], time: 0.0)]"#).unwrap();
        let techs: Vec<TechDef> = ron::from_str(
            r#"[Tech(id: "a", name: "A", tier: 0, requires: ["b"]), Tech(id: "b", name: "B", tier: 0, requires: ["a"])]"#,
        )
        .unwrap();
        FactoryContent::build(&base.materials, parts, vec![], recipes, techs, vec![], &mut errors);
        let all = errors.join("\n");
        for needle in ["both a material and a part", "no building crafts category", "unobtainium", "at least one output", "time must be above 0", "loop"] {
            assert!(all.contains(needle), "missing error `{needle}` in:\n{all}");
        }
    }
}
