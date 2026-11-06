//! [re:29dbfb] (constructor [re:3b06c3], tap [re:bf2f59], tempo
//! [re:b2ec19]), driven by `e_tapTempo` 0x1b.
//!
//! - Starts at 120 BPM with its stopwatch running.
//! - Each tap measures the time since the previous one (seconds): 5 s or
//!   more clears the intervals, otherwise the interval is appended; then
//!   the stopwatch restarts.
//! - With at least 3 intervals the tempo becomes 60 / mean interval
//!   (written as (60000 / (mean·1000·0.25))·0.25) and the tap reports a
//!   new tempo; when the intervals sum to more than 2 s the newest one is
//!   dropped again.

use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TapTempo {
    tempo: f32,
    last: Instant,
    intervals: Vec<f64>,
}

impl Default for TapTempo {
    fn default() -> Self {
        TapTempo { tempo: 120.0, last: Instant::now(), intervals: Vec::with_capacity(16) }
    }
}

impl TapTempo {
    /// One tap at `now`; true when a tempo is available.
    pub fn tap(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tap_after(elapsed)
    }

    fn tap_after(&mut self, elapsed: f64) -> bool {
        if elapsed >= 5.0 {
            self.intervals.clear();
        } else {
            self.intervals.push(elapsed);
        }
        let n = self.intervals.len();
        if n < 3 {
            return false;
        }
        let sum: f64 = self.intervals.iter().sum();
        let mean = sum / n as f64;
        self.tempo = ((60000.0 / (mean * 1000.0 * 0.25)) * 0.25) as f32;
        if sum > 2.0 {
            self.intervals.pop();
        }
        true
    }

    pub fn tempo(&self) -> f32 {
        self.tempo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_three_intervals_then_averages() {
        let mut t = TapTempo::default();
        assert!(!t.tap_after(0.5));
        assert!(!t.tap_after(0.5));
        assert!(t.tap_after(0.5));
        assert!((t.tempo() - 120.0).abs() < 1e-3);
        // Sum 1.5 s: nothing dropped; a fourth 0.25 s tap → mean 0.4375.
        assert!(t.tap_after(0.25));
        assert!((t.tempo() - 60.0 / 0.4375).abs() < 1e-2);
        // Sum 1.75 + 0.6 > 2: the newest interval is dropped after use.
        assert!(t.tap_after(0.6));
        assert_eq!(t.intervals.len(), 4);
        // A pause of 5 s or more starts over.
        assert!(!t.tap_after(5.0));
        assert!(t.intervals.is_empty());
    }
}
