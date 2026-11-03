//! The wave display's timeline strip ([re:a2cbaa]
//! [re:ad103f], `ComputeComponentAreas` [re:d9085e], `OnMouseRButtonDown`
//! [re:86971c], `LoadTimelineMode` [re:e959cc]). See the dreist-re GUI notes (Wave
//! display timeline).
//!
//! Areas: the timeline 0..15, the marker strip 15..30 (both filled with
//! colTimelineBackground), the waveform from 30 down. The visible range is
//! labelled in Time (min:sec:ms, relative to the start marker) or Beats
//! (bar.beat, bar.beat.16th when at most 4 beats are visible) mode; the
//! mode is the option `<prefix>WaveDisplayTimelineMode` ("Time" / "Beats";
//! prefixes `PadEdit` and `Slicer`).

use dreist_gui::colour::Colour;
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::style::{HAlign, StyleSheet, VAlign};

/// Timeline strip height.
pub const STRIP: f32 = 15.0;
/// Top of the waveform area (timeline + marker strip).
pub const WAVE_TOP: f32 = 30.0;

pub fn option_key(prefix: &str) -> String {
    format!("{prefix}WaveDisplayTimelineMode")
}

/// Beats mode stored for `prefix`.
pub fn beats_mode(prefix: &str) -> bool {
    crate::options::Options::load().string(&option_key(prefix), "") == "Beats"
}

pub fn save_mode(prefix: &str, beats: bool) {
    let mut o = crate::options::Options::load();
    o.set(&option_key(prefix), if beats { "Beats" } else { "Time" });
    o.save();
}

/// Time mode labels: (milliseconds since the start marker) for a visible
/// range starting `start_ms` and `len_ms` long. The original steps a time
/// cursor by `step` and labels floor(t / R)·R, skipping repeats; where the
/// step is below R that is every multiple of R in the range.
pub fn time_labels(start_ms: f64, len_ms: f64) -> Vec<f64> {
    let l = len_ms;
    let c = len_ms / 1000.0;
    let (step, round) = if l > 20000.0 {
        (c * 0.0005, 20000.0)
    } else if l > 10000.0 {
        (c * 0.1, 2000.0)
    } else if l > 5000.0 {
        (c * 0.25, 1000.0)
    } else if l > 4000.0 {
        (1000.0, 250.0)
    } else if l > 3000.0 {
        (750.0, 100.0)
    } else if l > 2000.0 {
        (500.0, 100.0)
    } else if l > 1000.0 {
        (200.0, 10.0)
    } else if l > 500.0 {
        (100.0, 10.0)
    } else if l > 200.0 {
        (50.0, 10.0)
    } else if l > 100.0 {
        (20.0, 10.0)
    } else {
        (if l > 50.0 { 10.0 } else { 5.0 }, 10.0)
    };
    let step = step.max(1e-9);
    let first = (start_ms.floor() / step).floor() * step;
    let end = first + l + step;
    let mut out = Vec::new();
    if step < round {
        let mut k = (first / round).floor();
        while k * round < end {
            out.push(k * round);
            k += 1.0;
        }
        return out;
    }
    let mut t = first;
    let mut last = f64::NAN;
    while t < end {
        let v = (t.floor() / round).floor() * round;
        if !(last - v).abs().lt(&1e-3) {
            out.push(v);
            last = v;
        }
        t += step;
    }
    out
}

/// "%d:%02d:%02d" as the original formats it (its minutes term subtracts
/// minutes·1000/60 instead of minutes·60000).
pub fn time_text(ms: f64) -> String {
    let ms_i = ms as i32;
    let min = ms_i / 60000;
    let f = ms as f32 - ((min * 1000) / 60) as f32;
    let sec = f as i32 / 1000;
    format!("{}:{:02}:{:02}", min, sec, (f - (sec * 1000) as f32) as i32)
}

/// Beats mode labels: (unit index, text, draw the grid line). `unit_ms` is
/// a beat (60000 / tempo), or a 16th when at most 4 beats are visible.
pub fn beat_labels(start_ms: f64, len_ms: f64, tempo: f64) -> (f64, Vec<(i64, String, bool)>) {
    let mut unit = 60000.0 / tempo.max(1.0);
    let nb = len_ms / unit;
    let sub = nb <= 4.0;
    let mut count = nb.ceil() as i64;
    if sub {
        count *= 4;
        unit /= 4.0;
    }
    let u = (start_ms / unit).ceil() as i64;
    // Counters for position u (4/4).
    let (mut bar, mut beat, mut six) = if u < 1 {
        (1, 1, 1)
    } else if sub {
        (u / 16 + 1, (u / 4) % 4 + 1, u % 4 + 1)
    } else {
        (u / 4 + 1, u % 4 + 1, 1)
    };
    let inc: i64 = if len_ms > 500000.0 {
        500
    } else if len_ms > 200000.0 {
        200
    } else {
        1
    };
    let mut out = Vec::new();
    let mut idx = u;
    while idx < count + u {
        let first = six == 1 && beat == 1 && bar == 1;
        let text = if sub { format!("{bar}.{beat}.{six}") } else { format!("{bar}.{beat}") };
        out.push((idx, text, !first));
        let mut b = beat;
        if sub {
            if six < 4 {
                six += 1;
            } else {
                six = 1;
                b += inc;
            }
        } else {
            b += inc;
        }
        if inc == 1 {
            if b > 4 {
                b = 1;
                six = 1;
                bar += 1;
            }
        } else {
            six = 1;
            while b > 4 {
                bar += 1;
                b -= inc;
            }
        }
        beat = b;
        idx += inc;
    }
    (unit, out)
}

/// Draw the timeline and marker strips. `start_s` / `len_s`: the visible
/// range; `offset_s`: the start marker (label zero); `x_of`: seconds → x;
/// `wave_h`: height of the waveform area below [`WAVE_TOP`].
#[allow(clippy::too_many_arguments)]
pub fn draw(p: &mut Painter, s: &StyleSheet, style: &str, w: f32, wave_h: f32, start_s: f64, len_s: f64, offset_s: f64, beats: bool, tempo: f64, x_of: &dyn Fn(f64) -> f32) {
    let purple = Colour::rgba(128, 0, 128, 255);
    let back = s.colour(style, "colTimelineBackground", purple);
    p.fill_rect(RectF::new(0.0, 0.0, w, WAVE_TOP), back);
    let text = s.colour(style, "colTimelineText", Colour::WHITE);
    let font = s.font_attr(style, "fontTimeline");
    let label = |p: &mut Painter, x: f32, t: &str| p.text(RectF::new(x + 2.0, 1.0, 50.0, STRIP), t, text, &font, HAlign::Left, VAlign::Center);
    // The range relative to the start marker.
    let (start_ms, len_ms) = ((start_s - offset_s) * 1000.0, len_s * 1000.0);
    if !beats {
        for ms in time_labels(start_ms, len_ms) {
            let x = x_of(ms * 0.001 + offset_s);
            if (x as i32) < 0 || x > w {
                continue;
            }
            let x = x.floor();
            p.line(x + 0.5, 0.0, x + 0.5, STRIP, text, 1.0);
            label(p, x, &time_text(ms));
        }
    } else {
        let marks = s.colour(style, "colTimelineMarkers", Colour::WHITE);
        let (unit, labels) = beat_labels(start_ms, len_ms, tempo);
        for (idx, t, line) in labels {
            let x = x_of(idx as f64 * unit * 0.001 + offset_s);
            if x < -60.0 || x > w {
                continue;
            }
            let x = x.floor();
            if line {
                p.line(x + 0.5, 0.0, x + 0.5, WAVE_TOP, marks, 1.0);
                p.line(x + 0.5, WAVE_TOP, x + 0.5, WAVE_TOP + wave_h, marks, 1.0);
            }
            label(p, x, &t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(time_labels(0.0, 2500.0), vec![0.0, 500.0, 1000.0, 1500.0, 2000.0, 2500.0]);
        assert_eq!(time_labels(0.0, 3500.0)[..4], [0.0, 700.0, 1500.0, 2200.0]);
        assert_eq!(time_labels(0.0, 30000.0)[..2], [0.0, 20000.0]);
        assert_eq!(time_text(1500.0), "0:01:500");
        assert_eq!(time_text(61000.0), "1:60:984");
        let (unit, b) = beat_labels(0.0, 1000.0, 120.0);
        assert!((unit - 125.0).abs() < 1e-9);
        assert_eq!(b[0], (0, "1.1.1".into(), false));
        assert_eq!(b[5].1, "1.2.2");
        let (_, b) = beat_labels(0.0, 10000.0, 120.0);
        assert_eq!(b[4].1, "2.1");
    }
}
