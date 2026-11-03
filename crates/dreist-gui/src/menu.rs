//! Popup menus ([re:071622], style `MenuStyle`): a list of items under
//! the control that opened it, drawn over everything else.

use crate::colour::Colour;
use crate::paint::{Painter, RectF};
use crate::style::{HAlign, StyleSheet, VAlign};

pub const MENU_STYLE: &str = "MenuStyle";
/// Leading marker of a slider item ([re:2b6ca2]): the rest
/// is the value 0..1.
pub const SLIDER: char = '\u{1}';
/// Slider item size ([re:2b6ca2] 120 × 15).
const SLIDER_W: f32 = 120.0;
const SLIDER_H: f32 = 15.0;

/// A slider item showing `v`.
pub fn slider_item(v: f32) -> String {
    format!("{SLIDER}{:.4}", v.clamp(0.0, 1.0))
}

/// Leading marker of a colour grid item ([re:2aab61]): the
/// rest is `cols;rows;` and the colours (`#rrggbb`, empty = transparent)
/// separated by `,`. A menu with one shows only the grid; its cells are
/// the selectable indices.
pub const GRID: char = '\u{3}';
/// Grid cell size and pitch ([re:2aab61] constructor [re:6db137]:
/// 12 × 12 cells every 14 px from (2, 2); item size cols·14 + 6 ×
/// rows·14 + 2, [re:ce04b6]).
const CELL: f32 = 12.0;
const PITCH: f32 = 14.0;

/// A colour grid item of `colours` (None = transparent) in `cols` columns.
pub fn colour_grid(colours: &[Option<Colour>], cols: usize) -> String {
    let rows = colours.len().div_ceil(cols.max(1));
    let list: Vec<String> = colours.iter().map(|c| c.map(|c| format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)).unwrap_or_default()).collect();
    format!("{GRID}{cols};{rows};{}", list.join(","))
}

struct Grid {
    cols: usize,
    rows: usize,
    colours: Vec<Option<Colour>>,
}

fn grid(item: &str) -> Option<Grid> {
    let rest = item.strip_prefix(GRID)?;
    let mut it = rest.splitn(3, ';');
    let cols = it.next()?.parse().ok()?;
    let rows = it.next()?.parse().ok()?;
    let colours =
        it.next()?.split(',').map(|c| u32::from_str_radix(c.trim_start_matches('#'), 16).ok().filter(|_| !c.is_empty()).map(|v| Colour::rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, 255))).collect();
    Some(Grid { cols, rows, colours })
}

/// The value of a slider item, if it is one.
pub fn slider_value(item: &str) -> Option<f32> {
    item.strip_prefix(SLIDER).and_then(|v| v.parse().ok())
}
/// Row height ([re:5bef62]: every row is at
/// least 20 px; text rows take max(font height, 20)).
const ITEM_H: f32 = 20.0;
/// Check item icon column ([re:cbc42a]
/// [re:02414f]: 20 × 20).
const CHECK_W: f32 = 20.0;
/// Sub menu indicator column ([re:006869]:
/// 20 when the item has a sub menu; the column is laid out 4 wider).
const SUB_W: f32 = 20.0;
/// Check mark prefix the editors put on checked items.
pub const CHECK: &str = "✓ ";
/// Prefix of a disabled item (drawn at alpha 0x80, not selectable).
pub const DISABLED: char = '\u{2}';

/// `text` as a disabled item when `!enabled`.
pub fn item(text: impl Into<String>, enabled: bool) -> String {
    let t = text.into();
    if enabled { t } else { format!("{DISABLED}{t}") }
}

fn is_disabled(item: &str) -> bool {
    item.starts_with(DISABLED)
}

/// A sub menu item (the editors end its text with "▸").
fn is_sub(item: &str) -> bool {
    item.trim_end().ends_with('▸')
}

/// The item's label without the check prefix or sub menu marker.
fn label(item: &str) -> &str {
    let item = item.strip_prefix(DISABLED).unwrap_or(item);
    let t = item.strip_prefix(CHECK).unwrap_or(item);
    t.trim_end().strip_suffix('▸').map(str::trim_end).unwrap_or(t)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    /// Path of the control that opened the menu.
    pub anchor: Vec<usize>,
    pub items: Vec<String>,
    pub checked: Option<usize>,
    /// Absolute position (logical px) and width.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub hover: Option<usize>,
    /// Attached menu (menus of popup buttons): only the bottom
    /// corners are rounded; popup menus round all four.
    pub attached: bool,
}

impl Menu {
    pub fn height(&self) -> f32 {
        if let Some(g) = self.grid() {
            return (g.rows as f32 * PITCH + 2.0).max(ITEM_H);
        }
        self.items.len() as f32 * ITEM_H
    }

    fn grid(&self) -> Option<Grid> {
        self.items.first().and_then(|i| grid(i))
    }

    /// Cell `i`'s rectangle of a colour grid menu.
    fn cell(&self, i: usize, cols: usize) -> RectF {
        let (c, r) = ((i % cols) as f32, (i / cols) as f32);
        RectF::new(self.x + self.text_x() + 2.0 + c * PITCH, self.y + 2.0 + r * PITCH, CELL, CELL)
    }

    /// Whether any row is a check item (they share a 20 px icon column).
    fn has_checks(&self) -> bool {
        self.checked.is_some() || self.items.iter().any(|i| i.starts_with(CHECK))
    }

    /// Left of the text column ([re:9f38a4]: icon at x = 2,
    /// text at icon width + 2).
    fn text_x(&self) -> f32 {
        if self.has_checks() { CHECK_W + 2.0 } else { 2.0 }
    }

    pub fn item_at(&self, x: f32, y: f32) -> Option<usize> {
        if x < self.x || x >= self.x + self.w || y < self.y {
            return None;
        }
        if let Some(g) = self.grid() {
            // [re:353782].
            return (0..g.colours.len()).find(|&i| {
                let r = self.cell(i, g.cols);
                x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
            });
        }
        let i = ((y - self.y) / ITEM_H) as usize;
        (i < self.items.len() && !self.items[i].is_empty() && !is_disabled(&self.items[i])).then_some(i)
    }

    /// Slider item `i`'s value for a pointer at `x`.
    pub fn slider_at(&self, i: usize, x: f32) -> Option<f32> {
        slider_value(self.items.get(i)?)?;
        let x0 = self.x + self.text_x();
        Some(((x - x0) / SLIDER_W).clamp(0.0, 1.0))
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.height()
    }

    /// [re:d71f90] and the items' highlight
    /// [re:3cd087] / icon / text [re:58bab5] /
    /// sub menu indicator [re:c0278b], [re:606f7b]
    /// [re:5d7174], [re:5954a7].
    pub fn draw(&self, styles: &StyleSheet, p: &mut Painter) {
        let fill = styles.colour(MENU_STYLE, "colFill", Colour::WHITE);
        let border = styles.colour(MENU_STYLE, "colBorder", Colour::BLACK);
        let hl = styles.colour(MENU_STYLE, "colAltFill", Colour::rgba(0, 228, 161, 255));
        let text = styles.colour(MENU_STYLE, "colText", Colour::BLACK);
        let divider = styles.colour(MENU_STYLE, "colDivider", border);
        let arrow = styles.colour(MENU_STYLE, "colSubMenuArrow", border);
        let radius = styles.float(MENU_STYLE, "realCornerRounding", 3.0);
        let font = styles.font(MENU_STYLE);
        let corners = if self.attached { [false, false, true, true] } else { [true; 4] };
        let r = RectF::new(self.x, self.y, self.w, self.height());
        // Shadow ([re:56990f] from the style's optDrawShadow).
        if styles.bool(MENU_STYLE, "optDrawShadow", false) {
            p.fill_rounded(RectF::new(r.x + 2.0, r.y + 3.0, r.w, r.h), radius, corners, Colour::rgba(0, 0, 0, 60));
        }
        p.fill_rounded(r, radius, corners, fill);
        if let Some(g) = self.grid() {
            // [re:43441c]: the colour (a red
            // cross when transparent), a black frame, the hovered cell
            // under white at alpha 0x78.
            for (i, c) in g.colours.iter().enumerate() {
                let cr = self.cell(i, g.cols);
                match c {
                    Some(c) => p.fill_rect(cr, *c),
                    None => {
                        let red = Colour::rgba(255, 0, 0, 255);
                        p.line(cr.x, cr.y, cr.x + cr.w - 1.0, cr.y + cr.h - 1.0, red, 1.0);
                        p.line(cr.x, cr.y + cr.h - 1.0, cr.x + cr.w - 1.0, cr.y, red, 1.0);
                    }
                }
                p.stroke_rect(cr.inset(0.5), Colour::BLACK, 1.0);
                if self.hover == Some(i) {
                    p.fill_rect(cr, Colour::rgba(255, 255, 255, 0x78));
                }
            }
            p.stroke_rounded(r.inset(0.5), radius, corners, border, 1.0);
            return;
        }
        let tx = self.text_x();
        let sub_x = self.w - (SUB_W + 4.0);
        for (i, item) in self.items.iter().enumerate() {
            let y = self.y + i as f32 * ITEM_H;
            if item.is_empty() {
                // Divider: (2, h/2) → (w − 4, h/2).
                let my = (y + ITEM_H / 2.0).floor() + 0.5;
                p.line(self.x + 2.0, my, self.x + self.w - 4.0, my, divider, 1.0);
                continue;
            }
            if let Some(v) = slider_value(item) {
                // `SliderStyle`: track colFill, value colAltFill.
                let st = "SliderStyle";
                let r = RectF::new(self.x + tx, y + (ITEM_H - SLIDER_H) / 2.0, SLIDER_W, SLIDER_H);
                p.fill_rounded(r, 3.0, [true; 4], styles.colour(st, "colFill", border));
                p.fill_rounded(RectF::new(r.x, r.y, r.w * v, r.h), 3.0, [true; 4], styles.colour(st, "colAltFill", hl));
                continue;
            }
            let lit = self.hover == Some(i);
            if lit {
                // Gradient from the highlight lightened 0.5 at (w/2, −2h) to
                // the highlight at (w/2, h), then a 1 px frame.
                let row = RectF::new(self.x, y, self.w, ITEM_H);
                let light = hl.lighten(0.5);
                let steps = ITEM_H as usize;
                for k in 0..steps {
                    let t = (2.0 * ITEM_H + k as f32 + 0.5) / (3.0 * ITEM_H);
                    p.fill_rect(RectF::new(row.x, row.y + k as f32, row.w, 1.0), mix(light, hl, t));
                }
                p.stroke_rect(row.inset(0.5), hl, 1.0);
            }
            if item.starts_with(CHECK) || self.checked == Some(i) {
                // Two 3 px strokes in the text colour.
                let (ix, iy) = (self.x + 2.0 + ((CHECK_W - 12.0) / 2.0).floor(), y + ((ITEM_H - 12.0) / 2.0).floor());
                p.line(ix + 1.0, iy + 5.0, ix + 5.0, iy + 10.0, text, 3.0);
                p.line(ix + 4.0, iy + 10.0, ix + 11.0, iy + 1.0, text, 3.0);
            }
            let tw = (sub_x - tx - 2.0).max(0.0);
            // Text colour: alpha 0x80 when disabled.
            let tc = if is_disabled(item) { text.with_alpha(text.f_alpha() * 128.0 / 255.0) } else { text };
            p.text(RectF::new(self.x + tx + 2.0, y, tw, ITEM_H), label(item), tc, &font, HAlign::Left, VAlign::Center);
            if is_sub(item) {
                // Triangle (x + 4, 4), (x + w − 8, h/2), (x + 4, h − 4).
                let ax = self.x + sub_x;
                let col = if lit { text } else { arrow };
                p.fill_polygon(&[(ax + 4.0, y + 4.0), (ax + SUB_W + 4.0 - 8.0, y + ITEM_H / 2.0), (ax + 4.0, y + ITEM_H - 4.0)], col);
            }
        }
        p.stroke_rounded(r.inset(0.5), radius, corners, border, 1.0);
    }
}

/// Width needed for `items` in the menu font: icon column (20 when any item
/// is a check item) + text column (widest text + 10) + sub menu column
/// (24 when any item has a sub menu), at least `min`.
pub fn menu_width(styles: &StyleSheet, fonts: &mut crate::text::Fonts, items: &[String], min: f32) -> f32 {
    if let Some(g) = items.first().and_then(|i| grid(i)) {
        return (g.cols as f32 * PITCH + 6.0 + 4.0).max(min.min(g.cols as f32 * PITCH + 10.0));
    }
    let font = styles.font(MENU_STYLE);
    let checks = items.iter().any(|i| i.starts_with(CHECK));
    let text = items
        .iter()
        .map(|s| {
            if slider_value(s).is_some() {
                SLIDER_W
            } else if s.is_empty() {
                0.0
            } else {
                fonts.measure(&font, label(s)).0 + 10.0
            }
        })
        .fold(0.0, f32::max);
    let sub = if items.iter().any(|s| is_sub(s)) { SUB_W + 4.0 } else { 0.0 };
    (if checks { CHECK_W } else { 0.0 } + text + sub + 2.0).max(min)
}

fn mix(a: Colour, b: Colour, t: f32) -> Colour {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Colour::rgba(l(a.r, b.r), l(a.g, b.g), l(a.b, b.b), l(a.a, b.a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_grid_cells() {
        let mut colours = vec![None];
        colours.extend((1..64).map(|i| Some(Colour::rgba(i * 4, 0, 0, 255))));
        let item = colour_grid(&colours, 8);
        let m = Menu { anchor: vec![], items: vec![item], checked: None, x: 100.0, y: 50.0, w: 122.0, hover: None, attached: false };
        // 8 rows of 14 px + 2.
        assert_eq!(m.height(), 114.0);
        // Cell 0 at (x + 4, y + 2), 12 × 12, every 14 px.
        assert_eq!(m.item_at(105.0, 53.0), Some(0));
        assert_eq!(m.item_at(105.0 + 14.0 * 3.0, 53.0 + 14.0 * 2.0), Some(19));
        assert_eq!(m.item_at(104.0 + 12.5, 53.0), None);
        let g = grid(&m.items[0]).unwrap();
        assert_eq!((g.cols, g.rows, g.colours.len()), (8, 8, 64));
        assert_eq!(g.colours[0], None);
        assert_eq!(g.colours[2], Some(Colour::rgba(8, 0, 0, 255)));
    }
}
