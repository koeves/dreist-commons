//! The CPU and memory readout: two rows ("DSP 31% · peak 58%" over
//! "UI 14% · 351 MB · 0 late") over everything else, passing clicks
//! through, at the bottom left of an area the app names ([`place`]). Apps
//! add it with [`show`], keep it placed every frame and feed it once a
//! second with [`update`].

use crate::app::App;
use crate::paint::{Painter, RectF};
use crate::style::{FontSpec, HAlign, VAlign};
use crate::{Colour, Ctrl, Rect};

/// The readout control's name and overload (drawn by the renderer).
pub const NAME: &str = "PerfReadout";

/// One row's height, the readout's widest width and its inset from the
/// area's edges.
const ROW: i32 = 16;
const W: i32 = 170;
const INSET: i32 = 4;

/// The readout's rectangle at the bottom left of `area` (root
/// coordinates); empty when the area is too small to hold it.
pub fn rect_in(area: Rect) -> Rect {
    let (w, h) = ((area.w - 2 * INSET).min(W), 2 * ROW);
    if w < 120 || area.h < h + 2 * INSET {
        return Rect::new(area.x, area.y, 0, 0);
    }
    Rect::new(area.x + INSET, area.y + area.h - INSET - h, w, h)
}

/// What the readout shows; `None` parts are left out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    /// Audio processing time over audio time, for the last second.
    pub dsp: Option<f32>,
    /// The slowest block of that second, over its own duration.
    pub peak: Option<f32>,
    /// Share of a core the window's frames took.
    pub ui: Option<f32>,
    /// Resident memory in bytes.
    pub memory: Option<u64>,
    /// Late audio blocks (dropouts) since the start.
    pub late: Option<u32>,
}

impl Reading {
    pub fn text(&self) -> String {
        let [a, b] = self.rows();
        [a, b].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
    }

    /// The text in two rows: the audio load, then UI, memory and late
    /// blocks.
    pub fn rows(&self) -> [String; 2] {
        let pc = |v: f32| format!("{:.0}%", v * 100.0);
        let mut dsp = Vec::new();
        if let Some(d) = self.dsp {
            dsp.push(format!("DSP {}", pc(d)));
        }
        if let Some(p) = self.peak {
            dsp.push(format!("peak {}", pc(p)));
        }
        let mut parts = Vec::new();
        if let Some(u) = self.ui {
            parts.push(format!("UI {}", pc(u)));
        }
        if let Some(m) = self.memory {
            parts.push(format!("{} MB", m / (1 << 20)));
        }
        if let Some(l) = self.late {
            parts.push(format!("{l} late"));
        }
        [dsp.join(" · "), parts.join(" · ")]
    }

    /// 0 fine, 1 close to the limit (peak 70 %), 2 at it (90 %).
    pub fn level(&self) -> u8 {
        match self.peak.or(self.dsp).unwrap_or(0.0) {
            p if p >= 0.9 => 2,
            p if p >= 0.7 => 1,
            _ => 0,
        }
    }
}

fn path(app: &App) -> Option<usize> {
    app.root.children.iter().position(|c| c.name == NAME)
}

/// Add the readout (placed by [`place`]), or remove it.
pub fn show(app: &mut App, on: bool) {
    match (path(app), on) {
        (None, true) => {
            app.root.children.push(Ctrl { class: "CtrlPanel".into(), name: NAME.into(), overload: NAME.into(), visible: true, passive: true, zorder: i16::MAX, ..Ctrl::default() });
            app.dirty = true;
        }
        (Some(i), false) => {
            app.root.children.remove(i);
            app.dirty = true;
        }
        _ => {}
    }
}

/// Keep the readout at the bottom left of `area` (root coordinates, see
/// [`rect_in`]); a move repaints the old and new places.
pub fn place(app: &mut App, area: Rect) {
    let Some(i) = path(app) else { return };
    let rect = rect_in(area);
    let c = &mut app.root.children[i];
    if c.rect == rect {
        return;
    }
    let old = std::mem::replace(&mut c.rect, rect);
    c.visible = rect.w > 0;
    repaint(app, old);
    repaint(app, rect);
}

fn repaint(app: &mut App, r: Rect) {
    if r.w > 0 && r.h > 0 {
        let (ox, oy) = (app.root.rect.x, app.root.rect.y);
        app.invalidate_rect(Rect::new(r.x + ox, r.y + oy, r.w, r.h));
    }
}

/// Show `r` (when the readout is on); repaints only the readout.
pub fn update(app: &mut App, r: &Reading) {
    let Some(i) = path(app) else { return };
    let (text, level) = (r.rows().join("\n"), r.level().to_string());
    let c = &mut app.root.children[i];
    if c.text == text && c.attr("level") == Some(level.as_str()) {
        return;
    }
    c.text = text;
    c.set_attr("level", level);
    let rect = c.rect;
    repaint(app, rect);
}

/// Draw the readout control: its text on a dark strip, amber or red near
/// the limit.
pub fn draw(p: &mut Painter, c: &Ctrl) {
    if c.text.is_empty() {
        return;
    }
    let font = FontSpec { family: crate::text::ui_family().into(), size: 11.0, bold: false };
    p.fonts.prepare(&font);
    let lines: Vec<&str> = c.text.split('\n').collect();
    let tw = lines.iter().map(|l| p.fonts.measure(&font, l).0).fold(0.0, f32::max).ceil() + 10.0;
    let (w, h) = (c.rect.w as f32, c.rect.h as f32);
    let bg = RectF::new(0.0, 0.0, tw.min(w), h);
    p.fill_rounded(bg, 4.0, [true; 4], Colour::rgba(16, 18, 24, 235));
    let col = match c.attr("level") {
        Some("2") => Colour::rgba(255, 92, 80, 255),
        Some("1") => Colour::rgba(255, 190, 60, 255),
        _ => Colour::rgba(220, 226, 236, 255),
    };
    let row = h / lines.len() as f32;
    for (k, l) in lines.iter().enumerate() {
        p.text(RectF::new(bg.x + 5.0, k as f32 * row, bg.w - 10.0, row), l, col, &font, HAlign::Center, VAlign::Center);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_lists_what_is_known() {
        let r = Reading { dsp: Some(0.312), peak: Some(0.58), ui: Some(0.14), memory: Some(351 << 20), late: Some(0) };
        assert_eq!(r.text(), "DSP 31% · peak 58% · UI 14% · 351 MB · 0 late");
        assert_eq!(r.rows(), ["DSP 31% · peak 58%".to_string(), "UI 14% · 351 MB · 0 late".to_string()]);
        assert_eq!(r.level(), 0);
        assert_eq!(Reading { ui: Some(0.2), ..Reading::default() }.text(), "UI 20%");
        assert_eq!(Reading { peak: Some(0.75), ..Reading::default() }.level(), 1);
        assert_eq!(Reading { peak: Some(0.95), ..Reading::default() }.level(), 2);
    }

    #[test]
    fn the_readout_sits_inside_the_bottom_left_of_its_area() {
        let area = Rect::new(7, 81, 295, 523);
        let r = rect_in(area);
        assert_eq!(r, Rect::new(11, 81 + 523 - 4 - 32, 170, 32));
        assert!(r.x >= area.x && r.y >= area.y && r.x + r.w <= area.x + area.w && r.y + r.h <= area.y + area.h);
        // Too small an area: nothing.
        assert_eq!(rect_in(Rect::new(0, 0, 100, 400)).w, 0);
        assert_eq!(rect_in(Rect::new(0, 0, 300, 30)).w, 0);
    }
}
