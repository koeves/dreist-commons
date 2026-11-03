//! [re:6b287b] iconic (folded) state.
//!
//! - `Serialize` [re:a2a347]: `intIconicDim` (default 20), `intNonIconicDim`
//!   (default −1), `iconicTarget` (`StringToAxisResizeEdge`, map built
//!   from "Top, Left, Bottom, Right": ARTop 0, ARLeft 1, ARBottom 2,
//!   ARRight 3; default 1).
//! - `SetRect` [re:09048a]: the header strip is (0, 0, width, iconic dim);
//!   for a left / right target in the iconic state it is (0, 0, iconic dim,
//!   height).
//! - `OnMouseLButtonDblClick` [re:b38b90]: a double click on the panel itself
//!   (no child under the pointer) inside the header toggles the state with
//!   `SetIconicStateAnimated(!iconic, 375 ms, Logarithmic)`.
//! - `SetIconicStateAnimated` [re:50c370]: folding first stores the current
//!   height (top / bottom) or width (left / right) as the non-iconic dim;
//!   then the start notification (collapse start, or expand start when
//!   iconic) and `SetRectAnimated` to the rect with that axis = iconic dim
//!   (folding) or the non-iconic dim. The end lambda sets the state and
//!   sends collapse end / expand end (as `SetIconicState` [re:16d952], which
//!   does the same without animating).
//! - [re:54085c]: linear t += dt / duration
//!   (clamped), v = curve(t), rect = start + floor((target − start)·v);
//!   [re:24335c] is powf(t, 10).
//! - The containers that stack these panels ([re:efb170] LayerParams,
//!   [re:6929fb], [re:8b43ec] $_0) re-stack on the
//!   panel's rect-changed event [re:91eddf] (vfunc 0x370
//!   `UpdateChildRects`): see [`restack`].

use crate::ctrl::{Ctrl, Rect};

/// `SetIconicStateAnimated` duration (0x43bb8000).
pub const ANIM_MS: f32 = 375.0;

/// `iconicTarget`: 0 top, 1 left, 2 bottom, 3 right, 4 unknown.
pub fn edge(c: &Ctrl) -> u8 {
    match c.attr("iconicTarget").map(str::trim) {
        None => 1,
        Some("ARTop") => 0,
        Some("ARLeft") => 1,
        Some("ARBottom") => 2,
        Some("ARRight") => 3,
        _ => 4,
    }
}

pub fn iconic_dim(c: &Ctrl) -> i32 {
    c.attr_i32("intIconicDim", 20)
}

pub fn is_iconic(c: &Ctrl) -> bool {
    c.attr("iconic") == Some("1")
}

/// The header strip (`SetRect`).
pub fn header(c: &Ctrl) -> Rect {
    let d = iconic_dim(c);
    match edge(c) {
        1 | 3 if is_iconic(c) => Rect::new(0, 0, d, c.rect.h),
        0..=3 => Rect::new(0, 0, c.rect.w, d),
        _ => Rect::default(),
    }
}

/// The header text is drawn rotated −90° (reading upwards).
pub fn vertical_text(c: &Ctrl) -> bool {
    is_iconic(c) && matches!(edge(c), 1 | 3)
}

/// The rect for state `iconic` (the folding axis = iconic dim or the
/// stored non-iconic dim).
pub fn target_rect(c: &Ctrl, iconic: bool) -> Rect {
    let mut r = c.rect;
    let full = c.attr_i32("nonIconicDim", c.attr_i32("intNonIconicDim", -1));
    // Folded at load (`SetIconicState(true)` stores nothing): expand to the
    // skin's size rather than the original's −1.
    let full = if full < 0 { c.attr_i32("skinDim", full) } else { full };
    let dim = if iconic { iconic_dim(c) } else { full };
    match edge(c) {
        0 | 2 => r.h = dim,
        1 | 3 => r.w = dim,
        _ => {}
    }
    r
}

/// Remember the unfolded size before folding (and the skin's size once).
pub fn store_full(c: &mut Ctrl) {
    let d = match edge(c) {
        0 | 2 => c.rect.h,
        1 | 3 => c.rect.w,
        _ => return,
    };
    if c.attr("skinDim").is_none() {
        c.set_attr("skinDim", d.to_string());
    }
    if !is_iconic(c) {
        c.set_attr("nonIconicDim", d.to_string());
    }
}

/// `SetIconicState`: jump to `iconic` without animating.
pub fn set_iconic(c: &mut Ctrl, iconic: bool) {
    if c.attr("skinDim").is_none() {
        let d = if matches!(edge(c), 0 | 2) { c.rect.h } else { c.rect.w };
        c.set_attr("skinDim", d.to_string());
    }
    let r = target_rect(c, iconic);
    c.set_attr("iconic", if iconic { "1" } else { "0" }.into());
    c.set_rect(r);
}

/// `Logarithmic` curve.
pub fn curve(t: f32) -> f32 {
    t.clamp(0.0, 1.0).powf(10.0)
}

/// [re:f41060]: the rect at curve value `v`.
pub fn lerp_rect(a: Rect, b: Rect, v: f32) -> Rect {
    let f = |s: i32, e: i32| s + ((e - s) as f32 * v).floor() as i32;
    Rect::new(f(a.x, b.x), f(a.y, b.y), f(a.w, b.w), f(a.h, b.h))
}

/// Re-stack `parent`'s children after one changed size.
///
/// - [re:d71c28]: children placed from
///   (`intLeftSpacing`, `intTopSpacing`) along the axis (`optIsVertical`)
///   `intIntraSpacing` apart, each spanning the box across it minus the
///   spacings; the box takes the total length.
/// - Containers the editor fills in code carry `stackAxis` ("h" / "v") and
///   `stackGap`: children keep their size and follow each other.
pub fn restack(parent: &mut Ctrl) {
    if parent.class == "CtrlAutoStackBox" {
        let vertical = parent.attr_bool("optIsVertical", false);
        let (l, t, r, b, gap) = (
            parent.attr_i32("intLeftSpacing", 0),
            parent.attr_i32("intTopSpacing", 0),
            parent.attr_i32("intRightSpacing", 0),
            parent.attr_i32("intBottomSpacing", 0),
            parent.attr_i32("intIntraSpacing", 0),
        );
        let n = parent.children.len() as i32;
        let (pw, ph) = (parent.rect.w, parent.rect.h);
        let mut pos = if vertical { t } else { l };
        let mut total = (n - 1).max(0) * gap + if vertical { t + b } else { l + r };
        for c in &mut parent.children {
            let r = if vertical { Rect::new(l, pos, (pw - r - l).max(0), c.rect.h.max(0)) } else { Rect::new(pos, t, c.rect.w.max(0), (ph - b - t).max(0)) };
            let step = if vertical { r.h } else { r.w };
            total += step;
            c.set_rect(r);
            pos += step + gap;
        }
        let mut pr = parent.rect;
        if vertical {
            pr.h = total;
        } else {
            pr.w = total;
        }
        parent.rect = pr;
        return;
    }
    let vertical = match parent.attr("stackAxis") {
        Some("v") => true,
        Some("h") => false,
        _ => return,
    };
    let gap = parent.attr_i32("stackGap", 0);
    let mut pos = 0;
    for c in &mut parent.children {
        let mut r = c.rect;
        if vertical {
            r.y = pos;
            pos += r.h + gap;
        } else {
            r.x = pos;
            pos += r.w + gap;
        }
        c.rect = r;
    }
}

/// [re:ae924d]: t above 0.98 passes through,
/// else 1 − exp(t / −0.2).
pub fn exponential(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t > 0.98 { t } else { 1.0 - (t / -0.2).exp() }
}

/// What a finished rect animation reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimEnd {
    /// A collapse panel fold (`iconic`).
    Fold(bool),
    /// A caller's animation (`UiEvent::Animated` tag).
    Tag(u32),
}

/// A running `Ctrl::SetRectAnimated`.
#[derive(Clone, Debug)]
pub struct Anim {
    pub path: Vec<usize>,
    pub from: Rect,
    pub to: Rect,
    pub ms: f32,
    pub curve: fn(f32) -> f32,
    pub end: AnimEnd,
    pub start: std::time::Instant,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel(target: &str) -> Ctrl {
        let mut c = Ctrl { class: "CtrlCollapsePanel".into(), rect: Rect::new(0, 0, 250, 182), ..Ctrl::default() };
        c.set_attr("iconicTarget", target.into());
        c
    }

    #[test]
    fn header_and_targets() {
        let mut c = panel("ARLeft");
        assert_eq!(header(&c), Rect::new(0, 0, 250, 20));
        store_full(&mut c);
        assert_eq!(target_rect(&c, true), Rect::new(0, 0, 20, 182));
        set_iconic(&mut c, true);
        assert_eq!(header(&c), Rect::new(0, 0, 20, 182));
        assert!(vertical_text(&c));
        assert_eq!(target_rect(&c, false), Rect::new(0, 0, 250, 182));
        let mut t = panel("ARTop");
        store_full(&mut t);
        assert_eq!(target_rect(&t, true), Rect::new(0, 0, 250, 20));
        assert_eq!(curve(0.5), 0.5f32.powf(10.0));
        assert_eq!(lerp_rect(Rect::new(0, 0, 250, 10), Rect::new(0, 0, 20, 10), 0.5), Rect::new(0, 0, 135, 10));
    }

    #[test]
    fn auto_stack_box_restacks() {
        let mut b = Ctrl { class: "CtrlAutoStackBox".into(), rect: Rect::new(0, 0, 500, 176), ..Ctrl::default() };
        b.set_attr("intIntraSpacing", "6".into());
        for w in [96, 55, 100] {
            b.children.push(Ctrl { rect: Rect::new(0, 0, w, 10), ..Ctrl::default() });
        }
        restack(&mut b);
        let xs: Vec<(i32, i32)> = b.children.iter().map(|c| (c.rect.x, c.rect.h)).collect();
        assert_eq!(xs, vec![(0, 176), (102, 176), (163, 176)]);
        assert_eq!(b.rect.w, 96 + 55 + 100 + 12);
    }
}
