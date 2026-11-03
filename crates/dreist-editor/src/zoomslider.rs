//! [re:28a1ae] ([re:bd0c5b]..) and the [re:1402db] zoom logic
//! shared by the layer wave display: the view is (start, size) as fractions
//! of the file. See the dreist-re GUI notes (Zoom slider, Wave display zoom).

use dreist_gui::colour::Colour;
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::style::StyleSheet;

/// Thumb (x, width) in a track `tw` wide.
pub fn thumb(view: (f32, f32), tw: f32) -> (f32, f32) {
    (view.0 * tw, view.1 * tw)
}

/// Left press [re:89f320]: 2 left edge, 3 right edge (within 2 px),
/// 1 inside the thumb.
pub fn hit(view: (f32, f32), tw: f32, h: f32, x: f32, y: f32) -> Option<u8> {
    let (tx, thw) = thumb(view, tw);
    if y < 0.0 || y >= h {
        None
    } else if (x - tx).abs() < 2.0 {
        Some(2)
    } else if (x - (tx + thw)).abs() < 2.0 {
        Some(3)
    } else if x >= tx && x < tx + thw {
        Some(1)
    } else {
        None
    }
}

/// [re:632a19] without a press: within
/// 2 px of a thumb edge the left / right cursor, else the arrow.
pub fn over_edge(view: (f32, f32), tw: f32, h: f32, x: f32, y: f32) -> bool {
    let (tx, thw) = thumb(view, tw);
    y >= 0.0 && y < h && ((x - tx).abs() < 2.0 || (x - (tx + thw)).abs() < 2.0)
}

/// The zone drags (centre [re:109246], left
/// [re:3ef21b], right [re:4f55d9]): the new view after moving
/// from `last` to `x` (thumb at least 20 px and `min` of the track).
pub fn drag(view: (f32, f32), tw: f32, mode: u8, last: f32, x: f32, min: f32) -> (f32, f32) {
    let (mut tx, mut thw) = thumb(view, tw);
    let min_w = min * tw;
    match mode {
        1 => tx = (tx + x - last).clamp(0.0, (tw - thw).max(0.0)),
        2 => {
            let right = tx + thw;
            let nx = x.min(right - 20.0).min(right - min_w).max(0.0);
            thw = (right - nx).max(0.0);
            tx = nx;
        }
        _ => {
            let right = x.max(tx + 20.0f32.max(min_w)).min(tw);
            thw = (right - tx).max(0.0);
        }
    }
    clamp((tx / tw.max(1.0), (thw / tw.max(1.0)).max(min)))
}

/// Keep a view inside the file.
pub fn clamp(view: (f32, f32)) -> (f32, f32) {
    let size = view.1.clamp(1e-6, 1.0);
    (view.0.clamp(0.0, 1.0 - size), size)
}

/// [re:34ae9d]: size / `factor` (at least `track_w /
/// frames`), the start moving by (old − new)·`centre`.
pub fn zoom_in(view: (f32, f32), factor: f32, centre: f32, track_w: f32, frames: u32) -> (f32, f32) {
    if factor == 1.0 || factor == 0.0 || frames == 0 {
        return view;
    }
    let size = (view.1 / factor).max(track_w / frames as f32).min(1.0);
    clamp((view.0 + (view.1 - size) * centre, size))
}

/// Auto zoom (`AutoZoomGroup`, option `WaveDisplayZoomMode`): 1
/// both markers [re:338aad] (the markers ± 500 frames), 2
/// start marker [re:d6b60d] / 3 end marker [re:e23870] (1 %
/// of the file centred on the marker); `None` for mode 0.
pub fn auto_zoom(mode: i64, frames: u32, start: u32, end: u32) -> Option<(f32, f32)> {
    if !(1..=3).contains(&mode) {
        return None;
    }
    if frames < 2 {
        return Some((0.0, 1.0));
    }
    let n1 = (frames - 1) as f32;
    let v = match mode {
        1 => {
            let a = (start as i64 - 500).max(0);
            let b = (end as i64 + 500).min(frames as i64 - 1);
            let s = if start == 0 { 0.0 } else { (a as f32 - 1.0) / n1 };
            (s, (b - a) as f32 / n1)
        }
        _ => {
            let m = if mode == 2 { start } else { end };
            let half = ((frames as f32 * 0.01) as i64) / 2;
            let s = if m == 0 { 0.0 } else { ((m as i64 - half) as f32 - 1.0) / n1 };
            (s.max(0.0), 0.01)
        }
    };
    Some(clamp(v))
}

/// [re:867658]: the thumb in colFill, 2 px white
/// edge lines, white 10 px discs with black 6 px centres.
pub fn draw(p: &mut Painter, s: &StyleSheet, style: &str, w: f32, h: f32, view: (f32, f32)) {
    let (tx, tw) = thumb(view, w);
    p.fill_rect(RectF::new(tx, 0.0, tw, h), s.colour(style, "colFill", Colour::rgba(255, 192, 203, 255)));
    for ex in [tx, tx + tw] {
        p.fill_rect(RectF::new(ex - 1.0, 0.0, 2.0, h), Colour::WHITE);
    }
    let cy = (h as i32 / 2) as f32;
    for ex in [tx, tx + tw] {
        p.fill_ellipse(RectF::new(ex - 5.0, cy - 5.0, 10.0, 10.0), Colour::WHITE);
        p.fill_ellipse(RectF::new(ex - 3.0, cy - 3.0, 6.0, 6.0), Colour::BLACK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zooms() {
        assert_eq!(zoom_in((0.0, 1.0), 2.0, 0.5, 100.0, 1000), (0.25, 0.5));
        let (s, z) = auto_zoom(1, 10001, 1000, 2000).unwrap();
        assert!((s - 0.0499).abs() < 1e-3 && (z - 0.2).abs() < 1e-3, "{s} {z}");
        assert_eq!(auto_zoom(2, 10001, 0, 2000), Some((0.0, 0.01)));
        assert_eq!(auto_zoom(0, 10001, 0, 2000), None);
        assert_eq!(drag((0.0, 0.5), 100.0, 1, 10.0, 30.0, 0.0), (0.2, 0.5));
    }
}
