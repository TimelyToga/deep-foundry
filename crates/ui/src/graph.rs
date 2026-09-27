//! Time series for the power and production graphs, and the math to draw them.
//!
//! The game keeps one [`History`] for each value it wants to graph (for example the power
//! production of one network). It calls [`History::push`] once per simulation tick.
//! The UI reads the samples of the time range that the player picked.
//!
//! Each time range keeps [`SAMPLES`] samples. A sample of a longer range is the average of
//! several samples of the next shorter range, so the long ranges cost no extra work per tick.

/// Samples in the graph of each time range.
pub const SAMPLES: usize = 300;

/// The time ranges of the graphs, as in Factorio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TimeRange {
    #[default]
    Seconds5,
    Minutes1,
    Minutes10,
    Hours1,
    Hours10,
}

impl TimeRange {
    pub const ALL: [TimeRange; 5] =
        [TimeRange::Seconds5, TimeRange::Minutes1, TimeRange::Minutes10, TimeRange::Hours1, TimeRange::Hours10];

    pub fn index(self) -> usize {
        match self {
            TimeRange::Seconds5 => 0,
            TimeRange::Minutes1 => 1,
            TimeRange::Minutes10 => 2,
            TimeRange::Hours1 => 3,
            TimeRange::Hours10 => 4,
        }
    }

    /// Short label for the tab: "5s", "1m", "10m", "1h", "10h".
    pub fn label(self) -> &'static str {
        match self {
            TimeRange::Seconds5 => "5s",
            TimeRange::Minutes1 => "1m",
            TimeRange::Minutes10 => "10m",
            TimeRange::Hours1 => "1h",
            TimeRange::Hours10 => "10h",
        }
    }

    /// Length of the range in seconds.
    pub fn seconds(self) -> f64 {
        match self {
            TimeRange::Seconds5 => 5.0,
            TimeRange::Minutes1 => 60.0,
            TimeRange::Minutes10 => 600.0,
            TimeRange::Hours1 => 3600.0,
            TimeRange::Hours10 => 36000.0,
        }
    }

    /// How many samples of the next shorter range make one sample of this range.
    /// The 5 s range takes one sample per tick (60 ticks per second, 300 samples).
    pub fn samples_per_step(self) -> usize {
        match self {
            TimeRange::Seconds5 => 1,
            TimeRange::Minutes1 => 12,  // 12 ticks = 0.2 s
            TimeRange::Minutes10 => 10, // 2 s
            TimeRange::Hours1 => 6,     // 12 s
            TimeRange::Hours10 => 10,   // 120 s
        }
    }
}

/// Samples for each time range, oldest first. The UI draws these. Each list has at most
/// [`SAMPLES`] values. A list can be shorter (at the start of a game).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TimeSeries {
    pub ranges: [Vec<f32>; 5],
}

impl TimeSeries {
    pub fn samples(&self, range: TimeRange) -> &[f32] {
        &self.ranges[range.index()]
    }

    /// The average of the samples of a range. 0 if there are none.
    pub fn average(&self, range: TimeRange) -> f32 {
        let s = self.samples(range);
        if s.is_empty() { 0.0 } else { s.iter().sum::<f32>() / s.len() as f32 }
    }

    /// The largest sample of a range. 0 if there are none.
    pub fn max(&self, range: TimeRange) -> f32 {
        self.samples(range).iter().copied().fold(0.0, f32::max)
    }
}

/// A fixed-size ring of samples.
#[derive(Debug, Clone)]
struct Ring {
    data: Vec<f32>,
    start: usize,
}

impl Ring {
    fn new() -> Self {
        Self { data: Vec::with_capacity(SAMPLES), start: 0 }
    }

    fn push(&mut self, v: f32) {
        if self.data.len() < SAMPLES {
            self.data.push(v);
        } else {
            self.data[self.start] = v;
            self.start = (self.start + 1) % SAMPLES;
        }
    }

    /// Write the samples, oldest first, into `out` (which is cleared first).
    fn copy_into(&self, out: &mut Vec<f32>) {
        out.clear();
        out.extend_from_slice(&self.data[self.start..]);
        out.extend_from_slice(&self.data[..self.start]);
    }
}

/// Collects one value per tick and keeps the samples of all five time ranges.
/// The game uses it to fill [`TimeSeries`] for the UI.
#[derive(Debug, Clone)]
pub struct History {
    rings: [Ring; 5],
    /// Sum and count of the samples that wait to become one sample of the next range.
    pending: [(f64, usize); 5],
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

impl History {
    pub fn new() -> Self {
        Self { rings: std::array::from_fn(|_| Ring::new()), pending: [(0.0, 0); 5] }
    }

    /// Add the value of one tick.
    pub fn push(&mut self, value: f32) {
        let mut v = value as f64;
        for level in 0..5 {
            let step = TimeRange::ALL[level].samples_per_step();
            let (sum, count) = &mut self.pending[level];
            *sum += v;
            *count += 1;
            if *count < step {
                return;
            }
            v = *sum / *count as f64;
            *sum = 0.0;
            *count = 0;
            self.rings[level].push(v as f32);
        }
    }

    /// Copy the samples into `out`. Reuses the memory of `out`.
    pub fn fill(&self, out: &mut TimeSeries) {
        for (ring, list) in self.rings.iter().zip(out.ranges.iter_mut()) {
            ring.copy_into(list);
        }
    }

    pub fn to_series(&self) -> TimeSeries {
        let mut s = TimeSeries::default();
        self.fill(&mut s);
        s
    }
}

/// The smallest "round" number that is at least `v`: 1, 2, 2.5 or 5 times a power of 10.
/// The graph uses it as the top of the vertical axis. Returns 1 for 0 or less.
pub fn nice_ceiling(v: f64) -> f64 {
    if !v.is_finite() || v <= 0.0 {
        return 1.0;
    }
    let exp = v.log10().floor();
    let base = 10f64.powf(exp);
    for m in [1.0, 2.0, 2.5, 5.0, 10.0] {
        let candidate = m * base;
        // A small tolerance, so that 100.0000001 does not become 200 from rounding.
        if candidate >= v * (1.0 - 1e-9) {
            return candidate;
        }
    }
    10.0 * base
}

/// Positions of `samples` inside a rectangle given as (left, top, width, height).
/// The last sample is at the right edge. `max` is the value at the top edge.
/// Writes into `out` (cleared first). Values above `max` are clamped to the top.
pub fn graph_points(samples: &[f32], rect: (f32, f32, f32, f32), max: f64, out: &mut Vec<(f32, f32)>) {
    out.clear();
    let (left, top, width, height) = rect;
    if samples.is_empty() || max <= 0.0 {
        return;
    }
    // The x step is set by the full sample count, so a short list sits at the right side
    // (new data enters from the right, as in Factorio).
    let step = width / (SAMPLES.max(2) - 1) as f32;
    let first_x = left + width - step * (samples.len() - 1) as f32;
    for (i, &s) in samples.iter().enumerate() {
        let frac = (s as f64 / max).clamp(0.0, 1.0) as f32;
        out.push((first_x + step * i as f32, top + height * (1.0 - frac)));
    }
}

/// The satisfaction of a power network: how much of the demand the supply covers (0 to 1).
/// 1 if there is no demand.
pub fn satisfaction(supply_w: f64, demand_w: f64) -> f32 {
    if demand_w <= 0.0 { 1.0 } else { (supply_w / demand_w).clamp(0.0, 1.0) as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_ceiling_picks_round_numbers() {
        assert_eq!(nice_ceiling(0.0), 1.0);
        assert_eq!(nice_ceiling(-5.0), 1.0);
        assert_eq!(nice_ceiling(1.0), 1.0);
        assert_eq!(nice_ceiling(1.5), 2.0);
        assert_eq!(nice_ceiling(2.2), 2.5);
        assert_eq!(nice_ceiling(37.0), 50.0);
        assert_eq!(nice_ceiling(50.0), 50.0);
        assert_eq!(nice_ceiling(51.0), 100.0);
        assert_eq!(nice_ceiling(101.0), 200.0);
        assert_eq!(nice_ceiling(4200.0), 5000.0);
        assert_eq!(nice_ceiling(36_000.0), 50_000.0);
        assert_eq!(nice_ceiling(0.03), 0.05);
    }

    #[test]
    fn history_fills_short_range_each_tick() {
        let mut h = History::new();
        for i in 0..10 {
            h.push(i as f32);
        }
        let s = h.to_series();
        assert_eq!(s.samples(TimeRange::Seconds5), &[0., 1., 2., 3., 4., 5., 6., 7., 8., 9.]);
        assert!(s.samples(TimeRange::Minutes1).is_empty());
    }

    #[test]
    fn history_averages_into_longer_ranges() {
        let mut h = History::new();
        // 12 ticks make one sample of the 1 minute range.
        for i in 0..24 {
            h.push(if i < 12 { 10.0 } else { 20.0 });
        }
        let s = h.to_series();
        assert_eq!(s.samples(TimeRange::Minutes1), &[10.0, 20.0]);
        assert!(s.samples(TimeRange::Minutes10).is_empty());

        // 120 ticks make one sample of the 10 minute range.
        let mut h = History::new();
        for _ in 0..120 {
            h.push(6.0);
        }
        let s = h.to_series();
        assert_eq!(s.samples(TimeRange::Minutes10), &[6.0]);
    }

    #[test]
    fn history_ring_keeps_newest_samples_in_order() {
        let mut h = History::new();
        for i in 0..(SAMPLES + 5) {
            h.push(i as f32);
        }
        let s = h.to_series();
        let short = s.samples(TimeRange::Seconds5);
        assert_eq!(short.len(), SAMPLES);
        assert_eq!(short[0], 5.0);
        assert_eq!(*short.last().unwrap(), (SAMPLES + 4) as f32);
    }

    #[test]
    fn five_seconds_is_three_hundred_ticks() {
        // 300 samples at 60 ticks per second is 5 s. Check each range covers its length.
        let mut ticks = 1usize;
        for r in TimeRange::ALL {
            ticks *= r.samples_per_step();
            let seconds = (ticks * SAMPLES) as f64 / 60.0;
            assert_eq!(seconds, r.seconds(), "{r:?}");
        }
    }

    #[test]
    fn graph_points_put_newest_on_the_right() {
        let mut out = vec![];
        graph_points(&[0.0, 50.0, 100.0], (0.0, 0.0, 299.0, 100.0), 100.0, &mut out);
        assert_eq!(out.len(), 3);
        assert_eq!(out[2], (299.0, 0.0));
        assert_eq!(out[1], (298.0, 50.0));
        assert_eq!(out[0], (297.0, 100.0));
        // Values above the maximum stay inside the rectangle.
        graph_points(&[500.0], (0.0, 0.0, 10.0, 10.0), 100.0, &mut out);
        assert_eq!(out[0].1, 0.0);
    }

    #[test]
    fn satisfaction_is_clamped() {
        assert_eq!(satisfaction(0.0, 0.0), 1.0);
        assert_eq!(satisfaction(50.0, 100.0), 0.5);
        assert_eq!(satisfaction(150.0, 100.0), 1.0);
    }

    #[test]
    fn series_average_and_max() {
        let mut s = TimeSeries::default();
        s.ranges[0] = vec![1.0, 2.0, 3.0];
        assert_eq!(s.average(TimeRange::Seconds5), 2.0);
        assert_eq!(s.max(TimeRange::Seconds5), 3.0);
        assert_eq!(s.average(TimeRange::Hours1), 0.0);
    }
}
