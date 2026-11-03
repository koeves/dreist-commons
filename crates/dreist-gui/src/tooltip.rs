//! Tooltips ([re:655f9e]).
//!
//! - [re:eb2a9b] asks the control under the pointer
//!   (then its parents) for text on every move, press and wheel event
//!   (`UpdateToolTipForEvent` [re:45d114]) and shows it at once
//!   (`SetToolTipText(.., immediate = true, ..)` [re:d57db3] →
//!   `ShowToolTip` [re:13518c] → `AppearToolTip` [re:4c11d7]); a control
//!   without a provider hides it, as does leaving the window.
//! - `AppearToolTip`: unchanged text while shown does nothing; else the
//!   size is the text size + (10, 5) (multi-line: widest line, line
//!   heights + 5 apart), at least 20 × 20, placed against the control's
//!   rectangle by [`Pos`], kept inside the window (x ≥ 0), and starts the
//!   auto-hide count: 150 ticks of the 30 Hz [re:6556de] (5 s).
//! - `OnMouseLButtonUp` → `HideToolTipDelayed` [re:da2cdc]: hidden now for
//!   [`Pos::Middle`], else after 100 ticks (3.3 s).
//! - `Draw` [re:cc7150]: `DrawDefaultFill` of `TooltipStyle`, the text in its
//!   colText / font, centred; lines share the height evenly.

use std::time::{Duration, Instant};

use crate::colour::Colour;
use crate::ctrl::Rect;
use crate::paint::{Painter, RectF};
use crate::style::{HAlign, StyleSheet, VAlign};

pub const STYLE: &str = "TooltipStyle";
/// `+0x2a8` / `+0x2ac`: padding added to the text size.
const PAD: (f32, f32) = (10.0, 5.0);
const MIN: f32 = 20.0;
const TICK: f32 = 1.0 / 30.0;
/// `AppearToolTip`: `+0x2a4 = 0x96`.
pub const AUTO_HIDE: Duration = Duration::from_millis((150.0 * TICK * 1000.0) as u64);
/// `ShowToolTip`'s appear count (`+0x268 = 0x32`).
pub const SHOW_DELAY: Duration = Duration::from_millis((50.0 * TICK * 1000.0) as u64);
/// `HideToolTipDelayed`: `+0x2a4 = 0x64`.
pub const DELAYED_HIDE: Duration = Duration::from_millis((100.0 * TICK * 1000.0) as u64);

/// [re:25cf21] (`AppearToolTip` switch).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Pos {
    /// Centred above (5 px gap).
    Above,
    /// Above, starting at the right edge.
    AboveRight,
    /// Right of the control, 10 px above its middle.
    Right,
    /// Below, starting at the right edge.
    BelowRight,
    /// Centred below (the default).
    #[default]
    Below,
    /// Below, left aligned.
    BelowLeft,
    /// Left of the control, 10 px above its middle.
    Left,
    /// Above, left aligned, no gap.
    AboveLeft,
    /// Over the control's left edge, 10 px above its middle.
    Middle,
}

impl Pos {
    pub fn from_index(i: u32) -> Pos {
        [Pos::Above, Pos::AboveRight, Pos::Right, Pos::BelowRight, Pos::Below, Pos::BelowLeft, Pos::Left, Pos::AboveLeft, Pos::Middle].get(i as usize).copied().unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tip {
    pub text: String,
    /// Root coordinates.
    pub rect: Rect,
    pub pos: Pos,
    /// When it disappears.
    pub hide_at: Instant,
    /// A delayed tooltip (`ShowToolTip(.., immediate = false, ..)`) not yet
    /// shown: when it appears and where the pointer was (a move of more
    /// than 4 px restarts the 50 ticks, `OnMouseMoveTT` [re:939074]).
    pub pending: Option<(Instant, (f32, f32))>,
}

/// Text size as `AppearToolTip` measures it.
fn measure(fonts: &mut crate::text::Fonts, styles: &StyleSheet, text: &str) -> (f32, f32) {
    let font = styles.font(STYLE);
    if !text.contains('\n') {
        return fonts.measure(&font, text);
    }
    let (mut w, mut h) = (0.0f32, 0.0f32);
    for (i, line) in text.split('\n').enumerate() {
        let (lw, lh) = fonts.measure(&font, line);
        w = w.max(lw);
        h = if i == 0 { lh } else { h + lh + PAD.1 };
    }
    (w, h)
}

/// The tooltip's rectangle for `text` at control rectangle `a` in a
/// window of `root` size.
pub fn layout(fonts: &mut crate::text::Fonts, styles: &StyleSheet, text: &str, a: Rect, pos: Pos, root: (i32, i32)) -> Rect {
    let (tw, th) = measure(fonts, styles, text);
    let (w, h) = ((tw.ceil() + PAD.0) as i32, (th.ceil() + PAD.1) as i32);
    let (bw, bh) = (w.max(MIN as i32), h.max(MIN as i32));
    let mid = a.x + a.w / 2;
    let (mut x, mut y) = match pos {
        Pos::Above => (mid - w / 2, a.y - 5 - h),
        Pos::AboveRight => (a.x + a.w, a.y - 5 - h),
        Pos::Right => (a.x + a.w, a.y + a.h / 2 - 10),
        Pos::BelowRight => (a.x + a.w, a.y + a.h),
        Pos::Below => (mid - w / 2, a.y + a.h),
        Pos::BelowLeft => (a.x, a.y + a.h),
        Pos::Left => (a.x - w, a.y + a.h / 2 - 10),
        Pos::AboveLeft => (a.x, a.y - h),
        Pos::Middle => (a.x, a.y + a.h / 2 - 10),
    };
    if x + bw > root.0 {
        x = root.0 - bw;
    }
    x = x.max(0);
    if y + bh > root.1 {
        y = root.1 - bh;
    }
    Rect::new(x, y, bw, bh)
}

impl Tip {
    pub fn visible(&self) -> bool {
        self.pending.is_none()
    }

    pub fn draw(&self, styles: &StyleSheet, p: &mut Painter) {
        let r = RectF::new(self.rect.x as f32, self.rect.y as f32, self.rect.w as f32, self.rect.h as f32);
        let fill = styles.colour(STYLE, "colFill", Colour::WHITE);
        let border = styles.colour(STYLE, "colBorder", Colour::BLACK);
        let text = styles.colour(STYLE, "colText", Colour::BLACK);
        let radius = styles.float(STYLE, "realCornerRounding", 0.0).max(0.0);
        if styles.bool(STYLE, "optDrawShadow", false) {
            p.fill_rounded(RectF::new(r.x + 2.0, r.y + 3.0, r.w, r.h), radius.max(0.5), [true; 4], Colour::rgba(0, 0, 0, 60));
        }
        if radius > 0.0 {
            p.fill_rounded(r, radius, [true; 4], fill);
            p.stroke_rounded(r.inset(0.5), radius, [true; 4], border, 1.0);
        } else {
            p.fill_rect(r, fill);
            p.stroke_rect(r.inset(0.5).pixel_align(), border, 1.0);
        }
        let font = styles.font(STYLE);
        let lines: Vec<&str> = self.text.split('\n').collect();
        let lh = r.h / lines.len() as f32;
        for (i, line) in lines.iter().enumerate() {
            p.text(RectF::new(r.x, r.y + i as f32 * lh, r.w, lh), line, text, &font, HAlign::Center, VAlign::Center);
        }
    }

    /// The area it covers, shadow included.
    pub fn damage(&self) -> Rect {
        Rect::new(self.rect.x - 1, self.rect.y - 1, self.rect.w + 5, self.rect.h + 6)
    }
}
