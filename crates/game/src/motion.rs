//! Smooth robot motion between ticks.
//!
//! The simulation makes a tick every 1/60 s. The window draws at the rate of the display (60 Hz,
//! 120 Hz, ...). The two clocks are not in step. On a 60 Hz display the new tick sometimes comes
//! just after a frame starts: that frame gets no new tick, and the next frame gets two. The old
//! code drew the robot from the arrival time of the newest tick, so the robot (and the camera,
//! which follows it) stood still for one frame and then jumped two ticks.
//!
//! `Track` draws the robot on a steady clock:
//! - It keeps the positions of the robot in the last few ticks.
//! - Tick N is due at `base + N × tick length`. A tick can come late (it waits for the next
//!   frame), but never early. So `base` is the smallest `arrival − N × tick length` of the last
//!   second.
//! - A frame at time `now` draws the robot at tick `(now − base) / tick length − DELAY_TICKS`,
//!   between the two known ticks around it.

use foundry_core::TICK_SECONDS;
use std::collections::VecDeque;
use std::time::Instant;

/// How many ticks the drawn robot is behind the clock. A tick that comes up to one tick length
/// late is still drawn in time.
const DELAY_TICKS: f64 = 1.25;
/// Robot positions to keep.
const POINTS: usize = 8;
/// Arrival times to keep for `base` (about one second).
const OFFSETS: usize = 60;
/// A tick this many ticks later than the clock says starts the clock again.
const RESTART_TICKS: f64 = 4.0;
/// A move longer than this (in cells) between two ticks is drawn at once, with no motion.
const JUMP: f32 = 16.0;

/// The robot positions of the last ticks and the clock of the ticks. See the module docs.
#[derive(Debug, Default)]
pub struct Track {
    /// Tick number and robot position (top left, in cells), oldest first.
    points: VecDeque<(u64, (f32, f32))>,
    /// `arrival − tick × tick length` of the last ticks, in seconds after `origin`.
    offsets: VecDeque<f64>,
    origin: Option<Instant>,
}

impl Track {
    /// A new tick arrived at `now` with the robot at `pos`.
    pub fn push(&mut self, tick: u64, pos: (f32, f32), now: Instant) {
        if let Some(&(last, last_pos)) = self.points.back() {
            if tick == last {
                return;
            }
            // Fewer ticks than before: a new world. A long move: the robot was put somewhere else.
            let far = (pos.0 - last_pos.0).abs() + (pos.1 - last_pos.1).abs() > JUMP;
            if tick < last || far {
                self.points.clear();
            }
            if tick < last {
                self.offsets.clear();
            }
        }
        let origin = *self.origin.get_or_insert(now);
        let offset = now.duration_since(origin).as_secs_f64() - tick as f64 * TICK_SECONDS;
        // Much later than the clock says (after a pause, or the simulation skipped time): the
        // clock starts again from this tick.
        if self.offsets.iter().copied().reduce(f64::min).is_some_and(|base| offset > base + RESTART_TICKS * TICK_SECONDS) {
            self.offsets.clear();
        }
        self.offsets.push_back(offset);
        if self.offsets.len() > OFFSETS {
            self.offsets.pop_front();
        }
        self.points.push_back((tick, pos));
        if self.points.len() > POINTS {
            self.points.pop_front();
        }
    }

    /// Where to draw the robot (top left, in cells) at `now`. `None` before the first tick.
    pub fn at(&self, now: Instant) -> Option<(f32, f32)> {
        let &(last_tick, last_pos) = self.points.back()?;
        let (Some(origin), Some(base)) = (self.origin, self.offsets.iter().copied().reduce(f64::min)) else {
            return Some(last_pos);
        };
        let tick = (now.saturating_duration_since(origin).as_secs_f64() - base) / TICK_SECONDS - DELAY_TICKS;
        if tick >= last_tick as f64 {
            return Some(last_pos);
        }
        let mut before = *self.points.front()?;
        for &(t, p) in &self.points {
            if t as f64 > tick {
                let k = ((tick - before.0 as f64) / (t - before.0).max(1) as f64).clamp(0.0, 1.0) as f32;
                let (a, b) = (before.1, p);
                return Some((a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k));
            }
            before = (t, p);
        }
        Some(last_pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Frames of a display at `hz` look at ticks that the simulation makes at 60 per second. The
    /// robot walks one cell per tick. Returns the number of frames in which the drawn robot moved
    /// much more or much less than the time of the frame says (a visible jerk), for the old way
    /// (from the arrival of the newest tick) and for `Track`.
    fn jerks(hz: f64, seconds: f64) -> (u32, u32) {
        let start = Instant::now();
        let at = |s: f64| start + Duration::from_secs_f64(s);
        let frame_len = 1.0 / hz;
        // Deterministic jitter of the publish time: 0 to 1 ms.
        let mut seed = 12345u64;
        let mut jitter = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as f64 / (1u64 << 31) as f64 * 0.001
        };
        let publish: Vec<f64> = (0..(seconds * 60.0) as u64 + 2).map(|n| 0.0002 + n as f64 * TICK_SECONDS + jitter()).collect();
        let (mut track, mut old) = (Track::default(), Old::default());
        let (mut next_tick, mut last) = (0usize, None::<(f64, f64)>);
        let (mut jerks_old, mut jerks_new) = (0u32, 0u32);
        let mut t = 0.004;
        while t < seconds {
            // The mailbox keeps only the newest tick.
            let mut newest = None;
            while publish[next_tick] <= t {
                newest = Some(next_tick as u64);
                next_tick += 1;
            }
            if let Some(n) = newest {
                track.push(n, (n as f32, 0.0), at(t));
                old.take(n, (n as f32, 0.0), at(t));
            }
            let (x_old, x_new) = (old.pos(at(t)).0 as f64, track.at(at(t)).unwrap().0 as f64);
            if let Some((lo, ln)) = last
                && t > 1.0
            {
                // One cell per tick: a frame should move the robot by `frame_len × 60` cells.
                let expected = frame_len * 60.0;
                jerks_old += (((x_old - lo) - expected).abs() > 0.5 * expected) as u32;
                jerks_new += (((x_new - ln) - expected).abs() > 0.5 * expected) as u32;
            }
            last = Some((x_old, x_new));
            t += frame_len;
        }
        (jerks_old, jerks_new)
    }

    /// The old way of `NormalMode::robot_pos`: from the tick before to the newest tick, over one
    /// tick length after the newest tick arrived.
    #[derive(Default)]
    struct Old {
        cur: Option<(f32, f32)>,
        prev: Option<(f32, f32)>,
        at: Option<Instant>,
        tick: u64,
    }

    impl Old {
        fn take(&mut self, tick: u64, pos: (f32, f32), now: Instant) {
            if tick != self.tick || self.at.is_none() {
                self.prev = self.cur;
                self.at = Some(now);
            }
            self.tick = tick;
            self.cur = Some(pos);
        }

        fn pos(&self, now: Instant) -> (f32, f32) {
            let cur = self.cur.unwrap();
            let (Some(prev), Some(at)) = (self.prev, self.at) else { return cur };
            let t = (now.saturating_duration_since(at).as_secs_f32() / TICK_SECONDS as f32).clamp(0.0, 1.0);
            (prev.0 + (cur.0 - prev.0) * t, prev.1 + (cur.1 - prev.1) * t)
        }
    }

    #[test]
    fn the_robot_moves_evenly_when_the_display_and_the_ticks_drift() {
        // A 60 Hz display whose clock is 0.1 % slow: the phase of frames and ticks moves across
        // a whole frame in 17 seconds.
        let (old, new) = jerks(59.94, 20.0);
        println!("59.94 Hz: jerks old {old}, new {new}");
        assert!(old > 20, "the old way jerks near the phase where ticks and frames meet: {old}");
        assert!(new <= 2, "{new} jerks");
        for hz in [60.0, 60.05, 120.0, 144.0] {
            let (old, new) = jerks(hz, 20.0);
            println!("{hz} Hz: jerks old {old}, new {new}");
            assert!(new <= 2, "{hz} Hz: {new} jerks");
        }
    }

    #[test]
    fn new_world_and_long_moves_start_again() {
        let now = Instant::now();
        let mut t = Track::default();
        assert_eq!(t.at(now), None);
        t.push(10, (5.0, 5.0), now);
        assert_eq!(t.at(now), Some((5.0, 5.0)));
        // A long move is drawn at once.
        t.push(11, (500.0, 5.0), now + Duration::from_millis(16));
        assert_eq!(t.at(now + Duration::from_millis(17)), Some((500.0, 5.0)));
        // A new world starts at tick 0.
        t.push(0, (7.0, 7.0), now + Duration::from_millis(40));
        assert_eq!(t.at(now + Duration::from_millis(41)), Some((7.0, 7.0)));
    }

    #[test]
    fn after_a_pause_the_robot_moves_smoothly_again_at_once() {
        let start = Instant::now();
        let at = |ms: f64| start + Duration::from_secs_f64(ms / 1000.0);
        let tick_ms = TICK_SECONDS * 1000.0;
        let mut t = Track::default();
        for n in 0..10 {
            t.push(n, (n as f32, 0.0), at(n as f64 * tick_ms + 1.0));
        }
        // A pause of 5 seconds, then ticks 10, 11, 12 come on time again.
        let resume = 5000.0;
        for n in 10..13 {
            t.push(n, (n as f32, 0.0), at(resume + (n - 10) as f64 * tick_ms + 1.0));
        }
        // Half a tick after tick 12 came, the robot is between ticks 10 and 11 (not at 12).
        let x = t.at(at(resume + 2.5 * tick_ms + 1.0)).unwrap().0;
        assert!((10.0..11.5).contains(&x), "{x}");
    }
}
