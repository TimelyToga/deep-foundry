//! Checks: conditions that a scene test tests after its ticks.
//!
//! Positions and rectangles in checks are in image pixels: (0, 0) is the top-left pixel of the scene PNG.
//! Negative values and values past the image size are allowed. They reach into the world around the image.

use crate::scene::{Point, Rect};
use foundry_content::Content;
use foundry_core::{CellPos, CellRect, MaterialId};
use foundry_sim::Simulation;
use serde::Deserialize;

/// How the `Temperature` check uses the cells it finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum TempOf {
    /// The mean temperature of the cells must be in the range.
    #[default]
    Mean,
    /// Every cell must be in the range.
    Each,
}

/// One check. In RON: `Count(material: "sand", rect: (x: 0, y: 40, w: 96, h: 24), min: 200)`.
///
/// Every check has `pending: true` or `false` (default `false`). A pending check is run and reported,
/// but its result does not fail the test. Use it for behavior that is not built yet.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Check {
    /// The number of cells of a material in a rectangle of the image.
    Count {
        material: String,
        rect: Rect,
        #[serde(default)]
        exact: Option<u64>,
        #[serde(default)]
        min: Option<u64>,
        #[serde(default)]
        max: Option<u64>,
        #[serde(default)]
        pending: bool,
    },
    /// The number of cells of a material in the whole world.
    Total {
        material: String,
        #[serde(default)]
        exact: Option<u64>,
        #[serde(default)]
        min: Option<u64>,
        #[serde(default)]
        max: Option<u64>,
        #[serde(default)]
        pending: bool,
    },
    /// The world has the same number of cells of a material at the end as at the start (nothing is lost).
    TotalUnchanged {
        material: String,
        #[serde(default)]
        pending: bool,
    },
    /// The cell at an image pixel has this material.
    CellIs {
        at: Point,
        material: String,
        #[serde(default)]
        pending: bool,
    },
    /// The temperature of the cells of a material, or of the cells in a rectangle, or of the cells
    /// of a material in a rectangle. Without `rect`: the whole world. `of: Mean` (default) or `of: Each`.
    Temperature {
        #[serde(default)]
        material: Option<String>,
        #[serde(default)]
        rect: Option<Rect>,
        #[serde(default)]
        of: TempOf,
        #[serde(default)]
        min: Option<i16>,
        #[serde(default)]
        max: Option<i16>,
        #[serde(default)]
        pending: bool,
    },
    /// The world hash did not change in the last `last_ticks` ticks.
    NoChange {
        last_ticks: u32,
        #[serde(default)]
        pending: bool,
    },
    /// Run the scene a second time with the same seed on a single thread.
    /// The world hash must be the same every `every` ticks and at the end.
    Deterministic {
        #[serde(default = "default_every")]
        every: u32,
        #[serde(default)]
        pending: bool,
    },
}

fn default_every() -> u32 {
    100
}

/// What a scene run recorded. The checks read it.
pub struct RunData<'a> {
    /// The simulation after the last tick.
    pub sim: &'a Simulation,
    /// The world position of image pixel (0, 0).
    pub origin: CellPos,
    /// Ticks that ran.
    pub ticks: u32,
    /// Cells of each material in the world before the first tick. Index: material id.
    pub start_totals: &'a [u64],
    /// Cells of each material in the world after the last tick. Index: material id.
    pub end_totals: &'a [u64],
    /// (tick, world hash) for the last ticks, oldest first. Used by `NoChange`.
    pub last_hashes: &'a [(u32, u64)],
    /// (tick, world hash) of the first and the second run. Used by `Deterministic`.
    pub first_run: &'a [(u32, u64)],
    pub second_run: &'a [(u32, u64)],
}

impl Check {
    pub fn pending(&self) -> bool {
        match self {
            Check::Count { pending, .. }
            | Check::Total { pending, .. }
            | Check::TotalUnchanged { pending, .. }
            | Check::CellIs { pending, .. }
            | Check::Temperature { pending, .. }
            | Check::NoChange { pending, .. }
            | Check::Deterministic { pending, .. } => *pending,
        }
    }

    /// A short text that says what the check tests.
    pub fn describe(&self) -> String {
        match self {
            Check::Count { material, rect, .. } => format!("count {material} in {rect}"),
            Check::Total { material, .. } => format!("total {material}"),
            Check::TotalUnchanged { material, .. } => format!("total {material} unchanged"),
            Check::CellIs { at, material, .. } => format!("cell {at} is {material}"),
            Check::Temperature { material, rect, of, .. } => {
                let what = material.as_deref().unwrap_or("cells");
                let place = rect.map_or("in world".to_string(), |r| format!("in {r}"));
                match of {
                    TempOf::Mean => format!("mean temperature of {what} {place}"),
                    TempOf::Each => format!("temperature of each of {what} {place}"),
                }
            }
            Check::NoChange { last_ticks, .. } => format!("no change in last {last_ticks} ticks"),
            Check::Deterministic { every, .. } => format!("same result twice (every {every} ticks)"),
        }
    }

    /// Check the fields: materials exist, ranges make sense.
    pub fn validate(&self, content: &Content) -> Result<(), String> {
        let known = |m: &str| {
            if content.material(m).is_some() { Ok(()) } else { Err(format!("unknown material `{m}`")) }
        };
        let rect_ok = |r: &Rect| {
            if r.w > 0 && r.h > 0 { Ok(()) } else { Err(format!("rect {r} is empty")) }
        };
        match self {
            Check::Count { material, rect, exact, min, max, .. } => {
                known(material)?;
                rect_ok(rect)?;
                count_bounds_ok(*exact, *min, *max)
            }
            Check::Total { material, exact, min, max, .. } => {
                known(material)?;
                count_bounds_ok(*exact, *min, *max)
            }
            Check::TotalUnchanged { material, .. } | Check::CellIs { material, .. } => known(material),
            Check::Temperature { material, rect, min, max, .. } => {
                if let Some(m) = material {
                    known(m)?;
                }
                if let Some(r) = rect {
                    rect_ok(r)?;
                }
                match (min, max) {
                    (None, None) => Err("give min, max, or both".into()),
                    (Some(a), Some(b)) if a > b => Err(format!("min {a} is above max {b}")),
                    _ => Ok(()),
                }
            }
            Check::NoChange { last_ticks, .. } => {
                if *last_ticks == 0 { Err("last_ticks must be 1 or more".into()) } else { Ok(()) }
            }
            Check::Deterministic { every, .. } => {
                if *every == 0 { Err("every must be 1 or more".into()) } else { Ok(()) }
            }
        }
    }

    /// Run the check. Returns (passed, detail). The detail says what was found and what was wanted.
    pub fn evaluate(&self, run: &RunData, content: &Content) -> (bool, String) {
        let id = |m: &str| content.expect_material(m);
        let world_rect = |r: &Rect| {
            CellRect::new(run.origin.x + r.x, run.origin.y + r.y, run.origin.x + r.x + r.w, run.origin.y + r.y + r.h)
        };
        match self {
            Check::Count { material, rect, exact, min, max, .. } => {
                let n = run.sim.count_material(world_rect(rect), id(material)) as u64;
                in_count_range(n, *exact, *min, *max)
            }
            Check::Total { material, exact, min, max, .. } => {
                let n = run.end_totals[id(material).index()];
                in_count_range(n, *exact, *min, *max)
            }
            Check::TotalUnchanged { material, .. } => {
                let i = id(material).index();
                let (a, b) = (run.start_totals[i], run.end_totals[i]);
                (a == b, format!("start {a}, end {b}"))
            }
            Check::CellIs { at, material, .. } => {
                let found = run.sim.cell(run.origin.offset(at.x, at.y)).material;
                let name = &content.materials.ids[found.index()];
                (found == id(material), format!("found {name}"))
            }
            Check::Temperature { material, rect, of, min, max, .. } => {
                let (w, h) = run.sim.size_cells();
                let area = rect.as_ref().map_or(CellRect::new(0, 0, w, h), world_rect);
                let filter = material.as_deref().map(id);
                temperature(run.sim, area, filter, *of, *min, *max)
            }
            Check::NoChange { last_ticks, .. } => {
                let need = *last_ticks as usize + 1;
                if run.last_hashes.len() < need {
                    return (false, format!("the scene ran only {} ticks", run.ticks));
                }
                let recent = &run.last_hashes[run.last_hashes.len() - need..];
                match recent.iter().find(|(_, h)| *h != recent[0].1) {
                    None => (true, format!("stable from tick {}", recent[0].0)),
                    Some((t, _)) => (false, format!("changed at tick {t}")),
                }
            }
            Check::Deterministic { every, .. } => {
                let wanted = |t: u32| t.is_multiple_of(*every) || t == run.ticks;
                let a = run.first_run.iter().filter(|(t, _)| wanted(*t));
                let b = run.second_run.iter().filter(|(t, _)| wanted(*t));
                let mut compared = 0;
                for (x, y) in a.zip(b) {
                    if x != y {
                        return (false, format!("hashes differ at tick {} ({:016x} vs {:016x})", x.0, x.1, y.1));
                    }
                    compared += 1;
                }
                (compared > 0, format!("{compared} hashes equal"))
            }
        }
    }
}

fn count_bounds_ok(exact: Option<u64>, min: Option<u64>, max: Option<u64>) -> Result<(), String> {
    match (exact, min, max) {
        (None, None, None) => Err("give exact, min, max, or min and max".into()),
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => Err("give exact, or min/max, not both".into()),
        (_, Some(a), Some(b)) if a > b => Err(format!("min {a} is above max {b}")),
        _ => Ok(()),
    }
}

fn in_count_range(n: u64, exact: Option<u64>, min: Option<u64>, max: Option<u64>) -> (bool, String) {
    let ok = exact.is_none_or(|e| n == e) && min.is_none_or(|m| n >= m) && max.is_none_or(|m| n <= m);
    (ok, format!("got {n}, want {}", range_text(exact, min, max)))
}

fn range_text<T: std::fmt::Display>(exact: Option<T>, min: Option<T>, max: Option<T>) -> String {
    match (exact, min, max) {
        (Some(e), _, _) => format!("{e}"),
        (None, Some(a), Some(b)) => format!("{a}..={b}"),
        (None, Some(a), None) => format!(">= {a}"),
        (None, None, Some(b)) => format!("<= {b}"),
        (None, None, None) => "anything".into(),
    }
}

fn temperature(
    sim: &Simulation,
    area: CellRect,
    material: Option<MaterialId>,
    of: TempOf,
    min: Option<i16>,
    max: Option<i16>,
) -> (bool, String) {
    let (mut n, mut sum, mut lo, mut hi) = (0u64, 0i64, i16::MAX, i16::MIN);
    for y in area.y0..area.y1 {
        for x in area.x0..area.x1 {
            let c = sim.cell(CellPos::new(x, y));
            if material.is_some_and(|m| m != c.material) {
                continue;
            }
            n += 1;
            sum += c.temperature as i64;
            lo = lo.min(c.temperature);
            hi = hi.max(c.temperature);
        }
    }
    let want = range_text(None, min, max);
    if n == 0 {
        return (false, "no cells found".into());
    }
    let above_min = |t: i16| min.is_none_or(|m| t >= m);
    let below_max = |t: i16| max.is_none_or(|m| t <= m);
    match of {
        TempOf::Mean => {
            let mean = sum as f64 / n as f64;
            let ok = min.is_none_or(|m| mean >= m as f64) && max.is_none_or(|m| mean <= m as f64);
            (ok, format!("mean {mean:.1} of {n} cells, want {want}"))
        }
        TempOf::Each => (above_min(lo) && below_max(hi), format!("{lo}..={hi} over {n} cells, want {want}")),
    }
}
