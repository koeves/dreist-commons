//! Scroll containers (`CtrlScrollContainer`), opt-in.
//!
//! A container scrolls once the editor that fills it marks it with
//! `scrollAxes` ("v", "h" or "hv"); containers without it stay as they
//! were (an app that never sets it sees no change).
//!
//! - Children keep their rects in view coordinates: content position
//!   minus the scroll offset (`scrollX` / `scrollY`). [`place`] takes
//!   freshly placed (unscrolled) children, [`scroll_to`] moves placed
//!   ones.
//! - [re:3c0f5e]: scroll bars from the skin attributes (see `bars`),
//!   `intScrollBarDim` wide (default 15) along the right / bottom edge,
//!   `VScrollBarStyle` / `HScrollBarStyle`. The bars are `CtrlScrollBar`
//!   children (`scrollBar`
//!   "v" / "h") above the content, left out of the content and of
//!   restacking. `optOverlay` is false in every shipped use, so bars
//!   take their room from the view.
//! - [re:5e1a7d]: the wheel scrolls the vertical bar, shift + wheel the
//!   horizontal one. A container with only a horizontal axis also takes
//!   the plain wheel sideways, and a trackpad's sideways swipe scrolls
//!   horizontally. Line deltas move `scrollStep` px per line (default 20),
//!   pixel deltas (trackpads) move by their pixels.
use crate::ctrl::{Ctrl, Rect};

/// The bar controls' marker attribute.
pub const BAR: &str = "scrollBar";

/// True when the editor made `c` scroll.
pub fn scrollable(c: &Ctrl) -> bool {
    c.attr("scrollAxes").is_some()
}

fn axes(c: &Ctrl) -> (bool, bool) {
    let a = c.attr("scrollAxes").unwrap_or("");
    (a.contains('h'), a.contains('v'))
}

/// The current offset (x, y).
pub fn offset(c: &Ctrl) -> (i32, i32) {
    (c.attr_i32("scrollX", 0), c.attr_i32("scrollY", 0))
}

fn is_bar(c: &Ctrl) -> bool {
    c.attr(BAR).is_some()
}

fn dim(c: &Ctrl) -> i32 {
    c.attr_i32("intScrollBarDim", 15).max(0)
}

/// Content size (unscrolled) of the children other than the bars.
pub fn content(c: &Ctrl) -> (i32, i32) {
    let (sx, sy) = offset(c);
    c.children.iter().filter(|k| !is_bar(k) && k.visible).fold((0, 0), |(w, h), k| (w.max(k.rect.right() + sx), h.max(k.rect.bottom() + sy)))
}

/// Which bars show for a content size: (horizontal, vertical).
///
/// [re:3c0f5e] (`NeedsScrollbars`): `optShowVBar` / `optShowHBar` (default
/// true) show a bar always, else only while the content overflows; with
/// `optAutoHide` (default false) each bar shows only while needed, the
/// other bar's room counted. Here only the editor's `scrollAxes` get bars,
/// and an axis whose bar style is set but empty scrolls without a drawn
/// bar (the effect chain; the class default style draws nothing visible).
fn bars(c: &Ctrl, cw: i32, ch: i32) -> (bool, bool) {
    let (ah, av) = axes(c);
    let styled = |k: &str| c.attr(k) != Some("");
    let (bh, bv) = (ah && styled("HScrollBarStyle"), av && styled("VScrollBarStyle"));
    let (w, h, d) = (c.rect.w, c.rect.h, dim(c));
    let (show_h, show_v) = (c.attr_bool("optShowHBar", true), c.attr_bool("optShowVBar", true));
    if !c.attr_bool("optAutoHide", false) {
        return (bh && (show_h || cw > w), bv && (show_v || ch > h));
    }
    let need_h = cw > w;
    let need_v = ch > h - if need_h { d } else { 0 };
    let need_h = need_h || cw > w - if need_v { d } else { 0 };
    (bh && need_h, bv && need_v)
}

/// The view (the container minus its bars), in the container's own
/// coordinates.
pub fn view(c: &Ctrl) -> Rect {
    if !scrollable(c) {
        return Rect::new(0, 0, c.rect.w, c.rect.h);
    }
    let (cw, ch) = content(c);
    view_for(c, cw, ch)
}

fn view_for(c: &Ctrl, cw: i32, ch: i32) -> Rect {
    let (sh, sv) = bars(c, cw, ch);
    let d = dim(c);
    Rect::new(0, 0, (c.rect.w - if sv { d } else { 0 }).max(0), (c.rect.h - if sh { d } else { 0 }).max(0))
}

/// The view a filled container will have, for laying its children out
/// before they are added (the bars it shows when the content is
/// `content` large).
pub fn view_for_content(c: &Ctrl, content: (i32, i32)) -> Rect {
    view_for(c, content.0, content.1)
}

/// The children were just placed at content coordinates: move them to the
/// current offset (clamped) and lay the bars out.
pub fn place(c: &mut Ctrl) {
    if !scrollable(c) {
        return;
    }
    span(c);
    let (sx, sy) = offset(c);
    c.set_attr("scrollX", "0".into());
    c.set_attr("scrollY", "0".into());
    let (cw, ch) = content(c);
    let (nx, ny) = clamp(c, cw, ch, sx, sy);
    shift(c, -nx, -ny);
    c.set_attr("scrollX", nx.to_string());
    c.set_attr("scrollY", ny.to_string());
    layout_bars(c, cw, ch);
}

/// A one-axis container's children span the view across the axis (as
/// the stack box the original scrolls does).
fn span(c: &mut Ctrl) {
    let (ah, av) = axes(c);
    if ah == av {
        return;
    }
    let (cw, ch) = content(c);
    let v = view_for(c, cw, ch);
    for k in c.children.iter_mut().filter(|k| !is_bar(k)) {
        let mut r = k.rect;
        if av {
            r.w = (v.w - r.x).max(0);
        } else {
            r.h = (v.h - r.y).max(0);
        }
        if r != k.rect {
            k.set_rect(r);
            if let Some(a) = k.anchored.as_mut() {
                a.placed = r;
            }
        }
    }
}

fn clamp(c: &Ctrl, cw: i32, ch: i32, x: i32, y: i32) -> (i32, i32) {
    let v = view_for(c, cw, ch);
    (x.clamp(0, (cw - v.w).max(0)), y.clamp(0, (ch - v.h).max(0)))
}

fn shift(c: &mut Ctrl, dx: i32, dy: i32) {
    if dx == 0 && dy == 0 {
        return;
    }
    for k in c.children.iter_mut().filter(|k| !is_bar(k)) {
        k.rect.x += dx;
        k.rect.y += dy;
        if let Some(a) = k.anchored.as_mut() {
            a.placed = k.rect;
        }
    }
}

/// Scroll to (x, y) (clamped); true when it moved.
pub fn scroll_to(c: &mut Ctrl, x: i32, y: i32) -> bool {
    if !scrollable(c) {
        return false;
    }
    let (sx, sy) = offset(c);
    let (cw, ch) = content(c);
    let (nx, ny) = clamp(c, cw, ch, x, y);
    shift(c, sx - nx, sy - ny);
    c.set_attr("scrollX", nx.to_string());
    c.set_attr("scrollY", ny.to_string());
    layout_bars(c, cw, ch);
    (nx, ny) != (sx, sy)
}

/// Re-clamp and re-lay the bars (after a resize or a content change).
pub fn refresh(c: &mut Ctrl) {
    if !scrollable(c) {
        return;
    }
    span(c);
    let (x, y) = offset(c);
    scroll_to(c, x, y);
}

/// Can `c` move by (dx, dy) at all?
pub fn can_scroll(c: &Ctrl, dx: i32, dy: i32) -> bool {
    let (sx, sy) = offset(c);
    let (cw, ch) = content(c);
    clamp(c, cw, ch, sx + dx, sy + dy) != (sx, sy)
}

/// A wheel / trackpad event: `dx`, `dy` positive = right / down content
/// moves into view (a wheel turned down, a swipe left). `lines`: line
/// deltas (× `scrollStep`), else pixels. Returns the movement applied.
pub fn wheel(c: &mut Ctrl, dx: f32, dy: f32, lines: bool, shift_key: bool) -> bool {
    let (ah, av) = axes(c);
    let step = if lines { c.attr_f32("scrollStep", 20.0) } else { 1.0 };
    let (mut mx, mut my) = (dx, dy);
    // Shift turns the wheel sideways; a container that only scrolls
    // sideways takes the plain wheel that way too.
    if (shift_key || !av) && mx == 0.0 {
        mx = my;
        my = 0.0;
    }
    if !ah {
        mx = 0.0;
    }
    if !av {
        my = 0.0;
    }
    let (px, py) = ((mx * step).round() as i32, (my * step).round() as i32);
    if px == 0 && py == 0 {
        return false;
    }
    let (sx, sy) = offset(c);
    scroll_to(c, sx + px, sy + py)
}

fn layout_bars(c: &mut Ctrl, cw: i32, ch: i32) {
    let (sh, sv) = bars(c, cw, ch);
    let d = dim(c);
    let v = view_for(c, cw, ch);
    let (sx, sy) = offset(c);
    for (axis, show) in [("h", sh), ("v", sv)] {
        let i = match c.children.iter().position(|k| k.attr(BAR) == Some(axis)) {
            Some(i) => i,
            None if !show => continue,
            None => {
                let mut b =
                    Ctrl { class: "CtrlScrollBar".into(), name: if axis == "h" { "HScrollBar" } else { "VScrollBar" }.into(), visible: true, enabled: true, zorder: i16::MAX, ..Ctrl::default() };
                b.style = c.attr(if axis == "h" { "HScrollBarStyle" } else { "VScrollBarStyle" }).filter(|s| !s.is_empty()).unwrap_or("ScrollbarStyle").to_string();
                b.set_attr(BAR, axis.into());
                c.children.push(b);
                c.children.len() - 1
            }
        };
        let (len, view_len, pos) = if axis == "h" { (cw, v.w, sx) } else { (ch, v.h, sy) };
        let b = &mut c.children[i];
        b.visible = show;
        b.rect = if axis == "h" { Rect::new(0, v.h, v.w, d) } else { Rect::new(v.w, 0, d, v.h) };
        let page = if len > 0 { (view_len as f32 / len as f32).min(1.0) } else { 1.0 };
        b.set_attr("realPage", page.to_string());
        let range = (len - view_len).max(0);
        b.value = if range > 0 { pos as f32 / range as f32 } else { 0.0 };
    }
}

/// A bar was dragged to `value` (0..1): scroll its container there.
pub fn from_bar(c: &mut Ctrl, axis: &str, value: f32) -> bool {
    let (cw, ch) = content(c);
    let v = view_for(c, cw, ch);
    let (sx, sy) = offset(c);
    if axis == "h" { scroll_to(c, (value * (cw - v.w).max(0) as f32).round() as i32, sy) } else { scroll_to(c, sx, (value * (ch - v.h).max(0) as f32).round() as i32) }
}

/// A press on a bar's track outside the thumb at `at` (bar coordinates
/// along its axis): one view length towards it.
pub fn page(c: &mut Ctrl, axis: &str, at: i32) -> bool {
    let Some(b) = c.children.iter().find(|k| k.attr(BAR) == Some(axis)) else { return false };
    let page = b.attr_f32("realPage", 1.0).clamp(0.0, 1.0);
    let len = if axis == "h" { b.rect.w } else { b.rect.h } as f32;
    let th = (len * page).max(8.0).min(len);
    let start = b.value.clamp(0.0, 1.0) * (len - th);
    let dir = if (at as f32) < start {
        -1
    } else if (at as f32) > start + th {
        1
    } else {
        return false;
    };
    let (cw, ch) = content(c);
    let v = view_for(c, cw, ch);
    let (sx, sy) = offset(c);
    if axis == "h" { scroll_to(c, sx + dir * v.w, sy) } else { scroll_to(c, sx, sy + dir * v.h) }
}

/// Re-lay every scroll container in the tree (after a resize).
pub fn refresh_all(c: &mut Ctrl) {
    for k in &mut c.children {
        refresh_all(k);
    }
    if scrollable(c) {
        refresh(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container(axes: &str, w: i32, h: i32, kids: &[(i32, i32, i32, i32)]) -> Ctrl {
        let mut c = Ctrl { class: "CtrlScrollContainer".into(), rect: Rect::new(0, 0, w, h), visible: true, ..Ctrl::default() };
        c.set_attr("scrollAxes", axes.into());
        c.set_attr("intScrollBarDim", "12".into());
        for &(x, y, w, h) in kids {
            c.children.push(Ctrl { rect: Rect::new(x, y, w, h), visible: true, ..Ctrl::default() });
        }
        place(&mut c);
        c
    }

    fn bar<'a>(c: &'a Ctrl, axis: &str) -> &'a Ctrl {
        c.children.iter().find(|k| k.attr(BAR) == Some(axis)).unwrap()
    }

    #[test]
    fn wheel_lines_scroll_and_clamp() {
        // Five 135 px panels 2 apart in a 390 x 300 view.
        let kids: Vec<_> = (0..5).map(|i| (0, i * 137, 378, 135)).collect();
        let mut c = container("v", 390, 300, &kids);
        let content_h = 4 * 137 + 135;
        assert_eq!(content(&c), (378, content_h));
        assert!(bar(&c, "v").visible);
        assert_eq!(bar(&c, "v").rect, Rect::new(378, 0, 12, 300));
        // Up at the top: nothing.
        assert!(!wheel(&mut c, 0.0, -1.0, true, false));
        assert!(wheel(&mut c, 0.0, 2.0, true, false));
        assert_eq!(offset(&c), (0, 40));
        assert_eq!(c.children[1].rect.y, 137 - 40);
        // Far down: clamped to content − view.
        wheel(&mut c, 0.0, 100.0, true, false);
        assert_eq!(offset(&c), (0, content_h - 300));
        assert_eq!(bar(&c, "v").value, 1.0);
        assert!(!can_scroll(&c, 0, 1));
        // The thumb's page is the visible fraction.
        assert!((bar(&c, "v").attr_f32("realPage", 0.0) - 300.0 / content_h as f32).abs() < 1e-6);
    }

    #[test]
    fn trackpad_pixels_and_shift_wheel_go_sideways() {
        let mut c = container("h", 500, 188, &[(0, 0, 939, 176)]);
        c.set_attr("optShowVBar", "false".into());
        refresh(&mut c);
        assert!(bar(&c, "h").visible);
        assert_eq!(bar(&c, "h").rect, Rect::new(0, 176, 500, 12));
        // Trackpad sideways swipe, pixels.
        assert!(wheel(&mut c, 13.4, 0.0, false, false));
        assert_eq!(offset(&c), (13, 0));
        // Shift + wheel.
        wheel(&mut c, 0.0, 1.0, true, true);
        assert_eq!(offset(&c), (33, 0));
        // A horizontal-only container takes the plain wheel sideways.
        wheel(&mut c, 0.0, -1.0, true, false);
        assert_eq!(offset(&c), (13, 0));
        wheel(&mut c, -100.0, 0.0, false, false);
        assert_eq!(offset(&c), (0, 0));
        wheel(&mut c, 0.0, 1000.0, false, true);
        assert_eq!(offset(&c), (939 - 500, 0));
        assert_eq!(c.children[0].rect.x, -(939 - 500));
    }

    #[test]
    fn bars_show_always_unless_auto_hidden_and_empty_styles_draw_none() {
        // optAutoHide off (the default): the bar shows even when all fits.
        let mut c = container("v", 300, 300, &[(0, 0, 288, 100)]);
        assert!(bar(&c, "v").visible);
        assert_eq!(view(&c), Rect::new(0, 0, 288, 300));
        assert!(!wheel(&mut c, 0.0, 1.0, true, false));
        c.set_attr("optAutoHide", "true".into());
        refresh(&mut c);
        assert!(!bar_exists(&c, "v"));
        assert_eq!(view(&c), Rect::new(0, 0, 300, 300));
        // optShowVBar false: only while the content overflows.
        let mut c = container("v", 300, 300, &[(0, 0, 288, 100)]);
        c.set_attr("optShowVBar", "false".into());
        refresh(&mut c);
        assert!(!bar_exists(&c, "v"));
        c.children.push(Ctrl { rect: Rect::new(0, 100, 288, 400), visible: true, ..Ctrl::default() });
        refresh(&mut c);
        assert!(bar_exists(&c, "v"));
        // A set but empty bar style: scrolls, no bar.
        let mut c = container("h", 300, 194, &[(0, 0, 500, 182)]);
        c.set_attr("HScrollBarStyle", String::new());
        refresh(&mut c);
        assert!(!bar_exists(&c, "h"));
        assert!(wheel(&mut c, 30.0, 0.0, false, false));
        assert_eq!(offset(&c), (30, 0));
    }

    fn bar_exists(c: &Ctrl, axis: &str) -> bool {
        c.children.iter().any(|k| k.attr(BAR) == Some(axis) && k.visible)
    }

    #[test]
    fn bar_drag_and_paging() {
        let kids: Vec<_> = (0..4).map(|i| (0, i * 100, 288, 100)).collect();
        let mut c = container("v", 300, 200, &kids);
        assert!(from_bar(&mut c, "v", 0.5));
        assert_eq!(offset(&c), (0, 100));
        // Below the thumb: one page down (clamped at the end).
        assert!(page(&mut c, "v", 199));
        assert_eq!(offset(&c), (0, 200));
        assert!(page(&mut c, "v", 0));
        assert_eq!(offset(&c), (0, 0));
    }

    #[test]
    fn place_keeps_the_offset_and_shrinking_content_clamps_it() {
        let kids: Vec<_> = (0..4).map(|i| (0, i * 100, 288, 100)).collect();
        let mut c = container("v", 300, 200, &kids);
        scroll_to(&mut c, 0, 150);
        // Refill with less content at content coordinates.
        c.children.retain(is_bar);
        c.children.push(Ctrl { rect: Rect::new(0, 0, 288, 100), visible: true, ..Ctrl::default() });
        c.children.push(Ctrl { rect: Rect::new(0, 100, 288, 150), visible: true, ..Ctrl::default() });
        place(&mut c);
        assert_eq!(offset(&c), (0, 50));
        assert_eq!(c.children.iter().filter(|k| !is_bar(k)).map(|k| k.rect.y).collect::<Vec<_>>(), vec![-50, 50]);
    }

    #[test]
    fn containers_without_the_marker_do_nothing() {
        let mut c = Ctrl { rect: Rect::new(0, 0, 100, 100), ..Ctrl::default() };
        c.children.push(Ctrl { rect: Rect::new(0, 0, 100, 400), visible: true, ..Ctrl::default() });
        place(&mut c);
        assert!(!scroll_to(&mut c, 0, 50));
        assert_eq!(c.children.len(), 1);
        assert_eq!(c.children[0].rect.y, 0);
    }
}
