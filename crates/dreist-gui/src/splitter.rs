//! [re:e58107]: two panels either side of a draggable bar.
//!
//! - Loading [re:c25b4d]: `optIsVertical` (default true: panels stacked
//!   top / bottom), `intMinSize0/1` (0), `intMaxSize0/1` (−1),
//!   `optSnapCollapse0/1` (true), `intSplitterDim` (5),
//!   `optDbClkCollapse1` (true), `optSplitterIsLocked` (false),
//!   `strAutoSaveName`, `optIsUserDraggable` (true), `intSplitPosition`.
//! - Clamping [re:e205e3]: with `total` = size − bar,
//!   past `total − min1` the position stops there, or snaps to `total`
//!   (panel 1 collapsed) beyond `total − min1/2` when `optSnapCollapse1`;
//!   below `min0` it stops at `min0`, or snaps to 0 below `min0/2` when
//!   `optSnapCollapse0`; then at most `intMaxSize0` (−1 none).
//!   `intMaxSize1` is never used.
//! - The bar ([re:e1f523]): mouse down ([re:a99e57]) unless
//!   locked keeps the position; mouse move [re:f5f6a3] (draggable only)
//!   moves it by the pointer's travel, clamped; mouse up
//!   [re:623bfb] writes the position to the registry under `strAutoSaveName`;
//!   double click ([re:7c6fcb], not locked) inverts the collapse state
//!   [re:621671]: at either end → back to the saved position, else save it
//!   and go to the end (`optDbClkCollapse1`) or 0, clamped and animated
//!   ([re:500207]: 250 ms Linear bounds animation).
//!   Enter ([re:3d5de1], not locked, draggable): up / down cursor when
//!   vertical, left / right otherwise; exit: arrow.
//! - [re:9e9841]: the bar's style background,
//!   then (thin side > 3 px) seven 4×4 dots 6 px apart, centred, in the
//!   style's colAltBorder (default white).
//! - Resizing ([re:1edcc1], [re:ed7bab]; flags as loaded by [re:7b5e65]): with
//!   `optSplitterIsLocked` or `optDbClkCollapse1` the bar moves with the far
//!   edge (panel 1 keeps its size), else it stays (panel 0 keeps its size);
//!   then the position is clamped as above. The port moves it from the
//!   position last chosen rather than the clamped one, so a window shrunk
//!   and grown back gets the chosen layout back.
//! - Opening the editor [re:468d3b] → reading the saved state [re:a605da]: a saved
//!   position ≥ 0 is clamped and applied.

use crate::ctrl::{Ctrl, Rect};

pub fn vertical(c: &Ctrl) -> bool {
    c.attr_bool("optIsVertical", true)
}

pub fn dim(c: &Ctrl) -> i32 {
    c.attr_i32("intSplitterDim", 5).max(1)
}

pub fn position(c: &Ctrl) -> i32 {
    c.attr_i32("intSplitPosition", 0)
}

pub fn locked(c: &Ctrl) -> bool {
    c.attr_bool("optSplitterIsLocked", false)
}

pub fn draggable(c: &Ctrl) -> bool {
    c.attr_bool("optIsUserDraggable", true)
}

/// The bar's rect (relative to the splitter panel).
pub fn bar(c: &Ctrl) -> Rect {
    let (p, d) = (position(c), dim(c));
    if vertical(c) { Rect::new(0, p, c.rect.w, d) } else { Rect::new(p, 0, d, c.rect.h) }
}

/// Whether (`x`, `y`) (relative to the panel) is on the bar.
pub fn on_bar(c: &Ctrl, x: i32, y: i32) -> bool {
    c.class == "CtrlSplitterPanel" && {
        let b = bar(c);
        x >= b.x && x < b.x + b.w && y >= b.y && y < b.y + b.h
    }
}

fn length(c: &Ctrl) -> i32 {
    if vertical(c) { c.rect.h } else { c.rect.w }
}

/// The clamped bar position.
pub fn clamp(c: &Ctrl, p: i32) -> i32 {
    let total = length(c) - dim(c);
    let (min0, min1) = (c.attr_i32("intMinSize0", 0), c.attr_i32("intMinSize1", 0));
    let mut v = p;
    if v > total - min1 {
        v = if c.attr_bool("optSnapCollapse1", true) && v > total - min1 / 2 { total } else { total - min1 };
    }
    if v < min0 {
        v = if c.attr_bool("optSnapCollapse0", true) && v < min0 / 2 { 0 } else { min0 };
    }
    let max0 = c.attr_i32("intMaxSize0", -1);
    if max0 != -1 && v > max0 {
        v = max0;
    }
    v
}

/// The position last chosen (dragged, restored, set by code) and the
/// panel length then, kept beside the clamped position the bar has now:
/// `(wanted, length, placed)`.
fn wanted(c: &Ctrl) -> Option<(i32, i32, i32)> {
    let mut it = c.attr("splitWanted")?.split(',').map(|v| v.parse::<i32>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

fn set_wanted(c: &mut Ctrl, p: i32, len: i32, placed: i32) {
    c.set_attr("splitWanted", format!("{p},{len},{placed}"));
}

/// The panel was resized from `old` (its rect is already the new one): the
/// bar keeps panel 1's size when locked or `optDbClkCollapse1`, else panel
/// 0's, clamped. It moves from the position last chosen, not from where an
/// earlier, smaller size clamped it, so room coming back brings the chosen
/// layout back (and any resize path ends as a single resize would).
pub fn follow_resize(c: &mut Ctrl, old: Rect) {
    let len = length(c);
    let old_len = if vertical(c) { old.h } else { old.w };
    let (p0, len0) = match wanted(c) {
        Some((p, l, placed)) if placed == position(c) => (p, l),
        // Never chosen, or set by code since: from here.
        _ => (position(c), if old_len > dim(c) { old_len } else { len }),
    };
    let keep1 = locked(c) || c.attr_bool("optDbClkCollapse1", true);
    let p = clamp(c, p0 + if keep1 { len - len0 } else { 0 });
    c.set_attr("intSplitPosition", p.to_string());
    set_wanted(c, p0, len0, p);
}

/// Move the bar to `p` (unclamped) and lay the panels out.
pub fn set_position(c: &mut Ctrl, p: i32) {
    c.set_attr("intSplitPosition", p.to_string());
    let len = length(c);
    set_wanted(c, p, len, p);
    c.layout_splitter();
}

/// Inverting the collapse state: the position to animate to (remembers the
/// current one when collapsing).
pub fn invert_collapse(c: &mut Ctrl) -> i32 {
    let (p, end) = (position(c), length(c));
    let at_end = p == 0 || p + dim(c) == end;
    if at_end {
        return clamp(c, c.attr_i32("savedSplit", 0));
    }
    c.set_attr("savedSplit", p.to_string());
    clamp(c, if c.attr_bool("optDbClkCollapse1", true) { end } else { 0 })
}

/// A running bar position animation.
#[derive(Clone, Debug)]
pub struct Anim {
    pub path: Vec<usize>,
    pub from: i32,
    pub to: i32,
    pub start: std::time::Instant,
}

/// Animation length (Linear).
pub const ANIM_MS: f32 = 250.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn split(attrs: &[(&str, &str)]) -> Ctrl {
        let mut c = Ctrl { class: "CtrlSplitterPanel".into(), rect: Rect::new(0, 0, 1390, 746), ..Ctrl::default() };
        for (k, v) in attrs {
            c.attrs.insert((*k).into(), (*v).into());
        }
        c
    }

    #[test]
    fn utility_splitter_clamps_and_snaps() {
        // RootSkin's UtilitySplitter.
        let c = split(&[("intMaxSize0", "600"), ("intMinSize0", "250"), ("intSplitPosition", "300"), ("intSplitterDim", "8"), ("optDbClkCollapse1", "false"), ("optIsVertical", "false")]);
        assert_eq!(clamp(&c, 400), 400);
        assert_eq!(clamp(&c, 900), 600);
        assert_eq!(clamp(&c, 200), 250);
        assert_eq!(clamp(&c, 100), 0);
        let mut c = c;
        let to = invert_collapse(&mut c);
        assert_eq!(to, 0);
        set_position(&mut c, to);
        assert_eq!(invert_collapse(&mut c), 300);
        assert_eq!(bar(&c), Rect::new(0, 0, 8, 746));
    }

    #[test]
    fn panel_one_snaps_shut() {
        let c = split(&[("intMinSize1", "220"), ("intSplitterDim", "8")]);
        // total = 746 − 8 = 738.
        assert_eq!(clamp(&c, 600), 518);
        assert_eq!(clamp(&c, 700), 738);
    }

    #[test]
    fn resizing_keeps_the_far_panel_and_clamps() {
        // RootSkin's pad splitter (panel 1 = the pads, min 220) in a
        // 674 px column: the bar moves up with the bottom edge, so the pads
        // keep their size.
        let mut c = split(&[("intMinSize0", "66"), ("intMinSize1", "220"), ("intSplitPosition", "518"), ("intSplitterDim", "8")]);
        c.rect = Rect::new(0, 0, 300, 746);
        let old = c.rect;
        c.rect.h = 674;
        follow_resize(&mut c, old);
        assert_eq!(position(&c), 446);
        // Without double-click collapse panel 0 keeps its size, clamped:
        // the pads still get their minimum.
        let mut c = split(&[("intMinSize1", "220"), ("intSplitPosition", "518"), ("intSplitterDim", "8"), ("optDbClkCollapse1", "false")]);
        let old = c.rect;
        c.rect.h = 674;
        follow_resize(&mut c, old);
        assert_eq!(position(&c), 446);
        // Growing: panel 0 gets back the size chosen before the squeeze,
        // then stays.
        let old = c.rect;
        c.rect.h = 900;
        follow_resize(&mut c, old);
        assert_eq!(position(&c), 518);
        let old = c.rect;
        c.rect.h = 1000;
        follow_resize(&mut c, old);
        assert_eq!(position(&c), 518);
        // A position set since (a drag) is the one kept.
        set_position(&mut c, 300);
        let old = c.rect;
        c.rect.h = 600;
        follow_resize(&mut c, old);
        c.rect.h = 1000;
        follow_resize(&mut c, Rect::new(0, 0, 1390, 600));
        assert_eq!(position(&c), 300);
    }
}
