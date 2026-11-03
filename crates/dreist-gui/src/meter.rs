//! [re:d45123]: per-channel smoothing, peak hold and clip
//! latch (`Update` [re:4d45b9]) and drawing (`DrawVerticalBackground`
//! [re:26a3fa], `DrawVerticalMeters` [re:c82e14], `DrawHorizontalMeters`
//! [re:cc7f14], `DecibelsToPosition` [re:ecd866]).
//!
//! Style attributes ([re:1e8fdf]
//! [re:26dfaa]): 0x20 intMarkingSize (12), 0x21 intClipDimension (10), 0x22
//! intChannelSpacing (2), 0x23 colChannelBack, 0x24 colPeak, 0x25 colClip,
//! 0x26 colClipArea, 0x27 colMinDB, 0x28 colMidDB, 0x29 colMaxDB, 0x2a
//! colMarkingText, 0x2b colMarkingLines, 0x2c optHideMarkingText.

use crate::colour::Colour;
use crate::ctrl::Ctrl;
use crate::paint::{Painter, RectF};
use crate::style::{HAlign, StyleSheet, VAlign};

/// Range set by the constructor (+0x224 / +0x228).
pub const MIN_DB: f32 = -90.0;
pub const MAX_DB: f32 = 0.0;
/// Smoothing coefficients set on the first `Update` (+0x218 / +0x21c).
const ATTACK: f32 = 4.641_581e-34;
const RELEASE: f32 = 0.814_912_74;
/// Peak hold in updates, then −0.6 dB per update.
const HOLD: i32 = 30;
const PEAK_FALL_DB: f32 = 0.6;

/// [re:64f223] (20 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeterInfo {
    /// Displayed value: dB (log meters) or amplitude (linear meters).
    pub value: f32,
    /// Smoothed amplitude.
    pub level: f32,
    pub peak: f32,
    pub clip: bool,
    pub hold: i32,
}

/// `Update` [re:4d45b9]: feed one peak amplitude per channel; true when the
/// display changed.
pub fn update(meters: &mut Vec<MeterInfo>, levels: &[f32], linear: bool) -> bool {
    if meters.len() != levels.len() {
        meters.resize(levels.len(), MeterInfo::default());
    }
    let mut changed = false;
    for (m, &x) in meters.iter_mut().zip(levels) {
        let a = x.abs();
        let (old_v, old_p, old_c) = (m.value, m.peak, m.clip);
        let v = if linear {
            let s = if a < m.level { a + (m.level - a) * RELEASE } else { a };
            m.level = s;
            s.abs()
        } else {
            let k = if a < m.level { RELEASE } else { ATTACK };
            let s = (m.level - a) * k + a;
            m.level = s;
            (20.0 * s.abs().log10()).clamp(MIN_DB, MAX_DB)
        };
        m.value = v;
        if v >= m.peak {
            m.peak = v;
            m.hold = HOLD;
        } else {
            let h = m.hold;
            if h != 0 {
                m.hold = h - 1;
            }
            if h < 2 {
                m.peak = (m.peak - PEAK_FALL_DB).max(v);
            }
        }
        m.clip |= if linear { v >= 1.0 } else { v >= 0.0 };
        changed |= (old_v - m.value).abs() >= 1e-6 || (old_p - m.peak).abs() >= 1e-6 || old_c != m.clip;
    }
    changed
}

struct Look {
    marking: f32,
    clip: f32,
    spacing: f32,
    hide_text: bool,
    fill: Colour,
    border: Colour,
    text_offset: f32,
    back: Colour,
    peak: Colour,
    clip_col: Colour,
    clip_area: Colour,
    min: Colour,
    mid: Colour,
    max: Colour,
    text: Colour,
    lines: Colour,
}

fn look(s: &StyleSheet, st: &str) -> Look {
    let i = |k: &str, d: f32| s.float(st, k, d);
    let c = |k: &str, d: Colour| s.colour(st, k, d);
    let border_std = Colour::rgba(100, 100, 100, 255);
    Look {
        marking: i("intMarkingSize", 12.0).max(0.0),
        clip: i("intClipDimension", 10.0),
        spacing: i("intChannelSpacing", 2.0),
        hide_text: s.bool(st, "optHideMarkingText", false),
        fill: c("colFill", Colour::WHITE),
        border: c("colBorder", border_std),
        text_offset: i("realTextOffset", 0.0),
        back: c("colChannelBack", Colour::WHITE),
        peak: c("colPeak", Colour::rgba(0, 228, 161, 255)),
        clip_col: c("colClip", Colour::rgba(255, 0, 0, 255)),
        clip_area: c("colClipArea", Colour::rgba(255, 0, 0, 255).darken(0.4)),
        min: c("colMinDB", Colour::rgba(0, 255, 0, 255)),
        mid: c("colMidDB", Colour::rgba(255, 255, 0, 255)),
        max: c("colMaxDB", Colour::rgba(255, 165, 0, 255)),
        text: c("colMarkingText", border_std),
        lines: c("colMarkingLines", border_std.with_alpha(0.25)),
    }
}

/// `DecibelsToPosition` [re:ecd866]: pixel offset (from the top for vertical
/// meters, from the left for horizontal ones).
fn position(w: f32, h: f32, clip: f32, v: f32, linear: bool) -> f32 {
    let frac = if linear { v } else { -((v - MIN_DB) / (MIN_DB - MAX_DB)) };
    if h < w { (frac * (w - clip)).floor() } else { (h - frac * (h - clip)).floor() }
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> RectF {
    RectF::new(x, y, w.max(0.0), h.max(0.0))
}

pub fn draw(p: &mut Painter, s: &StyleSheet, st: &str, c: &Ctrl) {
    let lk = look(s, st);
    let linear = c.attr_bool("optIsLinear", false);
    let (w, h) = (c.rect.w as f32, c.rect.h as f32);
    let n = c.meters.len().max(2);
    let meters: Vec<MeterInfo> = if c.meters.is_empty() { vec![MeterInfo { value: if linear { 0.0 } else { MIN_DB }, ..Default::default() }; n] } else { c.meters.clone() };
    let pos = |v: f32| position(w, h, lk.clip, v, linear);
    if h >= w {
        let m = if lk.marking + lk.marking > w || lk.hide_text { 0.0 } else { lk.marking };
        let hide = !(lk.marking + lk.marking <= w) || lk.hide_text;
        // DrawVerticalBackground.
        let area = rect(m, 0.0, w - m, h);
        p.fill_rect(area, lk.fill);
        let cw = ((area.w - (n as f32 + 1.0) * lk.spacing) / n as f32).max(0.0);
        let clip = lk.clip.max(0.0);
        for i in 0..n {
            let cx = area.x + lk.spacing + i as f32 * (cw + lk.spacing);
            p.fill_rect(rect(cx, 0.0, cw, h), lk.back);
            p.fill_rect(rect(cx, 0.0, cw, clip), lk.clip_area);
            if i + 1 < n {
                let x = (cx + cw).round() + 1.0;
                p.line(x, 0.0, x, h, lk.border, 1.0);
            }
        }
        let step = if h < 50.0 {
            18.0
        } else if h < 100.0 {
            12.0
        } else if h < 200.0 {
            9.0
        } else if h < 250.0 {
            6.0
        } else {
            3.0
        };
        if lk.lines.a != 0 || !hide {
            let font = s.font(st);
            let mut db = -3.0f32;
            while db > MIN_DB {
                let y = position(w, h, lk.clip, db, false);
                p.fill_rect(rect(area.x, y, area.w, 1.0), lk.lines);
                if !hide {
                    let label = format!("{}", db.floor() as i32);
                    p.text(RectF::new(0.0, y - 7.0, m - lk.text_offset, 14.0), &label, lk.text, &font, HAlign::Right, VAlign::Center);
                }
                db -= step;
            }
        }
        // DrawVerticalMeters.
        let (p30, p9) = (pos_db(w, h, lk.clip, -30.0), pos_db(w, h, lk.clip, -9.0));
        let bottom = h;
        for (i, mi) in meters.iter().enumerate() {
            let cx = area.x + lk.spacing + i as f32 * (cw + lk.spacing);
            if mi.clip {
                p.fill_rect(rect(cx + 2.0, 2.0, cw - 4.0, clip - 4.0), lk.clip_col);
            }
            let y = pos(mi.value);
            let (x, bw) = (cx + 2.0, cw - 4.0);
            if !linear && mi.value >= -30.0 {
                if mi.value >= -9.0 {
                    p.fill_rect(rect(x, y, bw, p9 - y), lk.max);
                    p.fill_rect(rect(x, p9, bw, p30 - p9), lk.mid);
                } else {
                    p.fill_rect(rect(x, y, bw, p30 - y), lk.mid);
                }
                p.fill_rect(rect(x, p30, bw, bottom - p30), lk.min);
            } else {
                p.fill_rect(rect(x, y, bw, bottom - y), lk.min);
            }
            if mi.peak.abs() >= 1e-6 && (mi.peak - MIN_DB).abs() >= 1e-6 {
                let half = (bw * 0.5).max(0.0);
                p.fill_rect(rect(x + bw * 0.25 + half * 0.25, pos(mi.peak), half * 0.5, 2.0), lk.peak);
            }
        }
        p.stroke_rect(area.pixel_align(), lk.border, 1.0);
    } else {
        // Horizontal: rows top to bottom, clip area at the right
        // (`DrawHorizontalMeters`; the background mirrors the vertical one).
        let m = if lk.marking + lk.marking > h || lk.hide_text { 0.0 } else { lk.marking };
        let area_h = (h - m).max(0.0);
        p.fill_rect(rect(0.0, 0.0, w, area_h), lk.fill);
        let rh = ((area_h - (n as f32 + 1.0) * lk.spacing) / n as f32).max(0.0);
        let clip = lk.clip.max(0.0);
        let clip_x = w - clip - lk.spacing + 2.0;
        let (p30, p9) = (pos_db(w, h, lk.clip, -30.0), pos_db(w, h, lk.clip, -9.0));
        for (i, mi) in meters.iter().enumerate() {
            let ry = lk.spacing + i as f32 * (rh + lk.spacing);
            p.fill_rect(rect(0.0, ry, w, rh), lk.back);
            p.fill_rect(rect(w - clip, ry, clip, rh), lk.clip_area);
            if mi.clip {
                p.fill_rect(rect(clip_x, ry + 2.0, clip - 4.0, rh - 4.0), lk.clip_col);
            }
            let x = pos(mi.value);
            let (y, bh) = (ry + 2.0, rh - 4.0);
            if !linear && x >= p30 {
                if x >= p9 {
                    p.fill_rect(rect(p9, y, x - p9, bh), lk.max);
                    p.fill_rect(rect(p30, y, p9 - p30, bh), lk.mid);
                } else {
                    p.fill_rect(rect(p30, y, x - p30, bh), lk.mid);
                }
                p.fill_rect(rect(0.0, y, p30.max(0.0), bh), lk.min);
            } else {
                p.fill_rect(rect(0.0, y, x, bh), lk.min);
            }
            if mi.peak.abs() >= 1e-6 && (mi.peak - MIN_DB).abs() >= 1e-6 {
                let half = (bh * 0.5).max(0.0);
                p.fill_rect(rect(pos(mi.peak), y + bh * 0.25 + half * 0.25, 2.0, half * 0.5), lk.peak);
            }
        }
    }
}

/// Position of a dB value regardless of the meter's linear flag (the
/// colour band edges at −30 / −9 dB).
fn pos_db(w: f32, h: f32, clip: f32, db: f32) -> f32 {
    position(w, h, clip, db, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_holds_then_falls() {
        let mut m = Vec::new();
        assert!(update(&mut m, &[1.0, 0.5], false));
        assert_eq!(m[0].value, 0.0);
        assert!(m[0].clip && !m[1].clip);
        for _ in 0..29 {
            update(&mut m, &[0.0, 0.0], false);
        }
        assert_eq!(m[0].peak, 0.0, "held for 30 updates");
        update(&mut m, &[0.0, 0.0], false);
        update(&mut m, &[0.0, 0.0], false);
        assert!(m[0].peak < 0.0 && m[0].peak > -2.0, "{}", m[0].peak);
        // Release smoothing.
        assert!(m[0].level > 0.0 && m[0].level < 1.0);
    }

    #[test]
    fn positions() {
        // 15×64 vertical, clip 8: 0 dB at the clip edge, −90 at the bottom.
        assert_eq!(position(15.0, 64.0, 8.0, 0.0, false), 8.0);
        assert_eq!(position(15.0, 64.0, 8.0, MIN_DB, false), 64.0);
        assert_eq!(position(156.0, 12.0, 8.0, 0.0, false), 148.0);
    }
}
