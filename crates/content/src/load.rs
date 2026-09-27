//! Loading and checking the data files.

use crate::defs::{MaterialDef, Phase, PhaseChange, ReactionDef};
use crate::table::{Burn, Change, Matcher, MaterialTable, Reaction, TagTable};
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
        let mut reactions = vec![];
        for (path, text) in read_ron_files(&data.join("reactions"))? {
            let list: Vec<ReactionDef> =
                ron::from_str(&text).map_err(|e| ContentError::Parse { path: path.clone(), message: e.to_string() })?;
            reactions.extend(list);
        }
        Content::build(materials, reactions)
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
            if !(0.0..=1.0).contains(&d.friction) || !(0.0..=1.0).contains(&d.conductivity) {
                errors.push(format!("`{}`: friction and conductivity must be in 0..=1", d.id));
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
            t.burn.push(d.burn.as_ref().map(|b| Burn {
                ignite_at: b.ignite_at,
                needs_air: b.needs_air,
                fire_temp: b.fire_temp,
                chance: b.chance,
                into: b.into.as_deref().map_or(MaterialId::AIR, |n| resolve(n, &d.id, &mut errors)),
                fire: resolve(b.fire.as_deref().unwrap_or("fire"), &d.id, &mut errors),
                smoke: b.smoke.as_deref().map(|n| resolve(n, &d.id, &mut errors)),
                smoke_chance: b.smoke_chance,
            }));
            t.broken_into.push(d.broken_into.as_deref().map_or(own, |n| resolve(n, &d.id, &mut errors)));
            t.life.push(d.life);
            t.decay_into.push(d.decay_into.as_deref().map_or(MaterialId::AIR, |n| resolve(n, &d.id, &mut errors)));
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
            if !(0.0..=1.0).contains(&r.chance) {
                errors.push(format!("reaction {} + {}: chance must be in 0..=1", r.a, r.b));
            }
            let owner = format!("reaction {} + {}", r.a, r.b);
            reactions.push(Reaction {
                a: matcher(&r.a, &mut errors),
                b: matcher(&r.b, &mut errors),
                chance: r.chance,
                min_temp: r.min_temp.unwrap_or(i16::MIN),
                max_temp: r.max_temp.unwrap_or(i16::MAX),
                into_a: r.into_a.as_deref().map(|n| resolve(n, &owner, &mut errors)),
                into_b: r.into_b.as_deref().map(|n| resolve(n, &owner, &mut errors)),
                heat: r.heat,
                needs_air: r.needs_air,
                event: r.event.clone(),
            });
        }

        if errors.is_empty() { Ok(Content { materials: t, reactions, tags }) } else { Err(ContentError::Invalid(errors)) }
    }
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
    fn colors_parse() {
        assert_eq!(parse_color("#ff8000"), Some([255, 128, 0, 255]));
        assert_eq!(parse_color("#ff800080"), Some([255, 128, 0, 128]));
        assert_eq!(parse_color("ff8000"), None);
    }
}
