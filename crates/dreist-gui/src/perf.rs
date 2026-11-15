//! The CPU and memory readout: a line ("DSP 31% · peak 58% · UI 14% ·
//! 351 MB · 0 late") over everything else, passing clicks through, where
//! the app puts it ([`Place`]). Apps add it with [`show`] and feed it once
//! a second with [`update`].

use crate::app::App;
use crate::paint::{Painter, RectF};
use crate::style::{FontSpec, HAlign, VAlign};
use crate::{Colour, Ctrl, Rect};

/// The readout control's name and overload (drawn by the renderer).
pub const NAME: &str = "PerfReadout";

const H: i32 = 16;

/// Where the readout goes in the root: `x` / `y` from the left / top, or
/// from the right / bottom edge when negative; `w` wide, text right-aligned
/// (left-aligned when `x` is from the left).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub w: i32,
}

impl Place {
    fn rect(&self, root: Rect) -> Rect {
        let x = if self.x < 0 { root.w + self.x - self.w } else { self.x };
        let y = if self.y < 0 { root.h + self.y - H } else { self.y };
        Rect::new(x, y, self.w, H)
    }
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
        let pc = |v: f32| format!("{:.0}%", v * 100.0);
        let mut parts = Vec::new();
        if let Some(d) = self.dsp {
            parts.push(format!("DSP {}", pc(d)));
        }
        if let Some(p) = self.peak {
            parts.push(format!("peak {}", pc(p)));
        }
        if let Some(u) = self.ui {
            parts.push(format!("UI {}", pc(u)));
        }
        if let Some(m) = self.memory {
            parts.push(format!("{} MB", m / (1 << 20)));
        }
        if let Some(l) = self.late {
            parts.push(format!("{l} late"));
        }
        parts.join(" · ")
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

/// Add the readout at `at`, or remove it.
pub fn show(app: &mut App, on: bool, at: Place) {
    match (path(app), on) {
        (None, true) => {
            let rect = at.rect(app.root.rect);
            let mut c = Ctrl { class: "CtrlPanel".into(), name: NAME.into(), overload: NAME.into(), rect, visible: true, passive: true, zorder: i16::MAX, ..Ctrl::default() };
            c.set_attr("place", format!("{} {} {}", at.x, at.y, at.w));
            app.root.children.push(c);
            app.dirty = true;
        }
        (Some(i), false) => {
            app.root.children.remove(i);
            app.dirty = true;
        }
        _ => {}
    }
}

/// Show `r` (when the readout is on); repaints only the readout.
pub fn update(app: &mut App, r: &Reading) {
    let Some(i) = path(app) else { return };
    let root = app.root.rect;
    let (text, level) = (r.text(), r.level().to_string());
    let c = &mut app.root.children[i];
    let at = c.attr("place").map(|v| v.split_whitespace().filter_map(|n| n.parse().ok()).collect::<Vec<i32>>()).filter(|v| v.len() == 3).map(|v| Place { x: v[0], y: v[1], w: v[2] });
    let Some(at) = at else { return };
    let rect = at.rect(root);
    if c.text == text && c.attr("level") == Some(level.as_str()) && c.rect == rect {
        return;
    }
    let old = c.rect;
    c.text = text;
    c.set_attr("level", level);
    c.rect = rect;
    let (ox, oy) = (app.root.rect.x, app.root.rect.y);
    app.invalidate_rect(Rect::new(old.x + ox, old.y + oy, old.w, old.h));
    app.invalidate_rect(Rect::new(rect.x + ox, rect.y + oy, rect.w, rect.h));
}

/// Draw the readout control: its text on a dark strip, amber or red near
/// the limit.
pub fn draw(p: &mut Painter, c: &Ctrl) {
    if c.text.is_empty() {
        return;
    }
    let font = FontSpec { family: crate::text::ui_family().into(), size: 11.0, bold: false };
    p.fonts.prepare(&font);
    let tw = p.fonts.measure(&font, &c.text).0.ceil() + 10.0;
    let (w, h) = (c.rect.w as f32, c.rect.h as f32);
    let left = c.attr("place").is_some_and(|v| !v.starts_with('-'));
    let bg = RectF::new(if left { 0.0 } else { (w - tw).max(0.0) }, 0.0, tw.min(w), h);
    p.fill_rounded(bg, 4.0, [true; 4], Colour::rgba(16, 18, 24, 235));
    let col = match c.attr("level") {
        Some("2") => Colour::rgba(255, 92, 80, 255),
        Some("1") => Colour::rgba(255, 190, 60, 255),
        _ => Colour::rgba(220, 226, 236, 255),
    };
    p.text(RectF::new(bg.x + 5.0, 0.0, bg.w - 10.0, h), &c.text, col, &font, HAlign::Center, VAlign::Center);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_lists_what_is_known() {
        let r = Reading { dsp: Some(0.312), peak: Some(0.58), ui: Some(0.14), memory: Some(351 << 20), late: Some(0) };
        assert_eq!(r.text(), "DSP 31% · peak 58% · UI 14% · 351 MB · 0 late");
        assert_eq!(r.level(), 0);
        assert_eq!(Reading { ui: Some(0.2), ..Reading::default() }.text(), "UI 20%");
        assert_eq!(Reading { peak: Some(0.75), ..Reading::default() }.level(), 1);
        assert_eq!(Reading { peak: Some(0.95), ..Reading::default() }.level(), 2);
    }
}
