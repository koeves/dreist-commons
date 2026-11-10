//! Drag and wheel arithmetic of the second-generation stock controls
//! (opt-in with [`crate::app::App::v2_controls`]).
//!
//! - Knobs ([re:5a71e0]): each move adds the pointer's travel since the
//!   last move, `dy / realMacroValueScalar` (default 200 px a turn), or
//!   `/ realMicroValueScaler` (2000) with shift alone; clamped at every
//!   move, so turning back after overshooting an end responds at once.
//! - Transmod knobs ([re:db47c7], [re:4d1c73]): the travel since the press
//!   (re-anchored when the modifiers change) at the same rates; a depth
//!   drag moves the modulation end (base + depth, kept in 0..1) and, unless
//!   shift alone is held, snaps it onto the base over a 0.1 wide zone
//!   ([`snap_to`]).
//! - Sliders ([re:8c03b2], [re:e0f4a6]): the travel since the press over the
//!   track minus the thumb, 20 times slower with shift alone
//!   (`realMacroValueScalar · 0.1`), re-anchored when the modifiers change;
//!   a plain slider pressed off its thumb jumps there first
//!   ([re:1b8f6c]).
//! - Wheel ([re:3a9d20]): every wheel event is one step of ±1 whatever its
//!   size (the horizontal delta first). Transmod knobs move 1/150 a step
//!   (1/300 with shift, [re:c7e2d4]); plain knobs take the wheel only while
//!   focused, which no Dreist knob ever is, and sliders never (both pass it
//!   on to their parents, [re:7b30e1] [re:2f61aa]).

use crate::app::Mods;

/// Shift alone (no command, option or control): the fine drag modifier.
pub fn fine(m: Mods) -> bool {
    m.shift && !m.cmd && !m.alt && !m.ctrl
}

/// Pixels a knob travels for a full turn: `realMicroValueScaler` with
/// shift alone, else `realMacroValueScalar`.
pub fn knob_range(m: Mods, macro_px: f32, micro_px: f32) -> f32 {
    if fine(m) { micro_px } else { macro_px }
}

/// A slider's pixel divisor: `realMacroValueScalar · 0.1` with shift alone.
pub fn slider_divisor(m: Mods, macro_px: f32) -> f32 {
    if fine(m) { macro_px * 0.1 } else { 1.0 }
}

/// Thumb size of a plain slider along its track (`realSliderPortion` ×
/// the cross size, [re:6e0b3d]).
pub fn slider_thumb(w: i32, h: i32, portion: f32) -> i32 {
    ((if w < h { w } else { h }) as f32 * portion) as i32
}

/// Thumb height of a transmod slider: the main track's width
/// (`w − w·realModDim`) × `realSliderPortion`.
pub fn transmod_slider_thumb(w: i32, mod_dim: f32, portion: f32) -> i32 {
    ((w as f32 - w as f32 * mod_dim) * portion) as i32
}

/// Value of a plain slider pressed at local (`x`, `y`) ([re:1b8f6c]).
pub fn slider_point(w: i32, h: i32, x: f32, y: f32) -> f32 {
    if w < h { 1.0 - (y / h.max(1) as f32).clamp(0.0, 1.0) } else { (x / w.max(1) as f32).clamp(0.0, 1.0) }
}

/// Whether local (`x`, `y`) is on a plain slider's thumb at `v`.
pub fn on_slider_thumb(w: i32, h: i32, portion: f32, v: f32, x: f32, y: f32) -> bool {
    let t = slider_thumb(w, h, portion);
    let (tx, ty, tw, th) = if w < h { (0, ((1.0 - v) * (h - t) as f32) as i32, w, t) } else { (((w - t) as f32 * v) as i32, 0, t, h) };
    let (x, y) = (x.floor() as i32, y.floor() as i32);
    y >= ty && y < ty + th && x >= tx && x < tx + tw
}

/// A transmod knob value dragged `d` from `start` while the other part
/// sits at `other`: unless `fine`, the value sticks to `other` over a
/// 0.1 wide zone and lags the pointer by 0.1 past it ([re:db47c7]).
pub fn snap_to(start: f32, d: f32, other: f32, fine: bool) -> f32 {
    if fine {
        return (start + d).clamp(0.0, 1.0);
    }
    let mut v = d + start + if other < start { 0.05 } else { -0.05 };
    if v <= other {
        v += 0.05;
        if other < v {
            v = other;
        }
    } else {
        v -= 0.05;
        if v < other {
            v = other;
        }
    }
    v.clamp(0.0, 1.0)
}

/// The step of a wheel event (`dx`, `dy` as reported): +1 / −1 by the
/// sign of the horizontal delta, else of the vertical one; 0 for none.
pub fn wheel_step(dx: f32, dy: f32) -> f32 {
    let d = if dx.abs() >= 1e-6 { dx } else { dy };
    if d.abs() < 1e-6 {
        0.0
    } else if d < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// A transmod control's wheel divisor: 150 per step, 300 with shift.
pub fn transmod_wheel(m: Mods) -> f32 {
    if m.shift { 300.0 } else { 150.0 }
}

/// The depth (shown as `(depth + 1) / 2`) after moving the modulation end
/// of a control at `base` from `depth0` by `d` (end kept in 0..1).
pub fn depth_after(base: f32, depth0: f32, d: f32, snap: Option<bool>) -> f32 {
    let end0 = base + depth0;
    let end = match snap {
        Some(fine) => snap_to(end0, d, base, fine),
        None => (end0 + d).clamp(0.0, 1.0),
    };
    end - base
}

/// Tap tempo ([re:71d2a8]): intervals under 5 s gather (a longer gap
/// starts again); from the fourth tap on the tempo is 60 / their mean, and
/// while they add up to more than 2 s the newest is dropped again.
#[derive(Clone, Debug, Default)]
pub struct TapTempo {
    last: Option<std::time::Instant>,
    intervals: Vec<f64>,
}

impl TapTempo {
    /// A tap at `now`: the new tempo once there are enough taps.
    pub fn tap(&mut self, now: std::time::Instant) -> Option<f32> {
        if let Some(t) = self.last {
            let dt = now.duration_since(t).as_secs_f64();
            if dt >= 5.0 {
                self.intervals.clear();
            } else {
                self.intervals.push(dt);
            }
        }
        self.last = Some(now);
        if self.intervals.len() < 3 {
            return None;
        }
        let sum: f64 = self.intervals.iter().sum();
        let mean = sum / self.intervals.len() as f64;
        let bpm = (60000.0 / (mean * 1000.0 * 0.25) * 0.25) as f32;
        if sum > 2.0 {
            self.intervals.pop();
        }
        Some(bpm)
    }

    /// Forget the taps (a plain press or an edit, [re:71d2a8]).
    pub fn reset(&mut self) {
        self.last = None;
        self.intervals.clear();
    }
}

/// Tempo box range ([re:b0c3e9]).
pub const TEMPO_RANGE: (f32, f32) = (20.0, 250.0);

/// A tempo box dragged `d` (value units, up positive): on the whole part
/// (left half) the tempo moves over the range keeping its fraction; on the
/// fraction (right half) `frac` (0..1, kept by the box) moves and the
/// tempo becomes `⌊tempo⌋ + frac · 0.99` ([re:b0c3e9]). Returns the tempo
/// and the new `frac`.
pub fn tempo_drag(tempo: f32, frac: f32, whole: bool, d: f32) -> (f32, f32) {
    let (lo, hi) = TEMPO_RANGE;
    let (t, frac) = if whole {
        let t01 = ((tempo - lo) / (hi - lo) + d).clamp(0.0, 1.0);
        ((t01 * (hi - lo) + lo).floor() + (tempo - tempo.floor()), frac)
    } else {
        let f = (frac + d).clamp(0.0, 1.0);
        (tempo.floor() + f * 0.99, f)
    };
    (t.clamp(lo, hi), frac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fine_needs_shift_alone() {
        let shift = Mods { shift: true, ..Mods::default() };
        assert!(fine(shift));
        assert!(!fine(Mods { cmd: true, ..shift }));
        assert!(!fine(Mods { alt: true, ..shift }));
        assert!(!fine(Mods { ctrl: true, ..shift }));
        assert_eq!(knob_range(shift, 200.0, 2000.0), 2000.0);
        assert_eq!(knob_range(Mods::default(), 200.0, 2000.0), 200.0);
        assert_eq!(slider_divisor(shift, 200.0), 20.0);
    }

    #[test]
    fn depth_snaps_onto_the_base() {
        // From depth 0, upwards: nothing until 0.1, then 0.1 behind.
        assert_eq!(depth_after(0.5, 0.0, 0.08, Some(false)), 0.0);
        assert!((depth_after(0.5, 0.0, 0.2, Some(false)) - 0.1).abs() < 1e-6);
        // Downwards it moves at once (the original's asymmetry).
        assert!((depth_after(0.5, 0.0, -0.08, Some(false)) + 0.08).abs() < 1e-6);
        // Shift alone: no snapping.
        assert!((depth_after(0.5, 0.0, 0.08, Some(true)) - 0.08).abs() < 1e-6);
        // From a positive depth, no jump at the press; crossing the base
        // sticks to it.
        assert!((depth_after(0.5, 0.2, 0.0, Some(false)) - 0.2).abs() < 1e-6);
        assert_eq!(depth_after(0.5, 0.2, -0.25, Some(false)), 0.0);
        // The modulation end stays in 0..1.
        assert!((depth_after(0.8, 0.0, 0.5, None) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn wheel_steps_are_unit_sized() {
        assert_eq!(wheel_step(0.0, 3.5), 1.0);
        assert_eq!(wheel_step(0.0, -0.2), -1.0);
        assert_eq!(wheel_step(-2.0, 5.0), -1.0);
        assert_eq!(wheel_step(0.0, 0.0), 0.0);
    }

    #[test]
    fn slider_thumbs_and_points() {
        assert_eq!(slider_thumb(20, 100, 1.0), 20);
        assert_eq!(transmod_slider_thumb(20, 0.25, 1.0), 15);
        assert_eq!(slider_point(20, 100, 5.0, 25.0), 0.75);
        assert!(on_slider_thumb(20, 100, 1.0, 1.0, 5.0, 10.0));
        assert!(!on_slider_thumb(20, 100, 1.0, 1.0, 5.0, 50.0));
    }

    #[test]
    fn tap_tempo_needs_four_taps() {
        let t0 = std::time::Instant::now();
        let mut t = TapTempo::default();
        let at = |s: f64| t0 + std::time::Duration::from_secs_f64(s);
        assert_eq!(t.tap(at(0.0)), None);
        assert_eq!(t.tap(at(0.5)), None);
        assert_eq!(t.tap(at(1.0)), None);
        assert!((t.tap(at(1.5)).unwrap() - 120.0).abs() < 1e-3);
        // A gap of 5 s starts again.
        assert_eq!(t.tap(at(7.0)), None);
    }

    #[test]
    fn tempo_drags_whole_and_fraction() {
        // 1/200 of the 230 BPM range: 1.15 → floor.
        let (t, _) = tempo_drag(120.5, 0.0, true, 1.0 / 200.0);
        assert!((t - 121.5).abs() < 1e-4);
        let (t, f) = tempo_drag(120.5, 0.0, false, 0.5);
        assert!((t - 120.495).abs() < 1e-4);
        assert_eq!(f, 0.5);
        assert_eq!(tempo_drag(249.0, 0.0, true, 1.0).0, 250.0);
    }
}
