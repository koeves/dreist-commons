//! Drawing of the stock [re:4a79c9] controls, following the original's
//! drawing. The original's custom panels (`overloadClass`) get their
//! skin children drawn here and their own content from the editor layer.

use crate::colour::{self, Colour};
use crate::ctrl::Ctrl;
use crate::paint::{Gradient, Painter, Radial, RectF, TextFx};
use crate::style::StyleSheet;

/// The skin control overload class of the header logo (a master level meter).
pub const LOGO_OVERLOAD: &str = "LogoMeter";

/// Knob/arc geometry shared by [re:c87121] and [re:69fe07].
const ARC_START: f32 = 110.0;
const ARC_SWEEP: f32 = 320.0;

/// Drawing for code-drawn controls (`overloadClass` panels). Called with
/// the painter at the control's origin; return true to replace the stock
/// drawing of the control itself (children are still drawn).
pub trait CustomDraw {
    fn draw(&self, r: &Renderer, p: &mut Painter, c: &Ctrl) -> bool;
    /// Drawn over the control's children.
    fn draw_over(&self, _r: &Renderer, _p: &mut Painter, _c: &Ctrl) {}
}

pub struct Renderer<'s> {
    pub styles: &'s StyleSheet,
    pub custom: Option<&'s dyn CustomDraw>,
    /// Collects where custom drawing happened (painter coordinates), so
    /// the app can repaint those areas when its model changes.
    pub custom_drawn: Option<&'s std::cell::RefCell<Vec<crate::ctrl::Rect>>>,
    /// Inside a translucent layer (painter coordinates are the layer's).
    in_layer: std::cell::Cell<u32>,
    /// The control whose children are being drawn is on (see
    /// `colParentOnFill`).
    parent_on: std::cell::Cell<bool>,
}

fn std_col(name: &str) -> Colour {
    colour::standard(name)
}

/// An option read from the control's own attributes first, then from its
/// style (`style`, through the parents). The attributes read this way are
/// opt-in: absent, the stock drawing is unchanged.
pub fn opt_raw<'a>(styles: &'a StyleSheet, style: &str, c: &'a Ctrl, k: &str) -> Option<&'a str> {
    c.attr(k).or_else(|| styles.raw(style, k))
}

/// [`opt_raw`] as a flag (`true` / `1`).
pub fn opt_bool(styles: &StyleSheet, style: &str, c: &Ctrl, k: &str, default: bool) -> bool {
    opt_raw(styles, style, c, k).map(|v| v.trim() == "true" || v.trim() == "1").unwrap_or(default)
}

/// [`opt_raw`] as a number.
pub fn opt_f32(styles: &StyleSheet, style: &str, c: &Ctrl, k: &str, default: f32) -> f32 {
    opt_raw(styles, style, c, k).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

/// [`opt_raw`] as a colour (a name or a literal).
pub fn opt_colour(styles: &StyleSheet, style: &str, c: &Ctrl, k: &str) -> Option<Colour> {
    opt_raw(styles, style, c, k).map(|v| styles.parse_colour(v))
}

/// A filled shape of the default fill.
#[derive(Clone, Copy, Debug)]
enum Shape {
    Rect(RectF),
    Rounded(RectF, f32, [bool; 4]),
    Ellipse(RectF),
}

impl Shape {
    fn path(self) -> Option<tiny_skia::Path> {
        match self {
            Shape::Rect(r) => tiny_skia::Rect::from_xywh(r.x, r.y, r.w, r.h).map(tiny_skia::PathBuilder::from_rect),
            Shape::Rounded(r, rad, c) => crate::paint::rounded_rect_path(r, rad, c),
            Shape::Ellipse(r) => tiny_skia::Rect::from_xywh(r.x, r.y, r.w, r.h).and_then(tiny_skia::PathBuilder::from_oval),
        }
    }

    fn inset(self, d: f32) -> Shape {
        match self {
            Shape::Rect(r) => Shape::Rect(r.inset(d)),
            Shape::Rounded(r, rad, c) => Shape::Rounded(r.inset(d), (rad - d).max(0.0), c),
            Shape::Ellipse(r) => Shape::Ellipse(r.inset(d)),
        }
    }

    /// Fill in `c` (a gradient from it with `g`); plain rectangles take
    /// the painter's rectangle fast path.
    fn fill(self, p: &mut Painter, c: Colour, g: Option<Gradient>) {
        match (self, g) {
            (Shape::Rect(r), None) => p.fill_rect(r, c),
            (sh, g) => {
                if let Some(path) = sh.path() {
                    p.fill_path_opt(&path, c, g);
                }
            }
        }
    }

    fn stroke(self, p: &mut Painter, c: Colour, w: f32) {
        if let Some(path) = self.path() {
            p.stroke_path(&path, c, w);
        }
    }
}

/// Fader cap shape (`enumHandleShape`): the stock round handle, or a
/// rectangular cap `realHandleLength` long along the travel.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Handle {
    Round,
    Rect(f32),
}

impl<'s> Renderer<'s> {
    pub fn new(styles: &'s StyleSheet) -> Renderer<'s> {
        Renderer { styles, custom: None, custom_drawn: None, in_layer: std::cell::Cell::new(0), parent_on: std::cell::Cell::new(false) }
    }

    pub fn style<'c>(&self, c: &'c Ctrl) -> &'c str {
        if !c.style.is_empty() && self.styles.styles.contains_key(&c.style) {
            &c.style
        } else if c.style.is_empty() && (!c.overload.is_empty() || c.class == "CtrlPanel") {
            // Code-built panels without a skin style draw nothing themselves.
            "InvisibleStyle"
        } else {
            "ApplicationStyle"
        }
    }

    /// Draw `c` (whose rect is relative to the painter's current origin) and
    /// its children ([re:0cca49]: visible children in
    /// order, each translated and clipped to its rect).
    pub fn draw(&self, p: &mut Painter, c: &Ctrl) {
        if !c.visible {
            return;
        }
        // The control's own alpha, on top
        // of the disabled alpha.
        let own = c.attr_f32("alpha", 1.0);
        if own <= 0.0 {
            return;
        }
        if own < 1.0 {
            let da = if c.enabled { 1.0 } else { self.styles.float(self.style(c), "realDisabledAlpha", 0.7) };
            return self.draw_alpha(p, c, own * da);
        }
        if !c.enabled {
            let st = self.style(c);
            match self.styles.raw(st, "enumDisabledDrawMode").map(str::trim) {
                Some("AlphaOut" | "1") | None => {
                    let alpha = self.styles.float(st, "realDisabledAlpha", 0.7);
                    return self.draw_alpha(p, c, alpha);
                }
                Some("ColourOverlay" | "2") => {
                    self.draw_enabled(p, c);
                    let overlay = self.styles.colour(st, "colDisabledOverlay", Colour::rgba(255, 255, 255, 128));
                    let (otx, oty) = (p.tx, p.ty);
                    p.tx += c.rect.x as f32;
                    p.ty += c.rect.y as f32;
                    self.default_fill_colours(p, c, overlay, Colour::TRANSPARENT, false);
                    p.tx = otx;
                    p.ty = oty;
                    return;
                }
                _ => {}
            }
        }
        self.draw_enabled(p, c);
    }

    /// [re:527038]: draw `c` into a layer, composite it.
    fn draw_alpha(&self, p: &mut Painter, c: &Ctrl, alpha: f32) {
        let s = p.scale;
        let (ax, ay) = (p.tx + c.rect.x as f32, p.ty + c.rect.y as f32);
        let (lw, lh) = (((c.rect.w as f32) * s).ceil() as u32, ((c.rect.h as f32) * s).ceil() as u32);
        let Some(mut layer) = tiny_skia::Pixmap::new(lw.max(1), lh.max(1)) else { return };
        if let Some(d) = self.custom_drawn.filter(|_| self.in_layer.get() == 0) {
            // Conservatively: the layer may hold custom drawing.
            d.borrow_mut().push(crate::ctrl::Rect::new(ax as i32, ay as i32, c.rect.w, c.rect.h));
        }
        self.in_layer.set(self.in_layer.get() + 1);
        {
            let mut lp = Painter::new(&mut layer, p.fonts, s);
            lp.tx = -c.rect.x as f32;
            lp.ty = -c.rect.y as f32;
            self.draw_enabled(&mut lp, c);
        }
        self.in_layer.set(self.in_layer.get() - 1);
        let paint = tiny_skia::PixmapPaint { opacity: alpha.clamp(0.0, 1.0), ..Default::default() };
        let m = if p.need_mask(((ax * s).floor(), (ay * s).floor(), lw as f32, lh as f32)) { p.mask.as_ref() } else { None };
        p.pm.draw_pixmap((ax * s).floor() as i32, (ay * s).floor() as i32, layer.as_ref(), &paint, tiny_skia::Transform::identity(), m);
    }

    fn draw_enabled(&self, p: &mut Painter, c: &Ctrl) {
        let abs_x = p.tx as i32 + c.rect.x;
        let abs_y = p.ty as i32 + c.rect.y;
        let prev = p.push_clip(abs_x, abs_y, c.rect.w, c.rect.h);
        if !p.clip_is_empty() {
            let (otx, oty) = (p.tx, p.ty);
            p.tx = abs_x as f32;
            p.ty = abs_y as f32;
            let custom = if c.overload.is_empty() { None } else { self.custom };
            if c.overload == LOGO_OVERLOAD {
                self.logo_meter(p, c);
            } else if custom.is_some_and(|cd| cd.draw(self, p, c)) {
                if let Some(d) = self.custom_drawn.filter(|_| self.in_layer.get() == 0) {
                    d.borrow_mut().push(crate::ctrl::Rect::new(abs_x, abs_y, c.rect.w, c.rect.h));
                }
            } else {
                self.draw_self(p, c);
            }
            if c.class == "CtrlSplitterPanel" {
                self.splitter_bar(p, c);
            }
            // [re:960cdf]: the selector under a
            // drag (`dropHighlight`) gets a (200, 200, 200, 120) fill over the
            // panel, under the children.
            for ch in c.children.iter().filter(|k| k.visible && k.attr("dropHighlight").is_some()) {
                p.fill_rect(crate::paint::RectF::new(ch.rect.x as f32, ch.rect.y as f32, ch.rect.w as f32, ch.rect.h as f32), Colour::rgba(200, 200, 200, 120));
            }
            // Children by z-order (stable); usually already in order.
            let parent_on = self.parent_on.replace(c.on);
            if self.native_table(c) {
                // The table draws its label children as rows.
                let mut order: Vec<&Ctrl> = c.children.iter().filter(|k| k.class != "CtrlLabel").collect();
                order.sort_by_key(|k| k.zorder);
                for ch in order {
                    self.draw(p, ch);
                }
            } else if c.children.windows(2).all(|w| w[0].zorder <= w[1].zorder) {
                for ch in &c.children {
                    self.draw(p, ch);
                }
            } else {
                let mut order: Vec<&Ctrl> = c.children.iter().collect();
                order.sort_by_key(|k| k.zorder);
                for ch in order {
                    self.draw(p, ch);
                }
            }
            self.parent_on.set(parent_on);
            if c.class == "CtrlLabeller" {
                self.labeller_labels(p, c);
            }
            if let Some(cd) = custom {
                cd.draw_over(self, p, c);
            }
            p.tx = otx;
            p.ty = oty;
        }
        p.pop_clip(prev);
    }

    /// The header logo with the master level ([re:2f6a08]
    /// [re:4a9597] / [re:a75af4] draw a 3 px frame inset 1.5 and the
    /// product mark in colFill, then again in colAltFill at the master
    /// level's alpha, `overlayAlpha` = min(level·0.75, 1)). The mark is the
    /// profile's [`logo_mark`](dreist_formats::profile::Profile::logo_mark).
    fn logo_meter(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let draw = |p: &mut Painter, col: Colour| {
            p.stroke_rect(RectF::new(1.5, 1.5, w - 3.0, h - 3.0), col, 3.0);
            for poly in dreist_formats::profile::get().logo_mark {
                p.fill_polygon(poly, col);
            }
        };
        draw(p, self.styles.colour(st, "colFill", Colour::rgba(255, 255, 0, 255)));
        let a = c.attr("overlayAlpha").and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        if a > 0.0 {
            draw(p, self.styles.colour(st, "colAltFill", Colour::rgba(152, 251, 152, 255)).with_alpha(a));
        }
    }

    fn draw_self(&self, p: &mut Painter, c: &Ctrl) {
        // Per-control fill (pad colour labels).
        if let Some(col) = c.attr("fillColour").and_then(Colour::parse_literal) {
            p.fill_rect(RectF::new(0.0, 0.0, c.rect.w as f32, c.rect.h as f32), col);
        }
        // Level overlays (engine selectors): colAltFill at `overlayAlpha`.
        if let Some(a) = c.attr("overlayAlpha").and_then(|v| v.parse::<f32>().ok()).filter(|a| *a > 0.0 && c.class != "CtrlLevelStrip") {
            let st = self.style(c);
            let col = self.styles.colour(st, "colAltFill", Colour::WHITE);
            let r = (c.rect.w.min(c.rect.h) as f32) / 2.0;
            p.fill_rounded(
                RectF::new(0.0, 0.0, c.rect.w as f32, c.rect.h as f32),
                r.min(self.styles.float(st, "realCornerRounding", 0.0).max(0.0)),
                self.styles.rounding_options(st),
                col.with_alpha(a),
            );
        }
        let renderer = c.attr("renderer").unwrap_or("");
        match renderer {
            "CheckBoxButtonRenderer" => return self.checkbox(p, c),
            "PopupButtonRenderer" => return self.popup(p, c),
            _ => {}
        }
        match c.class.as_str() {
            "CtrlWindowSizer" => self.window_sizer(p, c),
            "CtrlButton" | "CtrlDisclosureButton" => self.button(p, c),
            "CtrlSearchBox" => self.search_box(p, c),
            "CtrlLabel" | "CtrlMultiLineLabel" | "CtrlTempoBox" | "CtrlValueBox" | "CtrlTextBox" => self.label(p, c),
            "CtrlPopupButton" => self.popup(p, c),
            "CtrlKnob" => self.knob(p, c),
            "CtrlTransmodKnob" => self.transmod_knob(p, c),
            "CtrlSlider" | "CtrlTransmodSlider" => self.slider(p, c),
            "CtrlCollapsePanel" => self.collapse(p, c),
            "CtrlScrollBar" => {
                let st = self.style(c);
                let (w, h) = (c.rect.w as f32, c.rect.h as f32);
                let track = self.styles.colour(st, "colFill", std_col("StandardControlBackground"));
                let thumb = self.styles.colour(st, "colAltFill", std_col("StandardControlHighlight"));
                p.fill_rect(RectF::new(0.0, 0.0, w, h), track);
                // Opt-in: a frame in `colTrackBorder`, the thumb inset
                // `realThumbInset` all round (rounded to its short side),
                // and the thumb drawn full length when the whole content
                // fits (`optThumbWhenFull`).
                let s = self.styles;
                if let Some(b) = opt_colour(s, st, c, "colTrackBorder") {
                    for r in [RectF::new(0.0, 0.0, w, 1.0), RectF::new(0.0, h - 1.0, w, 1.0), RectF::new(0.0, 1.0, 1.0, h - 2.0), RectF::new(w - 1.0, 1.0, 1.0, h - 2.0)] {
                        p.fill_rect(r, b);
                    }
                }
                let page = c.attr_f32("realPage", 1.0).clamp(0.0, 1.0);
                if page < 1.0 || opt_bool(s, st, c, "optThumbWhenFull", false) {
                    let v = c.value.clamp(0.0, 1.0);
                    if let Some(i) = opt_raw(s, st, c, "realThumbInset").and_then(|v| v.trim().parse::<f32>().ok()) {
                        let (iw, ih) = ((w - 2.0 * i).max(0.0), (h - 2.0 * i).max(0.0));
                        let r = if h > w {
                            let th = (ih * page).max(8.0).min(ih);
                            RectF::new(i, i + v * (ih - th), iw, th)
                        } else {
                            let tw = (iw * page).max(8.0).min(iw);
                            RectF::new(i + v * (iw - tw), i, tw, ih)
                        };
                        p.fill_rounded(r, r.w.min(r.h) / 2.0, [true; 4], thumb);
                    } else {
                        let r = if h > w {
                            let th = (h * page).max(8.0);
                            RectF::new(1.0, v * (h - th), w - 2.0, th)
                        } else {
                            let tw = (w * page).max(8.0);
                            RectF::new(v * (w - tw), 1.0, tw, h - 2.0)
                        };
                        p.fill_rounded(r, (w.min(h) / 2.0 - 1.0).max(0.0), [true; 4], thumb);
                    }
                }
            }
            "CtrlValueStepper" => {
                self.label(p, c);
                let st = self.style(c);
                let col = self.styles.colour(st, "colText", std_col("StandardControlText"));
                let (w, h) = (c.rect.w as f32, c.rect.h as f32);
                let font = self.styles.font(st);
                p.text(RectF::new(w - 12.0, 0.0, 10.0, h / 2.0 + 2.0), "▴", col, &font, crate::style::HAlign::Center, crate::style::VAlign::Center);
                p.text(RectF::new(w - 12.0, h / 2.0 - 2.0, 10.0, h / 2.0 + 2.0), "▾", col, &font, crate::style::HAlign::Center, crate::style::VAlign::Center);
            }
            "CtrlMultiChannelDBMeter" => crate::meter::draw(p, self.styles, self.style(c), c),
            "CtrlProgressBar" => self.progress_bar(p, c),
            "CtrlLevelStrip" => self.level_strip(p, c),
            "CtrlPanSlider" => self.pan_slider(p, c),
            "CtrlRangeSlider" => self.range_slider(p, c),
            "CtrlPlot" => self.plot(p, c),
            "CtrlMeter" if self.style_class(c) == Some("CtrlMeterStyle") => self.needle_meter(p, c),
            "CtrlTableView" | "CtrlListBox" if self.native_table(c) => self.table(p, c),
            "root" => {}
            // Opt-in: panels inside an on control (icon parts drawn on a
            // button) use `colParentOnFill` / `colParentOnBorder`.
            _ if self.parent_on.get() && self.styles.raw(self.style(c), "colParentOnFill").is_some() => {
                let border = if self.styles.raw(self.style(c), "colParentOnBorder").is_some() { "colParentOnBorder" } else { "colBorder" };
                self.default_fill(p, c, "colParentOnFill", border, false)
            }
            _ => self.default_fill(p, c, "colFill", "colBorder", false),
        }
    }

    /// [re:33ddf4] (ev_db5615b5… background [re:7fa1f2] / bar
    /// [re:595f2d]), horizontal and determinate: the default fill (colFill /
    /// colBorder), then a bar round(w × value) wide — inset 2 px (clipped)
    /// when taller than 6 — filled colAltFill and framed 1 px in
    /// colAltBorder (default colAltFill darkened 0.4).
    fn progress_bar(&self, p: &mut Painter, c: &Ctrl) {
        self.default_fill(p, c, "colFill", "colBorder", false);
        let st = self.style(c);
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let fill = self.styles.colour(st, "colAltFill", std_col("StandardControlBackground"));
        let frame = self.styles.colour(st, "colAltBorder", fill.darken(0.4));
        let mut bw = (w * c.value.clamp(0.0, 1.0)).round();
        let (mut x, mut y, mut bh) = (0.0, 0.0, h);
        let prev = if c.rect.h > 6 {
            bw -= 4.0;
            x = 2.0;
            y = 2.0;
            bh = h - 4.0;
            Some(p.push_clip(p.tx as i32 + 2, p.ty as i32 + 2, c.rect.w - 4, c.rect.h - 4))
        } else {
            None
        };
        if bw > 0.0 {
            p.fill_rect(RectF::new(x, y, bw, bh), fill);
            p.stroke_rect(RectF::new(x + 0.5, y + 0.5, bw - 1.0, bh - 1.0), frame, 1.0);
        }
        if let Some(prev) = prev {
            p.pop_clip(prev);
        }
    }

    /// [re:df2c51]: three 1 px diagonals (corner to
    /// corner, then from h/2 − 3 and h/2 + 1) in LightGrey darkened 0.4,
    /// clipped to (w − 1, h − 1).
    fn window_sizer(&self, p: &mut Painter, c: &Ctrl) {
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let col = std_col("LightGrey").darken(0.4);
        let prev = p.push_clip(p.tx as i32, p.ty as i32, c.rect.w - 1, c.rect.h - 1);
        let (hw, hh) = ((c.rect.w / 2) as f32, (c.rect.h / 2) as f32);
        p.line(w, 0.0, 0.0, h, col, 1.0);
        p.line(w, hh - 3.0, hw - 3.0, h, col, 1.0);
        p.line(w, hh + 1.0, hw + 1.0, h, col, 1.0);
        p.pop_clip(prev);
    }

    /// [re:9e9841]: the bar in
    /// `SplitterPanelStyle`, with seven 4×4 dots 6 px apart along it in
    /// colAltBorder when its thin side is over 3 px.
    fn splitter_bar(&self, p: &mut Painter, c: &Ctrl) {
        let b = crate::splitter::bar(c);
        let style = c.attr("SplitterPanelStyle").unwrap_or("VCtrl::SplitterStyle").to_string();
        if !self.styles.styles.contains_key(&style) {
            return;
        }
        let bar = Ctrl { class: "CtrlPanel".into(), name: "Splitter".into(), style: style.clone(), rect: b, visible: true, enabled: true, ..Ctrl::default() };
        self.draw(p, &bar);
        let col = self.styles.colour(&style, "colAltBorder", Colour::WHITE);
        let (cx, cy) = (b.x + b.w / 2, b.y + b.h / 2);
        let horizontal = b.w > b.h;
        if (if horizontal { b.h } else { b.w }) <= 3 {
            return;
        }
        for i in 0..7 {
            let (x, y) = if horizontal { (cx - 2 - 18 + i * 6, cy - 2) } else { (cx - 2, cy - 2 - 18 + i * 6) };
            p.fill_ellipse(crate::paint::RectF::new(x as f32, y as f32, 4.0, 4.0), col);
        }
    }

    pub fn colour_attr(&self, st: &str, attr: &str) -> Colour {
        let d = match attr {
            "colBorder" => "StandardControlBorder",
            "colAltBorder" => "StandardControlBorderDarker",
            "colAltFill" => "StandardControlHighlight",
            "colFillHighlight" => "StandardControlHighlightDarker",
            "colText" | "colAltText" => "StandardControlText",
            _ => "StandardControlBackground",
        };
        self.styles.colour(st, attr, std_col(d))
    }

    /// The default fill [re:1efccd] with colour attributes (and the fill
    /// attribute's gradient, see [`Renderer::gradient`]).
    pub fn default_fill(&self, p: &mut Painter, c: &Ctrl, fill: &str, border: &str, shadow: bool) {
        let st = self.style(c);
        let f = self.colour_attr(st, fill);
        let b = self.colour_attr(st, border);
        let g = self.gradient(c, fill, f, None);
        self.default_fill_gradient(p, c, f, g, b, shadow);
    }

    pub fn default_fill_colours(&self, p: &mut Painter, c: &Ctrl, fill: Colour, border: Colour, shadow: bool) {
        self.default_fill_gradient(p, c, fill, None, border, shadow);
    }

    /// The gradient of fill attribute `fill` (e.g. `colFill`): its
    /// `<fill>GradientTo` colour (`colFillGradientTo`,
    /// `colAltFillGradientTo`), top to bottom, or left to right with
    /// `enumGradientDirection="Horizontal"`, or outwards with
    /// `enumGradientDirection="Radial"` from `realGradientCentreX` /
    /// `realGradientCentreY` (fractions of the bounds, default 0.5) to
    /// `realGradientRadius` (a fraction of the bounds' size, default 0.5;
    /// the fill colour holds up to `realGradientInnerRadius`, default 0).
    /// `adjust` changes the end colour as the fill was changed (hover).
    /// Without an end colour, `realGradientShade` shades `base` (the drawn
    /// fill) towards its end: darkened by the factor when positive,
    /// lightened when negative (so one inherited value suits every fill
    /// colour).
    pub fn gradient(&self, c: &Ctrl, fill: &str, base: Colour, adjust: Option<&dyn Fn(Colour) -> Colour>) -> Option<Gradient> {
        let st = self.style(c);
        let to = match opt_colour(self.styles, st, c, &format!("{fill}GradientTo")) {
            Some(to) => adjust.map(|f| f(to)).unwrap_or(to),
            None => {
                let k = opt_f32(self.styles, st, c, "realGradientShade", 0.0);
                if k.abs() < 1e-4 {
                    return None;
                }
                if k > 0.0 { base.darken(k) } else { base.lighten(-k) }
            }
        };
        let dir = opt_raw(self.styles, st, c, "enumGradientDirection").map(str::trim);
        let horizontal = dir.is_some_and(|v| v.eq_ignore_ascii_case("horizontal"));
        let radial = dir.filter(|v| v.eq_ignore_ascii_case("radial")).map(|_| Radial {
            cx: opt_f32(self.styles, st, c, "realGradientCentreX", 0.5),
            cy: opt_f32(self.styles, st, c, "realGradientCentreY", 0.5),
            r: opt_f32(self.styles, st, c, "realGradientRadius", 0.5).max(0.0),
            r0: opt_f32(self.styles, st, c, "realGradientInnerRadius", 0.0).max(0.0),
        });
        Some(Gradient { to, horizontal, radial })
    }

    /// A dash pattern (`len_key` px dashes, `gap_key` px gaps, the gap
    /// defaulting to the dash) when the dash is at least 1 px.
    fn dash(&self, c: &Ctrl, len_key: &str, gap_key: &str) -> Option<(f32, f32)> {
        let st = self.style(c);
        let d = opt_f32(self.styles, st, c, len_key, 0.0).round();
        (d >= 1.0).then(|| (d, opt_f32(self.styles, st, c, gap_key, d).round().max(1.0)))
    }

    /// `enumShape="Arrow"`: the depth of the chevron (`realArrowDepth`,
    /// default 0.35 × the height) and whether the start is notched
    /// (`optArrowNotch`, default true; off for the first of a row).
    fn arrow(&self, c: &Ctrl) -> Option<(f32, bool)> {
        let st = self.style(c);
        let s = self.styles;
        opt_raw(s, st, c, "enumShape").filter(|v| v.trim().eq_ignore_ascii_case("arrow"))?;
        let d = opt_f32(s, st, c, "realArrowDepth", (c.rect.h as f32 * 0.35).round()).clamp(0.0, c.rect.w as f32 / 2.0);
        Some((d, opt_bool(s, st, c, "optArrowNotch", true)))
    }

    /// The default fill with an optional gradient. Opt-in options:
    /// `enumShape="Arrow"` (a breadcrumb chevron, [`Renderer::arrow`]);
    /// `optBevel` (a 1 px edge inside the shape, `colBevelLight` at the top
    /// fading to `colBevelDark` at the bottom); `intDashLength` /
    /// `intDashGap` (a dashed border; thin panels, 2 px or less across,
    /// are lines and draw their fill as dashes along their length);
    /// `realNoiseAmount` (a brushed texture over the fill, see
    /// [`Painter::fill_path_noise`]: streaks `realNoiseStreak` px long,
    /// default 24, pattern `intNoiseSeed`, default 1).
    pub fn default_fill_gradient(&self, p: &mut Painter, c: &Ctrl, fill: Colour, grad: Option<Gradient>, border: Colour, _shadow: bool) {
        let st = self.style(c);
        let s = self.styles;
        let bw = s.float(st, "realBorderWidth", 1.0);
        let draw_border = s.bool(st, "optDrawBorder", true) && bw.abs() >= 1e-6;
        let corners = s.rounding_options(st);
        let radius = s.float(st, "realCornerRounding", 0.0).max(0.0);
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let full = RectF::new(0.0, 0.0, w, h);
        let dash = self.dash(c, "intDashLength", "intDashGap");
        if let Some((d, notch)) = self.arrow(c) {
            let poly = |i: f32| {
                let (x0, y0, x1, y1) = (i, i, w - i, h - i);
                let mut pb = tiny_skia::PathBuilder::new();
                pb.move_to(x0, y0);
                pb.line_to(x1 - d, y0);
                pb.line_to(x1, h / 2.0);
                pb.line_to(x1 - d, y1);
                pb.line_to(x0, y1);
                if notch {
                    pb.line_to(x0 + d, h / 2.0);
                }
                pb.close();
                pb.finish()
            };
            if let Some(path) = poly(0.0) {
                p.fill_path_opt(&path, fill, grad);
            }
            if draw_border {
                if let Some(path) = poly(bw * 0.5) {
                    p.dash = dash;
                    p.stroke_path(&path, border, bw);
                    p.dash = None;
                }
            }
            return;
        }
        let none = corners.iter().all(|x| !x);
        let all = corners.iter().all(|x| *x);
        if let Some((d, g)) = dash.filter(|_| (radius < 1e-6 || none) && (w <= 2.0 || h <= 2.0)) {
            let along = w > h;
            let len = if along { w } else { h };
            let mut a = 0.0;
            while a < len {
                let l = d.min(len - a);
                p.fill_rect(if along { RectF::new(a, 0.0, l, h) } else { RectF::new(0.0, a, w, l) }, fill);
                a += d + g;
            }
            return;
        }
        let border_rect = |bw: f32| {
            let r = full.inset(bw * 0.5);
            if (bw as i32) & 1 != 0 { r.pixel_align() } else { r }
        };
        let (body, edge, stroke) = if radius < 1e-6 || none {
            (Shape::Rect(full), Shape::Rect(border_rect(bw)), draw_border)
        } else if all {
            let (wi, hi) = ((w as i32) as f32, (h as i32) as f32);
            if (wi - hi).abs() > 1e-6 || (radius - wi * 0.5).abs() > 1e-6 {
                let r = RectF::new(bw * 0.5, bw * 0.5, wi - bw, hi - bw);
                (Shape::Rounded(RectF::new(0.0, 0.0, wi, hi), radius, corners), Shape::Rounded(if (bw as i32) & 1 != 0 { r.pixel_align() } else { r }, radius, corners), draw_border)
            } else {
                (Shape::Ellipse(full), Shape::Ellipse(border_rect(bw)), draw_border)
            }
        } else {
            // Quirk: with partial rounding a border wider than 1 is never
            // stroked (the inset path is built and discarded).
            (Shape::Rounded(full, radius, corners), Shape::Rounded(full, radius, corners), draw_border && bw <= 1.0)
        };
        body.fill(p, fill, grad);
        let noise = opt_f32(s, st, c, "realNoiseAmount", 0.0);
        if noise > 0.0 {
            if let Some(path) = body.path() {
                let seed = opt_f32(s, st, c, "intNoiseSeed", 1.0).round() as i64 as u32;
                p.fill_path_noise(&path, noise, opt_f32(s, st, c, "realNoiseStreak", 24.0), seed);
            }
        }
        if opt_bool(s, st, c, "optBevel", false) {
            let light = opt_colour(s, st, c, "colBevelLight").unwrap_or(Colour::rgba(255, 255, 255, 110));
            let dark = opt_colour(s, st, c, "colBevelDark").unwrap_or(Colour::rgba(0, 0, 0, 90));
            if let Some(path) = body.inset(if stroke { bw } else { 0.0 } + 0.5).path() {
                p.stroke_path_vgradient(&path, &[(0.0, light), (0.3, light.with_alpha(0.0)), (0.7, dark.with_alpha(0.0)), (1.0, dark)], 1.0);
            }
        }
        if stroke {
            p.dash = dash;
            edge.stroke(p, border, bw);
            p.dash = None;
        }
    }

    /// [re:485453].
    /// Opt-in `realTextOffsetY` moves the text down by that many px (up
    /// when negative).
    fn text_rect(&self, c: &Ctrl, r: RectF) -> RectF {
        let st = self.style(c);
        let dy = opt_f32(self.styles, st, c, "realTextOffsetY", 0.0);
        let r = RectF::new(r.x, r.y + dy, r.w, r.h);
        let Some(off) = self.styles.raw(st, "realTextOffset").and_then(|v| v.trim().parse::<f32>().ok()) else { return r };
        match self.styles.h_align(st) {
            crate::style::HAlign::Right => RectF::new(r.x - off, r.y, (r.w - off).max(0.0), r.h),
            _ => RectF::new(r.x + off, r.y, (r.w - off).max(0.0), r.h),
        }
    }

    pub fn draw_text(&self, p: &mut Painter, c: &Ctrl, r: RectF, text: &str, colour_attr: &str) {
        let st = self.style(c);
        if text.is_empty() {
            return;
        }
        let font = self.styles.font(st);
        let mut colour = match self.styles.raw(st, "colTextDisabled").filter(|_| !c.enabled) {
            // Disabled controls with a disabled text colour use it.
            Some(v) => self.styles.parse_colour(v),
            None => self.styles.colour(st, colour_attr, std_col("StandardControlText")),
        };
        // Runtime fade (a colour attribute with an alpha copy).
        if let Some(a) = c.attr("textAlpha").and_then(|v| v.parse::<f32>().ok()) {
            colour = colour.with_alpha(colour.f_alpha() * a.clamp(0.0, 1.0));
        }
        let tr = self.text_rect(c, r);
        let fx = self.text_fx(c);
        let s = p.fonts.shorten(&font, text, tr.w / fx.scale_x.max(0.1), self.styles.shorten(st));
        let (h, v) = (self.styles.h_align(st), self.styles.v_align(st));
        // Opt-in glow: `<colour attr>Glow` (`colTextGlow`, `colAltTextGlow`)
        // blurred `realTextGlowRadius` px (default 2) behind the text,
        // `realTextGlowStrength` (default 1) times as strong.
        if let Some(glow) = opt_colour(self.styles, st, c, &format!("{colour_attr}Glow")) {
            let radius = opt_f32(self.styles, st, c, "realTextGlowRadius", 2.0);
            p.text_glow(tr, &s, glow, &font, h, v, fx, radius, opt_f32(self.styles, st, c, "realTextGlowStrength", 1.0));
        }
        p.text_fx(tr, &s, colour, &font, h, v, fx);
        // Opt-in `realTextEmbolden`: the text drawn again that many px to
        // the right (heavier strokes than the font's bold).
        let bold = opt_f32(self.styles, st, c, "realTextEmbolden", 0.0);
        if bold > 0.0 {
            p.text_fx(RectF::new(tr.x + bold, tr.y, tr.w, tr.h), &s, colour, &font, h, v, fx);
        }
    }

    /// Opt-in text options: `realLetterSpacing` (px between letters),
    /// `realTextScaleX` (horizontal scale, below 1 condenses) and
    /// `realTextSlant` (degrees, positive leans right: italic).
    pub fn text_fx(&self, c: &Ctrl) -> TextFx {
        let st = self.style(c);
        let s = self.styles;
        TextFx { spacing: opt_f32(s, st, c, "realLetterSpacing", 0.0), scale_x: opt_f32(s, st, c, "realTextScaleX", 1.0), slant: opt_f32(s, st, c, "realTextSlant", 0.0) }
    }

    /// `optDrawText` (buttons and labels): false hides the text. The
    /// built-in `InvisibleStyle`'s false is not honoured, as before: code-built
    /// labels without a style resolve to it and still show their text.
    fn draws_text(&self, c: &Ctrl) -> bool {
        match self.styles.raw_from(self.style(c), "optDrawText") {
            Some((from, v)) if from != "InvisibleStyle" => !(v.trim() == "false" || v.trim() == "0"),
            _ => true,
        }
    }

    /// The element class of the control's own style (e.g. `CtrlMeterStyle`).
    fn style_class(&self, c: &Ctrl) -> Option<&str> {
        self.styles.styles.get(self.style(c)).map(|s| s.class.as_str())
    }

    /// Tables and list boxes draw their rows only with a table or list box
    /// style (`CtrlTableViewStyle` / `CtrlListBoxStyle`); otherwise they
    /// keep the default fill and their content is code-drawn.
    fn native_table(&self, c: &Ctrl) -> bool {
        matches!(c.class.as_str(), "CtrlTableView" | "CtrlListBox") && matches!(self.style_class(c), Some("CtrlTableViewStyle" | "CtrlListBoxStyle"))
    }

    fn bounds(c: &Ctrl) -> RectF {
        RectF::new(0.0, 0.0, c.rect.w as f32, c.rect.h as f32)
    }

    /// [re:b7a6d1] + text [re:26c463]. Opt-in: the fill's gradient
    /// (`colFillGradientTo` / `colAltFillGradientTo`), and a strip along the
    /// bottom `realStripHeight` high, inset `realStripInset` (default the
    /// border width) from the bottom and `realStripSideInset` (default
    /// `realStripInset`) from the sides, rounded `realStripRounding`
    /// (default 1), in `colStripOn` when on and `colStripOff` when off.
    fn button(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let fill_attr = if c.on { "colAltFill" } else { "colFill" };
        let mut fill = self.colour_attr(st, fill_attr);
        let mut border = self.colour_attr(st, if c.on { "colAltBorder" } else { "colBorder" });
        let hover = |f: Colour| {
            let f2 = f.lighten(0.7);
            if f2.a < 255 { f2.with_alpha(f2.f_alpha() + f2.f_alpha() * 0.2) } else { f2 }
        };
        if c.hover {
            fill = hover(fill);
            border = border.darken(0.4);
        }
        let grad = self.gradient(c, fill_attr, fill, if c.hover { Some(&hover) } else { None });
        self.default_fill_gradient(p, c, fill, grad, border, true);
        let s = self.styles;
        let sh = opt_f32(s, st, c, "realStripHeight", 0.0);
        if sh > 0.0 {
            if let Some(col) = opt_colour(s, st, c, if c.on { "colStripOn" } else { "colStripOff" }) {
                let bw = if s.bool(st, "optDrawBorder", true) { s.float(st, "realBorderWidth", 1.0).max(0.0) } else { 0.0 };
                let i = opt_f32(s, st, c, "realStripInset", bw).max(0.0);
                let si = opt_f32(s, st, c, "realStripSideInset", i).max(0.0);
                let rounding = opt_f32(s, st, c, "realStripRounding", 1.0).max(0.0);
                let (w, h) = (c.rect.w as f32, c.rect.h as f32);
                p.fill_rounded(RectF::new(si, h - i - sh, w - 2.0 * si, sh), rounding, [true; 4], col);
            }
        }
        self.button_glyph(p, c);
        if self.draws_text(c) {
            let b = match self.arrow(c) {
                // Text between the notch and the point.
                Some((d, notch)) => RectF::new(if notch { d * 0.5 } else { 0.0 }, 0.0, c.rect.w as f32 - d * if notch { 1.0 } else { 0.5 }, c.rect.h as f32),
                None => Self::bounds(c),
            };
            self.draw_text(p, c, b, &c.text, if c.on { "colAltText" } else { "colText" });
        }
    }

    /// Opt-in `enumGlyph`: a shape centred on the button, over the fill and
    /// under the text: `Bar` (an upright pill) or `TriangleLeft` /
    /// `TriangleRight` (pointing that way), `realGlyphWidth` ×
    /// `realGlyphHeight` px (default the whole rect), filled in `colGlyph`
    /// (when on `colAltGlyph`, default colGlyph; default colText /
    /// colAltText) and outlined 1 px in `colGlyphBorder` (when on
    /// `colAltGlyphBorder`, default colGlyphBorder; none without).
    fn button_glyph(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let s = self.styles;
        let Some(kind) = opt_raw(s, st, c, "enumGlyph").map(str::trim) else { return };
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let (gw, gh) = (opt_f32(s, st, c, "realGlyphWidth", w).clamp(0.0, w), opt_f32(s, st, c, "realGlyphHeight", h).clamp(0.0, h));
        let r = RectF::new(((w - gw) / 2.0).round(), ((h - gh) / 2.0).round(), gw, gh);
        let pick = |on: &str, off: &str| if c.on { opt_colour(s, st, c, on).or_else(|| opt_colour(s, st, c, off)) } else { opt_colour(s, st, c, off) };
        let fill = pick("colAltGlyph", "colGlyph").unwrap_or_else(|| self.colour_attr(st, if c.on { "colAltText" } else { "colText" }));
        let edge = pick("colAltGlyphBorder", "colGlyphBorder");
        if kind.eq_ignore_ascii_case("bar") {
            let rad = r.w.min(r.h) / 2.0;
            p.fill_rounded(r, rad, [true; 4], fill);
            if let Some(e) = edge {
                p.stroke_rounded(r.inset(0.5), (rad - 0.5).max(0.0), [true; 4], e, 1.0);
            }
        } else if kind.eq_ignore_ascii_case("triangleleft") || kind.eq_ignore_ascii_case("triangleright") {
            let r = r.inset(0.5);
            let pts = if kind.eq_ignore_ascii_case("triangleleft") {
                [(r.x + r.w, r.y), (r.x + r.w, r.y + r.h), (r.x, r.y + r.h / 2.0)]
            } else {
                [(r.x, r.y), (r.x, r.y + r.h), (r.x + r.w, r.y + r.h / 2.0)]
            };
            p.fill_polygon(&pts, fill);
            if let Some(e) = edge {
                p.polyline(&[pts[0], pts[1], pts[2], pts[0]], e, 1.0);
            }
        }
    }

    /// [re:29f888]: text box with the text between the
    /// h×h search icon ([re:a7d6df]: 2 px magnifier and a menu
    /// triangle in colText) and the clear icon ([re:369e45],
    /// only with text: a colAltFill disc inset h/5 with a 2 px colFill
    /// cross inset h/6).
    fn search_box(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let b = Self::bounds(c);
        let h = b.h;
        let editing = c.attr("editing") == Some("true");
        if editing {
            p.fill_rect(b, Colour::WHITE);
            p.stroke_rect(b.inset(0.5), std_col("StandardControlHighlight"), 1.0);
        } else {
            self.default_fill(p, c, "colFill", "colBorder", false);
        }
        let text_col = if editing { Colour::BLACK } else { self.styles.colour(st, "colText", std_col("StandardControlText")) };
        // `optSearchIcon` false (opt-in): no magnifier; the text starts at
        // the left edge (an app drawing the icon beside the box).
        let icon = self.styles.bool(st, "optSearchIcon", true);
        let x0 = if icon { h } else { 3.0 };
        let tr = RectF::new(x0, 0.0, (b.w - x0 - 2.0 - h).max(0.0), h);
        if editing {
            let font = self.styles.font(st);
            p.text(tr, &format!("{}|", c.text), Colour::BLACK, &font, crate::style::HAlign::Left, crate::style::VAlign::Center);
        } else {
            let text = if c.text.is_empty() { c.attr("placeholder").unwrap_or("").to_string() } else { c.text.clone() };
            self.draw_text(p, c, tr, &text, "colText");
        }
        // Magnifier.
        if icon {
            let d = ((h - 4.0) * 0.5).max(1.0);
            let (x, y) = (4.0f32.floor() + 0.5, 4.0f32.floor() + 0.5);
            p.stroke_ellipse(RectF::new(x, y, d, d), text_col, 2.0);
            let a = x + d * 0.5 + d * 0.5 * std::f32::consts::FRAC_1_SQRT_2;
            let e = x + d * 0.5 + d * 1.5 * std::f32::consts::FRAC_1_SQRT_2;
            p.line(a, a, e, e, text_col, 2.0);
            let q = (h - 4.0) * 0.25;
            let ty = 2.0 + (h - 4.0) * 0.5 + q;
            p.fill_polygon(&[(x, ty), (x + q, ty), (x + q * 0.5, ty + q)], text_col);
        }
        if !c.text.is_empty() {
            let hi = h as i32;
            let i6 = ((hi / 5).max(1) as f32 + 0.5) as i32 as f32;
            let i3 = (hi / 6).max(1) as f32;
            let r = RectF::new(b.w - h + i6, i6, h - 2.0 * i6, h - 2.0 * i6);
            p.fill_ellipse(r, self.styles.colour(st, "colAltFill", std_col("StandardControlBackgroundDark")));
            let fill = self.styles.colour(st, "colFill", std_col("StandardControlBackground"));
            p.line(r.x + i3, r.y + i3, r.x + r.w - i3, r.y + r.h - i3, fill, 2.0);
            p.line(r.x + r.w - i3, r.y + i3, r.x + i3, r.y + r.h - i3, fill, 2.0);
        }
    }

    /// [re:51d4eb] (rotation not supported yet). Text
    /// boxes being edited get a highlight frame and a caret.
    fn label(&self, p: &mut Painter, c: &Ctrl) {
        self.default_fill(p, c, "colFill", "colBorder", false);
        if c.attr("editing") == Some("true") {
            let b = Self::bounds(c);
            p.fill_rect(b, Colour::WHITE);
            p.stroke_rect(b.inset(0.5), std_col("StandardControlHighlight"), 1.0);
            let st = self.style(c);
            let font = self.styles.font(st);
            let text = format!("{}|", c.text);
            p.text(RectF::new(3.0, 0.0, b.w - 6.0, b.h), &text, Colour::BLACK, &font, crate::style::HAlign::Left, crate::style::VAlign::Center);
            return;
        }
        if !self.draws_text(c) {
            return;
        }
        let text = if c.text.is_empty() { c.attr("placeholder").unwrap_or("").to_string() } else { c.text.clone() };
        if c.class == "CtrlMultiLineLabel" {
            // One line per `intLineHeight` (default 12) from the top.
            let b = Self::bounds(c);
            let lh = c.attr_i32("intLineHeight", 12).max(1) as f32;
            for (i, line) in text.split('\n').enumerate() {
                self.draw_text(p, c, RectF::new(b.x, b.y + i as f32 * lh, b.w, lh), line, "colText");
            }
            return;
        }
        self.draw_text(p, c, Self::bounds(c), &text, "colText");
    }

    /// [re:638dd5] / [re:3d0384].
    fn popup(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        self.default_fill(p, c, "colFill", "colBorder", true);
        let b = Self::bounds(c);
        // Opt-in: `realPopupArrowSize` (× the height, default 0.4),
        // `realPopupArrowBoxWidth` (pixels; default the height: a square
        // box at the right) and `optPopupDivider` (default true: the line
        // left of the arrow).
        // With an explicit box width the text runs from the left edge up to
        // the arrow itself (v1's bitmap menu buttons: the text offset is
        // from the edge and the arrow has no box of its own).
        let explicit = opt_raw(self.styles, st, c, "realPopupArrowBoxWidth").is_some();
        let box_w = opt_f32(self.styles, st, c, "realPopupArrowBoxWidth", b.h);
        let arrow_x = b.w - box_w;
        let aw = b.h * opt_f32(self.styles, st, c, "realPopupArrowSize", 0.4);
        let ax = arrow_x + box_w * 0.5 - aw * 0.5;
        let tr = if explicit { RectF::new(b.x, b.y, ax - 1.0 - b.x, b.h) } else { RectF::new(b.x + 2.0, b.y, arrow_x - b.x - 2.0, b.h) };
        self.draw_text(p, c, tr, &c.text, "colText");
        let text = self.styles.colour(st, "colText", std_col("StandardControlText"));
        let border = self.styles.colour(st, "colBorder", std_col("StandardControlBorder"));
        let ay = b.y + b.h * 0.5 - aw * 0.5;
        p.fill_polygon(&[(ax, ay), (ax + aw, ay), (ax + aw * 0.5, ay + aw)], text);
        if opt_bool(self.styles, st, c, "optPopupDivider", true) {
            let lx = arrow_x.floor() + 0.5;
            p.line(lx, b.y, lx, b.y + b.h, border, 1.0);
        }
    }

    /// [re:5eb27d] (only with a checkbox style).
    fn checkbox(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        if self.styles.styles.get(st).map(|s| s.class.as_str()) != Some("CheckBoxButtonRendererStyle") {
            return;
        }
        let s = self.styles;
        let fill = s.colour(st, if c.on { "colAltFill" } else { "colFill" }, if c.on { Colour::TRANSPARENT } else { std_col("StandardControlBackground") });
        let border = s.colour(st, if c.on { "colAltBorder" } else { "colBorder" }, std_col("StandardControlBorder"));
        let tick = s.colour(st, "colTick", std_col("StandardControlBorder"));
        let radius = s.float(st, "realCornerRounding", 0.0);
        let bw = s.float(st, "realBorderWidth", 1.0);
        let b = Self::bounds(c);
        let inset = s.float(st, "realTickInset", 2.0);
        let lw = s.float(st, "realTickLineWidth", 2.0);
        p.fill_rounded(b, radius, [true; 4], fill);
        if s.bool(st, "optDrawBorder", true) {
            p.stroke_rounded(b, radius, [true; 4], border, bw);
        }
        if c.on {
            let (x, y, w, h) = (b.x + inset, b.y + inset, b.w - 2.0 * inset, b.h - 2.0 * inset);
            let pts = [(w * 0.125 + x, h * 0.6 + y), (lw * 0.5 + w * 0.3 + x, lw * 0.5 + h * 0.8 + y), (w * 0.9 + x, h * 0.1 + y)];
            // `colTickOutline` (opt-in): the tick drawn over a wider stroke
            // in that colour, 1 px each side.
            if let Some(o) = opt_colour(s, st, c, "colTickOutline") {
                p.polyline(&pts, o, lw + 2.0);
            }
            p.polyline(&pts, tick, lw);
        }
    }

    /// [re:7d8258]. `enumDialAlign` (`Top` / `Center`) draws a square dial
    /// of the rect's smaller side, centred across and at the top or in the
    /// middle along (rects taller or wider than the dial); without it the
    /// dial fills the rect.
    fn knob(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let s = self.styles;
        let b = Self::bounds(c);
        let b = match opt_raw(s, st, c, "enumDialAlign").map(str::trim) {
            Some(a @ ("Top" | "top" | "Center" | "center")) => {
                let side = b.w.min(b.h);
                let x = ((b.w - side) * 0.5).floor();
                let y = if a.eq_ignore_ascii_case("top") { 0.0 } else { ((b.h - side) * 0.5).floor() };
                RectF::new(x, y, side, side)
            }
            _ => b,
        };
        let thick = c.attr_f32("realDialThickness", 0.2) * b.w;
        let ring = b.inset(thick * 0.5);
        let back = s.colour(st, "colFill", std_col("StandardControlBackground"));
        let hl = s.colour(st, "colFillHighlight", back.darken(0.7));
        let dot_in = s.colour(st, "colAltFill", Colour::BLACK);
        let dot_out = s.colour(st, "colAltBorder", Colour::WHITE);
        let v = c.value.clamp(0.0, 1.0) * ARC_SWEEP;
        let angle = v + ARC_START;
        p.stroke_arc(ring, ARC_START, ARC_SWEEP, true, back, thick);
        if c.attr_bool("optCentered", false) {
            if angle >= 270.0 {
                p.stroke_arc(ring, 270.0, angle - 270.0, true, hl, thick);
            } else {
                p.stroke_arc(ring, 270.0, 270.0 - angle, false, hl, thick);
            }
        } else {
            p.stroke_arc(ring, ARC_START, v, true, hl, thick);
        }
        let (cx, cy) = (b.x + b.w * 0.5, b.y + b.h * 0.5);
        // Opt-in face: a disc inside the ring (`realFaceInset` from its inner
        // edge, default 1; negative covers the ring) in `colFace` (gradient
        // `colFaceGradientTo`) framed in `colFaceBorder`,
        // `realFaceBorderWidth` (default 1) wide.
        if let Some(face) = opt_colour(s, st, c, "colFace") {
            let r = b.inset(thick + opt_f32(s, st, c, "realFaceInset", 1.0));
            if r.w > 0.0 {
                if let Some(path) = tiny_skia::Rect::from_xywh(r.x, r.y, r.w, r.h).and_then(tiny_skia::PathBuilder::from_oval) {
                    p.fill_path_opt(&path, face, self.gradient(c, "colFace", face, None));
                }
                if let Some(fb) = opt_colour(s, st, c, "colFaceBorder") {
                    let fw = opt_f32(s, st, c, "realFaceBorderWidth", 1.0).max(0.0);
                    p.stroke_ellipse(r.inset(fw / 2.0), fb, fw);
                }
            }
        }
        // Opt-in pointer (`optPointer`, in place of the value dot): a line in
        // `colPointer` (default colAltBorder), `realPointerWidth` (default 2)
        // wide, from `realPointerStart` × the radius (default 0, the centre)
        // to `realPointerInset` (default 1) inside the ring; `optPointerDot`
        // keeps the value dot too.
        let pointer = opt_bool(s, st, c, "optPointer", false);
        if pointer {
            let col = opt_colour(s, st, c, "colPointer").unwrap_or(dot_out);
            let end = (b.w * 0.5 - thick - opt_f32(s, st, c, "realPointerInset", 1.0)).max(0.0);
            let start = end * opt_f32(s, st, c, "realPointerStart", 0.0).clamp(0.0, 1.0);
            let a = angle.to_radians();
            let (dx, dy) = (a.cos(), a.sin());
            p.line(cx + dx * start, cy + dy * start, cx + dx * end, cy + dy * end, col, opt_f32(s, st, c, "realPointerWidth", 2.0));
        }
        if b.w > 32.0 && b.h > 32.0 && (!pointer || opt_bool(s, st, c, "optPointerDot", false)) {
            let rad = cx - ring.x;
            let a = angle.to_radians();
            let half = thick * 0.5;
            let r = RectF::new((cx + a.cos() * rad - half).round(), (cy + a.sin() * rad - half).round(), thick.round(), thick.round());
            p.fill_ellipse(r, dot_out);
            let i = (half * 0.4).max(1.0).round();
            p.fill_ellipse(r.inset(i), dot_in);
        }
    }

    /// [re:d277c8] (layers 0, 1, 5; modulation
    /// layers 2–4 need a bound transmod source).
    fn transmod_knob(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let s = self.styles;
        let b = Self::bounds(c);
        let thick = c.attr_f32("realDialThickness", 0.2) * b.w;
        let mod_dim = c.attr_f32("realModDim", 0.25);
        // Layer 1: DrawTrack.
        let tw = b.w - b.w * mod_dim;
        let th = b.h - b.h * mod_dim;
        let half = thick * 0.5;
        let track = RectF::new(b.w * 0.5 - tw * 0.5 + half, b.h * 0.5 - th * 0.5 + half, tw - thick, th - thick);
        let fill = s.colour(st, "colFill", std_col("StandardControlBackground"));
        let alt = s.colour(st, "colAltFill", std_col("StandardControlBackground"));
        // While a transmod source is selected the knob edits that source's
        // depth: `value` holds (depth + 1) / 2 and the base value is kept in
        // `baseValue` (DrawModZones: the zone spans base .. base + depth).
        let editing_depth = c.attr("editingDepth") == Some("true");
        let base = if editing_depth { c.attr_f32("baseValue", 0.0) } else { c.value };
        let depth = c.attr_f32("modDepth", 0.0);
        p.stroke_arc(track, ARC_START, ARC_SWEEP, true, fill, thick);
        if depth.abs() > 1e-4 {
            let ring = b.inset((b.w * mod_dim * 0.25).max(1.0));
            let zone = s.colour(st, "colDepth", std_col("StandardControlHighlight"));
            let a0 = base.clamp(0.0, 1.0);
            let a1 = (base + depth).clamp(0.0, 1.0);
            let (lo, hi) = if a0 <= a1 { (a0, a1) } else { (a1, a0) };
            p.stroke_arc(ring, ARC_START + lo * ARC_SWEEP, (hi - lo) * ARC_SWEEP, true, zone, (b.w * mod_dim * 0.5).max(2.0));
        }
        if c.enabled {
            let v = base.clamp(0.0, 1.0) * ARC_SWEEP;
            if !c.attr_bool("optCentered", false) {
                p.stroke_arc(track, ARC_START, v, true, alt, thick);
            } else if ARC_START + v >= 270.0 {
                p.stroke_arc(track, 270.0, ARC_START + v - 270.0, true, alt, thick);
            } else {
                p.stroke_arc(track, 270.0, 270.0 - v - ARC_START, false, alt, thick);
            }
        }
        // Layer 5: DrawHandles (large knobs) or the small-knob outline.
        let small = c.rect.w <= 32 || c.rect.h <= 32;
        if c.enabled && !small {
            let border = s.colour(st, "colHandleBorder", Colour::WHITE);
            let hfill = s.colour(st, "colHandleFill", Colour::BLACK);
            let (cx, cy) = (b.w * 0.5, b.h * 0.5);
            let inner = cx - (b.w - b.w * mod_dim) * 0.5;
            let rad = cx - (inner + half);
            let a = (base.clamp(0.0, 1.0) * ARC_SWEEP + ARC_START).to_radians();
            let r = RectF::new(a.cos() * rad + cx - half, cy + a.sin() * rad - half, thick, thick);
            p.fill_ellipse(r, border);
            p.fill_ellipse(r.inset((half * 0.4).max(1.0)), hfill);
        } else {
            let d = s.colour(st, "colDepth", std_col("StandardControlHighlight"));
            p.stroke_ellipse(RectF::new(1.0, 1.0, b.w - 2.0, b.h - 2.0), Colour { a: 255, ..d }, 1.0);
        }
    }

    /// [re:0c57f8] + handle [re:a65320]. Opt-in: `enumHandleShape="Rect"`
    /// (a rectangular cap, see [`Renderer::handle`]) and `optFlipDirection`
    /// (vertical sliders grow downwards, horizontal ones leftwards).
    fn slider(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let s = self.styles;
        let (w, h) = (c.rect.w, c.rect.h);
        let inset = c.attr_f32("realTrackInset", 0.3);
        let back = s.colour(st, "colFill", std_col("StandardControlBackground"));
        let hl = s.colour(st, "colFillHighlight", back.darken(0.7));
        let radius = s.float(st, "realCornerRounding", 0.0).round();
        let v = c.value.clamp(0.0, 1.0);
        let flip = opt_bool(s, st, c, "optFlipDirection", false);
        let vertical = w < h;
        let handle = self.handle(c, (if vertical { w } else { h }) as f32);
        let hs = match handle {
            Handle::Round => (if vertical { w } else { h }) as f32,
            Handle::Rect(len) => len,
        };
        if vertical {
            let i = (w as f32 * inset).round();
            let track = RectF::new(i, 0.0, w as f32 - 2.0 * i, h as f32);
            let hy = if flip { v * (h as f32 - hs) } else { (1.0 - v) * (h as f32 - hs) };
            p.fill_rounded(track, radius, [true; 4], back);
            let lit = if flip { RectF::new(i, 0.0, w as f32 - 2.0 * i, hy + hs / 2.0) } else { RectF::new(i, hy + hs / 2.0, w as f32 - 2.0 * i, (h as f32 - hy - hs / 2.0).max(0.0)) };
            p.fill_rounded(lit, radius, [true; 4], hl);
            self.track_decor(p, c, track, radius, true);
            self.draw_handle(p, c, handle, RectF::new(0.0, hy, w as f32, hs), true);
        } else {
            let i = (h as f32 * inset).round();
            let track = RectF::new(0.0, i, w as f32, h as f32 - 2.0 * i);
            let hx = if flip { (1.0 - v) * (w as f32 - hs) } else { v * (w as f32 - hs) };
            p.fill_rounded(track, radius, [true; 4], back);
            let lit = if flip { RectF::new(hx + hs / 2.0, i, (w as f32 - hx - hs / 2.0).max(0.0), h as f32 - 2.0 * i) } else { RectF::new(0.0, i, hx + hs / 2.0, h as f32 - 2.0 * i) };
            p.fill_rounded(lit, radius, [true; 4], hl);
            self.track_decor(p, c, track, radius, false);
            self.draw_handle(p, c, handle, RectF::new(hx, 0.0, hs, h as f32), false);
        }
    }

    /// Opt-in track decoration of sliders, pan and range sliders: a groove
    /// along the middle of the track in `colGroove`, `realGrooveWidth`
    /// (default 2) wide, ending `realGrooveInset` (default 3) from its ends,
    /// and a 1 px frame in `colTrackBorder`.
    fn track_decor(&self, p: &mut Painter, c: &Ctrl, track: RectF, radius: f32, vertical: bool) {
        let st = self.style(c);
        let s = self.styles;
        if let Some(g) = opt_colour(s, st, c, "colGroove") {
            let gw = opt_f32(s, st, c, "realGrooveWidth", 2.0).max(0.0);
            let gi = opt_f32(s, st, c, "realGrooveInset", 3.0).max(0.0);
            let r = if vertical {
                RectF::new((track.x + (track.w - gw) / 2.0).round(), track.y + gi, gw, track.h - 2.0 * gi)
            } else {
                RectF::new(track.x + gi, (track.y + (track.h - gw) / 2.0).round(), track.w - 2.0 * gi, gw)
            };
            p.fill_rect(r, g);
        }
        if let Some(b) = opt_colour(s, st, c, "colTrackBorder") {
            p.stroke_rounded(track.inset(0.5), (radius - 0.5).max(0.0), [true; 4], b, 1.0);
        }
    }

    /// The slider handle (`enumHandleShape`, default round): `Rect` is a
    /// cap across the track `realHandleLength` long (default the track's
    /// thickness `thick`).
    fn handle(&self, c: &Ctrl, thick: f32) -> Handle {
        let st = self.style(c);
        match opt_raw(self.styles, st, c, "enumHandleShape").map(str::trim) {
            Some("Rect" | "rect") => Handle::Rect(opt_f32(self.styles, st, c, "realHandleLength", thick).max(1.0)),
            _ => Handle::Round,
        }
    }

    /// Draw a slider handle in `r` (the cap's rect; round handles are
    /// `r`'s square): round, a colAltBorder ring around colAltFill; a
    /// cap, colAltFill (gradient `colAltFillGradientTo`) framed in
    /// colAltBorder with `intGripLines` (default 1) grip lines 2 px apart
    /// across its middle in `colGrip` (default colAltBorder;
    /// `enumGripShape`: `Squares` or `Along`, see below);
    /// `realHandleRounding` (default 1) rounds its corners.
    fn draw_handle(&self, p: &mut Painter, c: &Ctrl, handle: Handle, r: RectF, vertical: bool) {
        let st = self.style(c);
        let s = self.styles;
        let handle_fill = s.colour(st, "colAltFill", Colour::BLACK);
        let handle_ring = s.colour(st, "colAltBorder", Colour::WHITE);
        match handle {
            Handle::Round => {
                let hs = if vertical { r.w } else { r.h };
                let hr = RectF::new(r.x, r.y, hs, hs);
                p.fill_ellipse(hr, handle_ring);
                p.fill_ellipse(hr.inset((hs * 0.125).ceil()), handle_fill);
            }
            Handle::Rect(_) => {
                // `realHandleInset`: the cap narrower than the control, px
                // off each side across the travel.
                let hi = opt_f32(s, st, c, "realHandleInset", 0.0).max(0.0);
                let r = if vertical { RectF::new(r.x + hi, r.y, r.w - 2.0 * hi, r.h) } else { RectF::new(r.x, r.y + hi, r.w, r.h - 2.0 * hi) };
                let rounding = opt_f32(s, st, c, "realHandleRounding", 1.0).max(0.0).min(r.w.min(r.h) / 2.0);
                match self.gradient(c, "colAltFill", handle_fill, None) {
                    Some(g) => {
                        if let Some(path) = crate::paint::rounded_rect_path(r, rounding, [true; 4]) {
                            p.fill_path_gradient(&path, handle_fill, Gradient { horizontal: !vertical && g.horizontal, ..g });
                        }
                    }
                    None => p.fill_rounded(r, rounding, [true; 4], handle_fill),
                }
                p.stroke_rounded(r.inset(0.5), rounding, [true; 4], handle_ring, 1.0);
                let grip = opt_colour(s, st, c, "colGrip").unwrap_or(handle_ring);
                let n = opt_f32(s, st, c, "intGripLines", 1.0).round().max(0.0) as i32;
                match opt_raw(s, st, c, "enumGripShape").map(str::trim) {
                    // `Squares`: n squares `realGripSize` (default 2) px
                    // centred across the cap, spread evenly along it.
                    Some(v) if v.eq_ignore_ascii_case("squares") => {
                        let g = opt_f32(s, st, c, "realGripSize", 2.0).max(0.0);
                        for k in 0..n {
                            let f = (k as f32 + 0.5) / n as f32;
                            let (x, y) = if vertical { (r.x + (r.w - g) / 2.0, r.y + r.h * f - g / 2.0) } else { (r.x + r.w * f - g / 2.0, r.y + (r.h - g) / 2.0) };
                            p.fill_rect(RectF::new(x.round(), y.round(), g, g), grip);
                        }
                        return;
                    }
                    // `Along`: the lines run along the travel instead of
                    // across it.
                    Some(v) if v.eq_ignore_ascii_case("along") => {
                        for k in 0..n {
                            let off = (k as f32 - (n - 1) as f32 / 2.0) * 2.0;
                            if vertical {
                                let x = (r.x + r.w / 2.0 + off).floor() + 0.5;
                                p.line(x, r.y + 2.0, x, r.y + r.h - 2.0, grip, 1.0);
                            } else {
                                let y = (r.y + r.h / 2.0 + off).floor() + 0.5;
                                p.line(r.x + 2.0, y, r.x + r.w - 2.0, y, grip, 1.0);
                            }
                        }
                        return;
                    }
                    _ => {}
                }
                for k in 0..n {
                    let off = (k as f32 - (n - 1) as f32 / 2.0) * 2.0;
                    if vertical {
                        let y = (r.y + r.h / 2.0 + off).floor() + 0.5;
                        p.line(r.x + 2.0, y, r.x + r.w - 2.0, y, grip, 1.0);
                    } else {
                        let x = (r.x + r.w / 2.0 + off).floor() + 0.5;
                        p.line(x, r.y + 2.0, x, r.y + r.h - 2.0, grip, 1.0);
                    }
                }
            }
        }
    }

    /// A single-channel level strip (`CtrlLevelStrip`): vertical when
    /// taller than wide, filling from the bottom (horizontal: from the
    /// left; `optFlipDirection` reverses either). The level is the
    /// `overlayAlpha` attribute when set, else the value (0..1). The track
    /// is the default fill (colFill / colBorder), the lit part colAltFill.
    /// `intSegmentSize` > 0 draws LED segments that long, `intSegmentGap`
    /// (default 1) apart, unlit ones in colFill and no track.
    fn level_strip(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let s = self.styles;
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let level = c.attr("overlayAlpha").and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(c.value).clamp(0.0, 1.0);
        let vertical = h > w;
        let flip = opt_bool(s, st, c, "optFlipDirection", false);
        let len = if vertical { h } else { w };
        let back = self.colour_attr(st, "colFill");
        let lit = self.colour_attr(st, "colAltFill");
        // The rect of the strip's span [a, b) along its length.
        let span = |a: f32, b: f32| {
            let (a, b) = if flip { (a, b) } else { (len - b, len - a) };
            let (a, b) = if vertical { (a, b) } else { (len - b, len - a) };
            if vertical { RectF::new(0.0, a, w, b - a) } else { RectF::new(a, 0.0, b - a, h) }
        };
        let seg = opt_f32(s, st, c, "intSegmentSize", 0.0).round();
        if seg >= 1.0 {
            let gap = opt_f32(s, st, c, "intSegmentGap", 1.0).round().max(0.0);
            let n = ((len + gap) / (seg + gap)).floor().max(1.0) as usize;
            let on = (n as f32 * level).round() as usize;
            for i in 0..n {
                let a = i as f32 * (seg + gap);
                p.fill_rect(span(a, (a + seg).min(len)), if i < on { lit } else { back });
            }
        } else {
            self.default_fill_colours(p, c, back, self.colour_attr(st, "colBorder"), false);
            let l = (len * level).round();
            if l > 0.0 {
                p.fill_rect(span(0.0, l), lit);
            }
        }
    }

    /// A bipolar pan slider (`CtrlPanSlider`): the slider's track and
    /// handle (round, or `enumHandleShape="Rect"`), lit from the centre to
    /// the value in colFillHighlight, with a centre mark in colBorder
    /// (default colFillHighlight). Vertical when taller than wide (up is
    /// right). Value 0.5 is the centre.
    fn pan_slider(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let s = self.styles;
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let vertical = w < h;
        let inset = c.attr_f32("realTrackInset", 0.3);
        let back = s.colour(st, "colFill", std_col("StandardControlBackground"));
        let hl = s.colour(st, "colFillHighlight", back.darken(0.7));
        let mark = s.colour(st, "colBorder", hl);
        let mark = if mark.a == 0 { hl } else { mark };
        let radius = s.float(st, "realCornerRounding", 0.0).round();
        let v = c.value.clamp(0.0, 1.0);
        let (len, thick) = if vertical { (h, w) } else { (w, h) };
        let handle = self.handle(c, thick);
        let hs = match handle {
            Handle::Round => thick,
            Handle::Rect(l) => l,
        };
        let i = (thick * inset).round();
        // Handle start along the length, the centre and the handle's middle.
        let pos = if vertical { (1.0 - v) * (len - hs) } else { v * (len - hs) };
        let mid = len / 2.0;
        let at = pos + hs / 2.0;
        let (a, b) = if at < mid { (at, mid) } else { (mid, at) };
        if vertical {
            p.fill_rounded(RectF::new(i, 0.0, w - 2.0 * i, h), radius, [true; 4], back);
            p.fill_rect(RectF::new(i, a, w - 2.0 * i, b - a), hl);
            self.track_decor(p, c, RectF::new(i, 0.0, w - 2.0 * i, h), radius, true);
            p.line(0.0, mid.floor() + 0.5, w, mid.floor() + 0.5, mark, 1.0);
            self.draw_handle(p, c, handle, RectF::new(0.0, pos, w, hs), true);
        } else {
            p.fill_rounded(RectF::new(0.0, i, w, h - 2.0 * i), radius, [true; 4], back);
            p.fill_rect(RectF::new(a, i, b - a, h - 2.0 * i), hl);
            self.track_decor(p, c, RectF::new(0.0, i, w, h - 2.0 * i), radius, false);
            p.line(mid.floor() + 0.5, 0.0, mid.floor() + 0.5, h, mark, 1.0);
            self.draw_handle(p, c, handle, RectF::new(pos, 0.0, hs, h), false);
        }
    }

    /// A range slider (`CtrlRangeSlider`): the slider's track with two
    /// handles (round, or `enumHandleShape="Rect"`) at the `valueLow` and
    /// `valueHigh` attributes (0..1, default 0 and 1), lit between them in
    /// colFillHighlight. Vertical when taller than wide (up is more).
    /// Opt-in `enumHandleShape="Triangles"` (horizontal ones): the track
    /// fills the rect, the range lit inset 4 px across, and the handles are
    /// markers `realHandleLength` (default 11) wide and about half the
    /// height, the high one ▼ from the top, the low one ▲ from the bottom,
    /// colAltFill outlined in colAltBorder.
    fn range_slider(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let s = self.styles;
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let vertical = w < h;
        if !vertical && opt_raw(s, st, c, "enumHandleShape").is_some_and(|v| v.trim().eq_ignore_ascii_case("triangles")) {
            return self.range_markers(p, c);
        }
        let inset = c.attr_f32("realTrackInset", 0.3);
        let back = s.colour(st, "colFill", std_col("StandardControlBackground"));
        let hl = s.colour(st, "colFillHighlight", back.darken(0.7));
        let radius = s.float(st, "realCornerRounding", 0.0).round();
        let (lo, hi) = range_values(c);
        let (len, thick) = if vertical { (h, w) } else { (w, h) };
        let handle = self.handle(c, thick);
        let hs = match handle {
            Handle::Round => thick,
            Handle::Rect(l) => l,
        };
        let i = (thick * inset).round();
        let pos = |v: f32| if vertical { (1.0 - v) * (len - hs) } else { v * (len - hs) };
        let (a, b) = {
            let (pa, pb) = (pos(lo) + hs / 2.0, pos(hi) + hs / 2.0);
            if pa < pb { (pa, pb) } else { (pb, pa) }
        };
        if vertical {
            p.fill_rounded(RectF::new(i, 0.0, w - 2.0 * i, h), radius, [true; 4], back);
            p.fill_rect(RectF::new(i, a, w - 2.0 * i, b - a), hl);
            self.track_decor(p, c, RectF::new(i, 0.0, w - 2.0 * i, h), radius, true);
            for v in [lo, hi] {
                self.draw_handle(p, c, handle, RectF::new(0.0, pos(v), w, hs), true);
            }
        } else {
            p.fill_rounded(RectF::new(0.0, i, w, h - 2.0 * i), radius, [true; 4], back);
            p.fill_rect(RectF::new(a, i, b - a, h - 2.0 * i), hl);
            self.track_decor(p, c, RectF::new(0.0, i, w, h - 2.0 * i), radius, false);
            for v in [lo, hi] {
                self.draw_handle(p, c, handle, RectF::new(pos(v), 0.0, hs, h), false);
            }
        }
    }

    /// [`Renderer::range_slider`]'s `Triangles` handles.
    fn range_markers(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let s = self.styles;
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let back = s.colour(st, "colFill", std_col("StandardControlBackground"));
        let hl = s.colour(st, "colFillHighlight", back.darken(0.7));
        let radius = s.float(st, "realCornerRounding", 0.0).round().min(h / 2.0);
        let (lo, hi) = range_values(c);
        let hs = opt_f32(s, st, c, "realHandleLength", 11.0).clamp(1.0, w);
        let mh = (h * 0.47).round();
        let at = |v: f32| v.clamp(0.0, 1.0) * (w - hs) + hs / 2.0;
        let track = RectF::new(0.0, 0.0, w, h);
        p.fill_rounded(track, radius, [true; 4], back);
        let (a, b) = if at(lo) < at(hi) { (at(lo), at(hi)) } else { (at(hi), at(lo)) };
        p.fill_rect(RectF::new(a, 4.0, b - a, h - 8.0), hl);
        self.track_decor(p, c, track, radius, false);
        let fill = s.colour(st, "colAltFill", Colour::WHITE);
        let edge = s.colour(st, "colAltBorder", Colour::BLACK);
        for (v, down) in [(hi, true), (lo, false)] {
            let x = (at(v) - hs / 2.0).round() + 0.5;
            let pts = if down { [(x, 0.5), (x + hs - 1.0, 0.5), (x + (hs - 1.0) / 2.0, mh)] } else { [(x, h - 0.5), (x + hs - 1.0, h - 0.5), (x + (hs - 1.0) / 2.0, h - mh)] };
            p.fill_polygon(&pts, fill);
            p.polyline(&[pts[0], pts[1], pts[2], pts[0]], edge, 1.0);
        }
    }

    /// A plot (`CtrlPlot`): the default fill, then a polyline in colLine
    /// (default colText), `realLineWidth` (default 1.5) wide, inside the rect
    /// inset by `realPlotInset` (default 0). The curve is the `points`
    /// attribute, `x,y` pairs (0..1, y up) separated by spaces; without
    /// it a flat line across the middle.
    ///
    /// Opt-in, several curves ([re:b732af]: one polyline per Y vector, in
    /// the order added, each in its own colour): a `curves` attribute holds
    /// them separated by `;`, each `colour|points` with the colour a
    /// registered name or literal and the points as above. With `curves`
    /// set (even empty) `points` and the flat line are not drawn.
    fn plot(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let s = self.styles;
        self.default_fill(p, c, "colFill", "colBorder", false);
        let area = Self::bounds(c).inset(opt_f32(s, st, c, "realPlotInset", 0.0).max(0.0));
        let line = s.colour(st, "colLine", self.colour_attr(st, "colText"));
        let lw = opt_f32(s, st, c, "realLineWidth", 1.5);
        let parse = |v: &str| -> Vec<(f32, f32)> {
            v.split_whitespace()
                .filter_map(|pt| {
                    let (x, y) = pt.split_once(',')?;
                    Some((x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?))
                })
                .map(|(x, y)| (area.x + x * area.w, area.y + (1.0 - y) * area.h))
                .collect()
        };
        if let Some(curves) = c.attr("curves") {
            for curve in curves.split(';') {
                let Some((col, pts)) = curve.split_once('|') else { continue };
                let pts = parse(pts);
                if pts.len() >= 2 {
                    p.polyline(&pts, s.parse_colour(col), lw);
                }
            }
            return;
        }
        let mut pts = parse(c.attr("points").unwrap_or(""));
        if pts.len() < 2 {
            pts = parse("0,0.5 1,0.5");
        }
        p.polyline(&pts, line, lw);
    }

    /// A needle meter (`CtrlMeter` with a `CtrlMeterStyle`): the default
    /// fill, a scale arc with five ticks in colAltBorder, range texts
    /// (`textLeftRange` / `textRightRange`) at its ends and `textUnits`
    /// inside it in colText, and a needle in colLine (default colText,
    /// `realLineWidth` default 1.5) at the value (0..1). The pivot is at
    /// (`realPivotX`, `realPivotY`) × the rect (default 0.5, 1), the radius
    /// `realRadius` × the smaller side (default 0.9); the scale starts at
    /// `realAngleStart` degrees (counter-clockwise from 3 o'clock, default
    /// 135) and sweeps `realAngleSize` (default 90) clockwise.
    fn needle_meter(&self, p: &mut Painter, c: &Ctrl) {
        use crate::style::{HAlign, VAlign};
        let st = self.style(c);
        let s = self.styles;
        self.default_fill(p, c, "colFill", "colBorder", false);
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let o = |k: &str, d: f32| opt_f32(s, st, c, k, d);
        let (px, py) = (o("realPivotX", 0.5) * w, o("realPivotY", 1.0) * h);
        let rad = o("realRadius", 0.9) * w.min(h);
        // Screen angles (degrees clockwise from 3 o'clock, y down).
        let a0 = -o("realAngleStart", 135.0);
        let sweep = o("realAngleSize", 90.0);
        let scale = self.colour_attr(st, "colAltBorder");
        let text = self.colour_attr(st, "colText");
        p.stroke_arc(RectF::new(px - rad, py - rad, 2.0 * rad, 2.0 * rad), a0.rem_euclid(360.0), sweep, true, scale, 1.0);
        let at = |v: f32, r: f32| {
            let a = (a0 + v * sweep).to_radians();
            (px + a.cos() * r, py + a.sin() * r)
        };
        // Opt-in `colBaseBand`: a band `realBaseBandHeight` (default 5) px
        // high along the bottom, inside the border, with a half disc of the
        // band's height as radius bulging up from it under the pivot.
        if let Some(band) = opt_colour(s, st, c, "colBaseBand") {
            let bh = o("realBaseBandHeight", 5.0).max(0.0);
            let bw = if s.bool(st, "optDrawBorder", true) { s.float(st, "realBorderWidth", 1.0).max(0.0) } else { 0.0 };
            p.fill_rect(RectF::new(bw, h - bw - bh, w - 2.0 * bw, bh), band);
            p.fill_ellipse(RectF::new(px - bh, h - bw - 2.0 * bh, 2.0 * bh, 2.0 * bh), band);
        }
        // `intTicks` (default 5): the marks along the scale (0: none).
        let ticks = o("intTicks", 5.0).round().max(0.0) as i32;
        for k in 0..ticks {
            let v = if ticks > 1 { k as f32 / (ticks - 1) as f32 } else { 0.5 };
            let (x0, y0) = at(v, rad - 4.0);
            let (x1, y1) = at(v, rad);
            p.line(x0, y0, x1, y1, scale, 1.0);
        }
        let font = s.font(st);
        for (k, v, ha) in [("textLeftRange", 0.0, HAlign::Left), ("textRightRange", 1.0, HAlign::Right)] {
            if let Some(t) = c.attr(k).or_else(|| s.raw(st, k)).filter(|t| !t.is_empty()) {
                let (x, y) = at(v, rad - 6.0);
                let r = if ha == HAlign::Left { RectF::new(x, y, 30.0, 12.0) } else { RectF::new(x - 30.0, y, 30.0, 12.0) };
                p.text(r, t, text, &font, ha, VAlign::Top);
            }
        }
        if let Some(t) = c.attr("textUnits").or_else(|| s.raw(st, "textUnits")).filter(|t| !t.is_empty()) {
            let (_, y) = at(0.5, rad * 0.55);
            p.text(RectF::new(px - 30.0, y - 6.0, 60.0, 12.0), t, text, &font, HAlign::Center, VAlign::Center);
        }
        let needle = s.colour(st, "colLine", text);
        let (x, y) = at(c.value.clamp(0.0, 1.0), rad * 0.95);
        p.line(px, py, x, y, needle, o("realLineWidth", 1.5));
    }

    /// A table or list box with a table style (see [`Renderer::native_table`]):
    /// the default fill; with `optHeader` (default true for tables) a
    /// header row `intHeaderHeight` (default 15) high in colHeaderFill with
    /// the `columns` titles (`|`-separated) in colHeaderText and a line
    /// under it in colHeaderDivider; then rows `intRowHeight` (default 16,
    /// else `intLineHeight`) high: the `rows` attribute (one row per line,
    /// cells `|`-separated), else the text of the label children. Rows fill
    /// colRow (`optAltRowColours`: every other one colRowAlt); the selected
    /// row (`intSelectedRow`, or a label child that is on) colSelection.
    /// `optColumnDividers` / `optRowDividers` draw colColumnDivider /
    /// colRowDivider (default colColumnDivider) lines (dashed with
    /// `intDividerDashLength`). Column widths:
    /// `columnWidths` (comma-separated pixels, the last column takes the
    /// rest), else equal. No scrolling: rows past the bottom are clipped.
    fn table(&self, p: &mut Painter, c: &Ctrl) {
        use crate::style::{HAlign, Shorten, VAlign};
        let st = self.style(c);
        let s = self.styles;
        self.default_fill(p, c, "colFill", "colBorder", false);
        let b = Self::bounds(c);
        let col = |k: &str| s.raw(st, k).map(|v| s.parse_colour(v));
        let text = self.colour_attr(st, "colText");
        let font = s.font(st);
        let header = opt_bool(s, st, c, "optHeader", c.class == "CtrlTableView");
        let hh = if header { opt_f32(s, st, c, "intHeaderHeight", 15.0) } else { 0.0 };
        let rh = opt_f32(s, st, c, "intRowHeight", opt_f32(s, st, c, "intLineHeight", 16.0)).max(1.0);
        let titles: Vec<&str> = c.attr("columns").map(|v| v.split('|').collect()).unwrap_or_default();
        let rows: Vec<(Vec<String>, bool)> = match c.attr("rows") {
            Some(v) => v.split('\n').map(|r| (r.split('|').map(str::to_string).collect(), false)).collect(),
            None => c.children.iter().filter(|k| k.class == "CtrlLabel" && k.visible).map(|k| (k.text.split('|').map(str::to_string).collect(), k.on)).collect(),
        };
        let ncols = titles.len().max(rows.iter().map(|r| r.0.len()).max().unwrap_or(1)).max(1);
        let mut xs = Vec::with_capacity(ncols + 1);
        let widths: Vec<f32> = c.attr("columnWidths").map(|v| v.split(',').filter_map(|t| t.trim().parse().ok()).collect()).unwrap_or_default();
        let mut x = 0.0;
        for i in 0..ncols {
            xs.push(x);
            x += widths.get(i).copied().unwrap_or((b.w - x) / (ncols - i) as f32);
        }
        xs.push(b.w);
        let cell = |p: &mut Painter, i: usize, y: f32, h: f32, t: &str, colour: Colour| {
            let r = RectF::new(xs[i] + 3.0, y, (xs[i + 1] - xs[i] - 6.0).max(0.0), h);
            let t = p.fonts.shorten(&font, t, r.w, Shorten::End);
            p.text(r, &t, colour, &font, HAlign::Left, VAlign::Center);
        };
        let col_div = col("colColumnDivider");
        let selected = c.attr_i32("intSelectedRow", -1);
        let alt = opt_bool(s, st, c, "optAltRowColours", false);
        let row_div = col("colRowDivider").or(col_div);
        // Dashed row / column dividers: `intDividerDashLength` /
        // `intDividerDashGap`, else the border's `intDashLength` / `intDashGap`.
        let div_dash = self.dash(c, "intDividerDashLength", "intDividerDashGap").or_else(|| self.dash(c, "intDashLength", "intDashGap"));
        for (n, (cells, on)) in rows.iter().enumerate() {
            let y = hh + n as f32 * rh;
            if y >= b.h {
                break;
            }
            let fill = if *on || n as i32 == selected {
                col("colSelection")
            } else if alt && n % 2 == 1 {
                col("colRowAlt")
            } else {
                col("colRow")
            };
            if let Some(f) = fill {
                p.fill_rect(RectF::new(0.0, y, b.w, rh), f);
            }
            for (i, t) in cells.iter().enumerate().take(ncols) {
                cell(p, i, y, rh, t, text);
            }
            if let Some(d) = row_div.filter(|_| opt_bool(s, st, c, "optRowDividers", false)) {
                p.dash = div_dash;
                p.line(0.0, (y + rh).floor() - 0.5, b.w, (y + rh).floor() - 0.5, d, 1.0);
                p.dash = None;
            }
        }
        if let Some(d) = col_div.filter(|_| opt_bool(s, st, c, "optColumnDividers", false)) {
            p.dash = div_dash;
            for x in &xs[1..ncols] {
                p.line(x.floor() + 0.5, hh, x.floor() + 0.5, b.h, d, 1.0);
            }
            p.dash = None;
        }
        if header {
            if let Some(f) = col("colHeaderFill") {
                p.fill_rect(RectF::new(0.0, 0.0, b.w, hh), f);
            }
            let ht = col("colHeaderText").unwrap_or(text);
            for (i, t) in titles.iter().enumerate() {
                cell(p, i, 0.0, hh, t, ht);
            }
            if let Some(d) = col("colHeaderDivider") {
                p.line(0.0, hh - 0.5, b.w, hh - 0.5, d, 1.0);
                for x in &xs[1..ncols] {
                    p.line(x.floor() + 0.5, 0.0, x.floor() + 0.5, hh, d, 1.0);
                }
            }
        }
    }

    /// [re:e0ce2b]: body in
    /// colFill, header strip in colAltFill with the display name in colText,
    /// border in colBorder.
    fn collapse(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let s = self.styles;
        let b = Self::bounds(c);
        let radius = s.float(st, "realCornerRounding", 0.0);
        let corners = s.rounding_options(st);
        p.fill_rounded(b, radius, corners, s.colour(st, "colFill", std_col("StandardControlBackground")));
        // Header strip (width × intIconicDim, or the iconic
        // dim × height when folded to the left / right); folded, its bottom
        // corners lose their rounding.
        let hr = crate::collapse::header(c);
        let head = RectF::new(hr.x as f32, hr.y as f32, hr.w as f32, hr.h as f32);
        let mut hc = corners;
        if crate::collapse::is_iconic(c) {
            hc[2] = false;
            hc[3] = false;
        }
        p.fill_rounded(head, radius, hc, s.colour(st, "colAltFill", std_col("StandardControlHighlight")));
        if crate::collapse::vertical_text(c) {
            // Rotated −90° about (0, h) into the rect (0, h, h, w).
            let font = s.font(st);
            let col = s.colour(st, "colText", std_col("StandardControlText"));
            p.text_up(0.0, head.h, head.h, head.w, &c.display_name, col, &font, s.h_align(st), s.v_align(st));
        } else {
            self.draw_text(p, c, head, &c.display_name, "colText");
        }
        if s.bool(st, "optDrawBorder", true) {
            p.stroke_rounded(b.inset(0.5), radius, corners, s.colour(st, "colBorder", std_col("StandardControlBorder")), s.float(st, "realBorderWidth", 1.0));
        }
    }

    /// [re:9c1fd5]: each visible child's
    /// display name next to it (position from `enumTextPosition`, default
    /// Below; gap `intTextOffset`, default 2).
    fn labeller_labels(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let font = self.styles.font(st);
        let colour = self.styles.colour(st, "colText", std_col("StandardControlText"));
        let off = c.attr_i32("intTextOffset", 2) as f32;
        let pos = c.attr("enumTextPosition").unwrap_or("Below");
        let h_default = self.styles.h_align(st);
        for ch in c.children.iter().filter(|k| k.visible && !k.display_name.is_empty()) {
            let (tw, th) = p.fonts.measure(&font, &ch.display_name);
            let lw = tw.round() + 2.0;
            let lh = th.round().max(20.0);
            let r = &ch.rect;
            use crate::style::{HAlign, VAlign};
            let (x, y, ha, va) = match pos {
                "Above" => ((r.x + r.w / 2) as f32 - lw * 0.5, r.y as f32 - lh - off, h_default, VAlign::Bottom),
                "Left" => (r.x as f32 - lw - off, (r.y + r.h / 2) as f32 - lh * 0.5, HAlign::Right, VAlign::Center),
                "Right" => (r.right() as f32 + off, (r.y + r.h / 2) as f32 - lh * 0.5, HAlign::Left, VAlign::Center),
                _ => ((r.x + r.w / 2) as f32 - lw * 0.5, r.bottom() as f32 + off, h_default, VAlign::Top),
            };
            p.text(RectF::new(x, y, lw, lh), &ch.display_name, colour, &font, ha, va);
        }
    }
}

/// A range slider's (`valueLow`, `valueHigh`), clamped to 0..1 and ordered.
pub fn range_values(c: &Ctrl) -> (f32, f32) {
    let lo = c.attr_f32("valueLow", 0.0).clamp(0.0, 1.0);
    let hi = c.attr_f32("valueHigh", 1.0).clamp(0.0, 1.0);
    if lo <= hi { (lo, hi) } else { (hi, lo) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Style;

    /// A stylesheet with `name` of element class `class` holding `attrs`.
    fn sheet(styles: &[(&str, &str, &[(&str, &str)])]) -> StyleSheet {
        let mut s = StyleSheet::new();
        for (name, class, attrs) in styles {
            let mut st = Style { name: name.to_string(), class: class.to_string(), parents: vec!["ApplicationStyle".into()], ..Style::default() };
            for (k, v) in *attrs {
                st.attrs.insert(k.to_string(), v.to_string());
            }
            s.styles.insert(name.to_string(), st);
        }
        s
    }

    fn ctrl(class: &str, style: &str, w: i32, h: i32, attrs: &[(&str, &str)]) -> Ctrl {
        let mut c = Ctrl { class: class.into(), style: style.into(), rect: crate::ctrl::Rect::new(0, 0, w, h), visible: true, enabled: true, ..Ctrl::default() };
        for (k, v) in attrs {
            c.set_attr(k, v.to_string());
        }
        c
    }

    /// RGBA bytes of `c` drawn on a black pixmap.
    fn draw(s: &StyleSheet, c: &Ctrl) -> Vec<u8> {
        let mut pm = tiny_skia::Pixmap::new(c.rect.w as u32, c.rect.h as u32).unwrap();
        pm.fill(tiny_skia::Color::BLACK);
        let mut fonts = crate::text::Fonts::new(std::path::Path::new("/nonexistent"));
        {
            let mut p = Painter::new(&mut pm, &mut fonts, 1.0);
            Renderer::new(s).draw(&mut p, c);
        }
        pm.data().to_vec()
    }

    fn painted(px: &[u8]) -> usize {
        px.chunks(4).filter(|p| p[..3] != [0, 0, 0]).count()
    }

    /// Draws something, and a different value draws differently.
    fn check(s: &StyleSheet, mut c: Ctrl, v0: f32, v1: f32) -> (Vec<u8>, Vec<u8>) {
        c.value = v0;
        let a = draw(s, &c);
        c.value = v1;
        let b = draw(s, &c);
        assert!(painted(&a) > 0 && painted(&b) > 0, "{} draws nothing", c.class);
        assert_ne!(a, b, "{}: the value changes nothing", c.class);
        (a, b)
    }

    const COLOURS: &[(&str, &str)] = &[
        ("colFill", "40, 40, 40, 255"),
        ("colAltFill", "0, 255, 0, 255"),
        ("colFillHighlight", "255, 0, 0, 255"),
        ("colBorder", "90, 90, 90, 255"),
        ("colAltBorder", "255, 255, 255, 255"),
        ("colText", "200, 200, 0, 255"),
    ];

    #[test]
    fn level_strips_fill_with_the_level() {
        let s = sheet(&[("Vu", "CtrlStyle", COLOURS), ("Led", "CtrlStyle", &[("colFill", "40, 40, 40, 255"), ("colAltFill", "0, 255, 0, 255"), ("intSegmentSize", "3")])]);
        let lit = |px: &[u8]| px.chunks(4).filter(|p| p[..3] == [0, 255, 0]).count();
        // Vertical: from the bottom.
        let (a, b) = check(&s, ctrl("CtrlLevelStrip", "Vu", 4, 40, &[]), 0.25, 0.75);
        assert!(lit(&b) > lit(&a));
        assert_eq!(&b[(39 * 4) * 4..(39 * 4 + 1) * 4], &[0, 255, 0, 255], "bottom row lit");
        assert_ne!(&b[0..4], &[0, 255, 0, 255], "top row unlit");
        // Horizontal, from overlayAlpha (over the value), and LED segments.
        let c = ctrl("CtrlLevelStrip", "Vu", 40, 4, &[("overlayAlpha", "0.5")]);
        let px = draw(&s, &c);
        assert_eq!(&px[(40 + 1) * 4..(40 + 2) * 4], &[0, 255, 0, 255], "left lit");
        assert_ne!(&px[(40 + 38) * 4..(40 + 39) * 4], &[0, 255, 0, 255], "right unlit");
        let (_, led) = check(&s, ctrl("CtrlLevelStrip", "Led", 4, 39, &[]), 0.0, 1.0);
        // 3 px segments 1 apart: every fourth row is a gap.
        assert_eq!(&led[(3 * 4) * 4..(3 * 4 + 1) * 4], &[0, 0, 0, 255]);
        assert_eq!(&led[(4 * 4) * 4..(4 * 4 + 1) * 4], &[0, 255, 0, 255]);
    }

    #[test]
    fn pan_sliders_fill_from_the_centre() {
        let s = sheet(&[("Pan", "CtrlStyle", COLOURS), ("PanRect", "CtrlStyle", &[COLOURS, &[("enumHandleShape", "Rect"), ("realHandleLength", "8")]].concat())]);
        let red = |px: &[u8]| px.chunks(4).filter(|p| p[..3] == [255, 0, 0]).count();
        let (centre, right) = check(&s, ctrl("CtrlPanSlider", "Pan", 73, 12, &[]), 0.5, 0.9);
        let (_, left) = check(&s, ctrl("CtrlPanSlider", "Pan", 73, 12, &[]), 0.5, 0.1);
        assert!(red(&right) > red(&centre) && red(&left) > red(&centre));
        // A lit pixel right of the centre, none left of it (row 6).
        let at = |px: &[u8], x: usize| px[(6 * 73 + x) * 4..(6 * 73 + x) * 4 + 3].to_vec();
        assert_eq!(at(&right, 45), [255, 0, 0]);
        assert_ne!(at(&right, 30), [255, 0, 0]);
        check(&s, ctrl("CtrlPanSlider", "PanRect", 73, 12, &[]), 0.2, 0.8);
    }

    #[test]
    fn fader_caps_and_flipping_are_opt_in() {
        let s = sheet(&[("Fader", "CtrlStyle", COLOURS), ("Cap", "CtrlStyle", &[COLOURS, &[("enumHandleShape", "Rect"), ("realHandleLength", "16")]].concat())]);
        let round = check(&s, ctrl("CtrlSlider", "Fader", 15, 75, &[]), 0.2, 0.8).1;
        let cap = check(&s, ctrl("CtrlSlider", "Cap", 15, 75, &[]), 0.2, 0.8).1;
        assert_ne!(round, cap);
        // The cap's corner pixel (a rect, not a disc) at value 1: top row.
        let mut c = ctrl("CtrlSlider", "Cap", 15, 75, &[]);
        c.value = 1.0;
        assert_eq!(&draw(&s, &c)[7 * 4..8 * 4], &[255, 255, 255, 255]);
        // Flipped: value 1 sits at the bottom.
        let mut f = ctrl("CtrlSlider", "Cap", 15, 75, &[("optFlipDirection", "true")]);
        f.value = 1.0;
        let px = draw(&s, &f);
        assert_eq!(&px[(74 * 15 + 7) * 4..(74 * 15 + 8) * 4], &[255, 255, 255, 255]);
        assert_ne!(px, draw(&s, &c));
    }

    #[test]
    fn needle_meters_need_a_meter_style() {
        let meter = [COLOURS, &[("realPivotY", "1.1"), ("colLine", "255, 0, 255, 255")]].concat();
        let s = sheet(&[("Needle", "CtrlMeterStyle", &meter), ("Plain", "CtrlStyle", &meter)]);
        check(&s, ctrl("CtrlMeter", "Needle", 107, 50, &[]), 0.0, 1.0);
        // Any other style: the default fill only, whatever the value.
        let mut c = ctrl("CtrlMeter", "Plain", 107, 50, &[]);
        let a = draw(&s, &c);
        c.value = 1.0;
        assert_eq!(a, draw(&s, &c));
    }

    #[test]
    fn needle_meter_ticks_and_base_band_are_opt_in() {
        let meter = [COLOURS, &[("realPivotY", "1.1"), ("colLine", "255, 0, 255, 255"), ("optDrawBorder", "false")]].concat();
        let band = [&meter[..], &[("colBaseBand", "0, 255, 0, 255"), ("realBaseBandHeight", "5"), ("intTicks", "0")]].concat();
        let s = sheet(&[("Needle", "CtrlMeterStyle", &meter), ("Band", "CtrlMeterStyle", &band)]);
        let green = |px: &[u8]| px.chunks(4).filter(|p| p[1] == 255 && p[0] == 0 && p[2] == 0).count();
        let plain = draw(&s, &ctrl("CtrlMeter", "Needle", 107, 50, &[]));
        assert_eq!(green(&plain), 0, "no band by default");
        let b = draw(&s, &ctrl("CtrlMeter", "Band", 107, 50, &[]));
        assert_eq!(px(&b, 107, 3, 47), [0, 255, 0], "the band along the bottom");
        assert_eq!(px(&b, 107, 3, 44)[1], px(&plain, 107, 3, 44)[1], "5 px high");
        assert_eq!(px(&b, 107, 53, 41), [0, 255, 0], "the bump under the pivot");
        // The scale's marks go with intTicks 0; 5 is the default.
        let five = [&meter[..], &[("intTicks", "5")]].concat();
        let none = [&meter[..], &[("intTicks", "0")]].concat();
        let s2 = sheet(&[("Needle", "CtrlMeterStyle", &meter), ("Five", "CtrlMeterStyle", &five), ("None", "CtrlMeterStyle", &none)]);
        let m = |st: &str| draw(&s2, &ctrl("CtrlMeter", st, 107, 50, &[]));
        assert_eq!(m("Needle"), m("Five"));
        assert_ne!(m("Needle"), m("None"));
    }

    #[test]
    fn plots_draw_their_points() {
        let s = sheet(&[("Plot", "CtrlStyle", &[COLOURS, &[("colLine", "255, 0, 255, 255"), ("realPlotInset", "2")]].concat())]);
        let magenta = |px: &[u8]| px.chunks(4).filter(|p| p[0] > 128 && p[2] > 128 && p[1] < 64).count();
        let flat = draw(&s, &ctrl("CtrlPlot", "Plot", 107, 50, &[]));
        let curve = draw(&s, &ctrl("CtrlPlot", "Plot", 107, 50, &[("points", "0,0.2 0.5,0.9 1,0.2")]));
        assert_ne!(flat, curve);
        assert!(magenta(&flat) > 0 && magenta(&curve) > 0);
        // The curve's peak (x 0.5, y 0.9 of the inset area) is on it.
        let at = |px: &[u8], x: usize, y: usize| px[(y * 107 + x) * 4];
        assert!(at(&curve, 53, 6) > 128 && at(&flat, 53, 6) < 128);
    }

    #[test]
    fn plots_draw_coloured_curves_when_asked() {
        let s = sheet(&[("Plot", "CtrlStyle", &[COLOURS, &[("colLine", "255, 0, 255, 255")]].concat())]);
        let at = |px: &[u8], x: usize, y: usize| px[(y * 107 + x) * 4..(y * 107 + x) * 4 + 3].to_vec();
        // Two curves in their own colours; no flat line in colLine.
        let two = draw(&s, &ctrl("CtrlPlot", "Plot", 107, 50, &[("curves", "0, 255, 0, 255|0,0.9 1,0.9;0, 0, 255, 255|0,0.1 1,0.1")]));
        assert!(at(&two, 53, 5)[1] > 128 && at(&two, 53, 5)[2] < 64);
        assert!(at(&two, 53, 45)[2] > 128 && at(&two, 53, 45)[1] < 64);
        assert!(two.chunks(4).all(|p| !(p[0] > 128 && p[2] > 128 && p[1] < 64)));
        // Empty: nothing but the fill.
        let none = draw(&s, &ctrl("CtrlPlot", "Plot", 107, 50, &[("curves", "")]));
        assert_eq!(at(&none, 53, 25), at(&none, 53, 5));
    }

    #[test]
    fn tables_draw_rows_and_the_selection() {
        let t =
            [COLOURS, &[("colRow", "30, 30, 120, 255"), ("colSelection", "200, 120, 0, 255"), ("colHeaderFill", "0, 90, 90, 255"), ("colHeaderDivider", "255, 255, 255, 255"), ("intRowHeight", "12")]]
                .concat();
        let s = sheet(&[("Table", "CtrlTableViewStyle", &t), ("Panel", "CtrlStyle", &t)]);
        let at = |px: &[u8], x: usize, y: usize| px[(y * 100 + x) * 4..(y * 100 + x) * 4 + 3].to_vec();
        let mut c = ctrl("CtrlTableView", "Table", 100, 60, &[("columns", "A|B"), ("rows", "one|1\ntwo|2")]);
        let a = draw(&s, &c);
        assert_eq!(at(&a, 20, 5), [0, 90, 90], "header");
        assert_eq!(at(&a, 50, 20), [30, 30, 120], "row 0");
        assert_eq!(at(&a, 50, 50), [40, 40, 40], "below the rows");
        c.set_attr("intSelectedRow", "1".into());
        assert_eq!(at(&draw(&s, &c), 50, 32), [200, 120, 0], "row 1 selected");
        // A list box: no header; rows from label children, which are not
        // drawn themselves.
        let mut l = ctrl("CtrlListBox", "Table", 100, 60, &[]);
        let mut row = ctrl("CtrlLabel", "Panel", 100, 60, &[]);
        row.text = "file".into();
        row.on = true;
        l.children.push(row);
        let px = draw(&s, &l);
        assert_eq!(at(&px, 50, 5), [200, 120, 0], "selected first row at the top");
        assert_eq!(at(&px, 50, 30), [40, 40, 40], "the label child is not drawn");
        // Without a table style: the default fill, children drawn.
        let mut d = l.clone();
        d.style = "Panel".into();
        assert_eq!(at(&draw(&s, &d), 50, 30), [40, 40, 40]);
        assert_ne!(at(&draw(&s, &d), 50, 5), [200, 120, 0]);
    }

    #[test]
    fn knob_dials_align_in_tall_rects() {
        let s = sheet(&[("Knob", "CtrlStyle", COLOURS), ("Top", "CtrlStyle", &[COLOURS, &[("enumDialAlign", "Top")]].concat())]);
        let (stretched, _) = check(&s, ctrl("CtrlKnob", "Knob", 40, 60, &[]), 0.2, 0.8);
        let (top, _) = check(&s, ctrl("CtrlKnob", "Top", 40, 60, &[]), 0.2, 0.8);
        assert_ne!(stretched, top);
        // Top-aligned: nothing below the 40 px dial.
        assert_eq!(painted(&top[40 * 40 * 4..]), 0);
        // Default: the same pixels as before (dial spans the rect).
        assert!(painted(&stretched[40 * 40 * 4..]) > 0);
    }

    #[test]
    fn text_options() {
        let s = sheet(&[
            ("B", "CtrlStyle", COLOURS),
            ("NoText", "CtrlStyle", &[COLOURS, &[("optDrawText", "false")]].concat()),
            ("Dis", "CtrlStyle", &[COLOURS, &[("colTextDisabled", "255, 0, 0, 255")]].concat()),
        ]);
        let r = Renderer::new(&s);
        let b = ctrl("CtrlButton", "B", 10, 10, &[]);
        assert!(r.draws_text(&b));
        assert!(!r.draws_text(&ctrl("CtrlButton", "NoText", 10, 10, &[])));
        // The built-in InvisibleStyle's optDrawText is not honoured.
        assert!(r.draws_text(&ctrl("CtrlLabel", "InvisibleStyle", 10, 10, &[])));
        assert_eq!(s.raw("Dis", "colTextDisabled"), Some("255, 0, 0, 255"));
    }

    fn px(px: &[u8], w: usize, x: usize, y: usize) -> [u8; 3] {
        let i = (y * w + x) * 4;
        [px[i], px[i + 1], px[i + 2]]
    }

    #[test]
    fn fills_take_gradients() {
        let s = sheet(&[
            ("Flat", "CtrlStyle", &[("colFill", "255, 0, 0, 255"), ("optDrawBorder", "false")]),
            ("V", "CtrlStyle", &[("colFill", "255, 0, 0, 255"), ("colFillGradientTo", "0, 0, 255, 255"), ("optDrawBorder", "false")]),
            ("H", "CtrlStyle", &[("colFill", "255, 0, 0, 255"), ("colFillGradientTo", "0, 0, 255, 255"), ("enumGradientDirection", "Horizontal"), ("optDrawBorder", "false")]),
            ("R", "CtrlStyle", &[("colFill", "255, 0, 0, 255"), ("colFillGradientTo", "0, 0, 255, 255"), ("realCornerRounding", "4")]),
            ("On", "CtrlStyle", &[("colAltFill", "0, 255, 0, 255"), ("colAltFillGradientTo", "0, 0, 0, 255"), ("optDrawBorder", "false")]),
        ]);
        let flat = draw(&s, &ctrl("CtrlPanel", "Flat", 20, 20, &[]));
        assert_eq!(px(&flat, 20, 10, 0), px(&flat, 20, 10, 19));
        let v = draw(&s, &ctrl("CtrlPanel", "V", 20, 20, &[]));
        assert!(px(&v, 20, 10, 0)[0] > 200 && px(&v, 20, 10, 19)[2] > 200, "red at the top, blue at the bottom");
        assert_eq!(px(&v, 20, 0, 10), px(&v, 20, 19, 10), "vertical: rows are even");
        let h = draw(&s, &ctrl("CtrlPanel", "H", 20, 20, &[]));
        assert!(px(&h, 20, 0, 10)[0] > 200 && px(&h, 20, 19, 10)[2] > 200);
        // Rounded shapes too, and the attribute on the control itself.
        assert_ne!(draw(&s, &ctrl("CtrlPanel", "R", 20, 20, &[])), draw(&s, &ctrl("CtrlPanel", "R", 20, 20, &[("colFillGradientTo", "255, 0, 0, 255")])));
        // Buttons: the on fill's own gradient.
        let mut b = ctrl("CtrlButton", "On", 20, 20, &[]);
        b.on = true;
        let on = draw(&s, &b);
        assert!(px(&on, 20, 10, 0)[1] > 200 && px(&on, 20, 10, 19)[1] < 40);
    }

    #[test]
    fn bevels_light_the_top_and_shade_the_bottom() {
        let fill = [("colFill", "100, 100, 100, 255"), ("optDrawBorder", "false")];
        let s = sheet(&[("Flat", "CtrlStyle", &fill), ("Bevel", "CtrlStyle", &[&fill[..], &[("optBevel", "true")]].concat())]);
        let flat = draw(&s, &ctrl("CtrlPanel", "Flat", 30, 20, &[]));
        let bev = draw(&s, &ctrl("CtrlPanel", "Bevel", 30, 20, &[]));
        assert_ne!(flat, bev);
        assert!(px(&bev, 30, 15, 0)[0] > 110, "light top edge");
        assert!(px(&bev, 30, 15, 19)[0] < 90, "dark bottom edge");
        assert_eq!(px(&bev, 30, 15, 10), [100, 100, 100], "the middle is the fill");
    }

    #[test]
    fn radial_gradients_shade_outwards() {
        let base = [("colFill", "255, 255, 255, 255"), ("colFillGradientTo", "0, 0, 0, 255"), ("optDrawBorder", "false")];
        let radial = [&base[..], &[("enumGradientDirection", "Radial")]].concat();
        let offset = [&radial[..], &[("realGradientCentreX", "0"), ("realGradientCentreY", "0"), ("realGradientRadius", "1")]].concat();
        let inner = [&radial[..], &[("realGradientInnerRadius", "0.25")]].concat();
        let s = sheet(&[("V", "CtrlStyle", &base), ("R", "CtrlStyle", &radial), ("O", "CtrlStyle", &offset), ("I", "CtrlStyle", &inner)]);
        let r = draw(&s, &ctrl("CtrlPanel", "R", 40, 40, &[]));
        assert_ne!(r, draw(&s, &ctrl("CtrlPanel", "V", 40, 40, &[])));
        // Brightest in the middle, the same distance out alike either way,
        // the end colour at the radius and beyond.
        let mid = px(&r, 40, 20, 20)[0];
        assert!(mid > 240, "centre {mid}");
        assert!(px(&r, 40, 20, 10)[0].abs_diff(px(&r, 40, 10, 20)[0]) <= 2);
        assert!(px(&r, 40, 20, 10)[0] < mid && px(&r, 40, 20, 10)[0] > 60);
        assert!(px(&r, 40, 0, 0)[0] < 5, "corner");
        // A centre at the top left, a radius of the whole rect.
        let o = draw(&s, &ctrl("CtrlPanel", "O", 40, 40, &[]));
        assert!(px(&o, 40, 1, 1)[0] > 240 && px(&o, 40, 38, 1)[0] < 20 && px(&o, 40, 20, 1)[0] > 100);
        // The fill colour holds up to the inner radius.
        let i = draw(&s, &ctrl("CtrlPanel", "I", 40, 40, &[]));
        assert!(px(&i, 40, 20, 15)[0] > 245 && px(&r, 40, 20, 15)[0] < 235);
        // Read from the control first.
        assert_eq!(draw(&s, &ctrl("CtrlPanel", "V", 40, 40, &[("enumGradientDirection", "Radial")])), r);
    }

    #[test]
    fn noise_textures_are_opt_in_subtle_and_reproducible() {
        let base = [("colFill", "128, 128, 128, 255"), ("optDrawBorder", "false")];
        let s = sheet(&[("Flat", "CtrlStyle", &base), ("Noise", "CtrlStyle", &[&base[..], &[("realNoiseAmount", "0.1")]].concat())]);
        let flat = draw(&s, &ctrl("CtrlPanel", "Flat", 60, 20, &[]));
        assert!(flat.chunks(4).all(|p| p[..3] == [128, 128, 128]));
        let n = draw(&s, &ctrl("CtrlPanel", "Noise", 60, 20, &[]));
        assert_eq!(n, draw(&s, &ctrl("CtrlPanel", "Noise", 60, 20, &[])), "deterministic");
        assert!(n.chunks(4).any(|p| p[0] > 128) && n.chunks(4).any(|p| p[0] < 128), "light and dark streaks");
        assert!(n.chunks(4).all(|p| p[0].abs_diff(128) <= 26), "at most the amount");
        // Streaks run along x: neighbours in a row differ less than in a column.
        let d = |dx: usize, dy: usize| -> u32 {
            let mut t = 0;
            for y in 0..19 {
                for x in 0..59 {
                    t += px(&n, 60, x, y)[0].abs_diff(px(&n, 60, x + dx, y + dy)[0]) as u32;
                }
            }
            t
        };
        assert!(d(1, 0) * 2 < d(0, 1), "row {} column {}", d(1, 0), d(0, 1));
        let other = draw(&s, &ctrl("CtrlPanel", "Noise", 60, 20, &[("intNoiseSeed", "2")]));
        assert_ne!(n, other, "the seed picks the pattern");
    }

    #[test]
    fn slider_grip_shapes() {
        let cap = [COLOURS, &[("enumHandleShape", "Rect"), ("realHandleLength", "16"), ("colGrip", "255, 0, 255, 255"), ("intGripLines", "2")]].concat();
        let squares = [&cap[..], &[("enumGripShape", "Squares"), ("realGripSize", "2")]].concat();
        let along = [&cap[..], &[("enumGripShape", "Along")]].concat();
        let s = sheet(&[("Lines", "CtrlStyle", &cap), ("Squares", "CtrlStyle", &squares), ("Along", "CtrlStyle", &along)]);
        let magenta = |px: &[u8], w: usize| -> Vec<(usize, usize)> { px.chunks(4).enumerate().filter(|(_, p)| p[0] > 200 && p[1] < 60 && p[2] > 200).map(|(i, _)| (i % w, i / w)).collect() };
        let mut f = ctrl("CtrlSlider", "Squares", 15, 76, &[]);
        f.value = 0.5;
        let sq = magenta(&draw(&s, &f), 15);
        // Two 2 x 2 squares, a quarter and three quarters along the 16 px cap.
        assert_eq!(sq.len(), 8, "{sq:?}");
        let ys: std::collections::BTreeSet<usize> = sq.iter().map(|p| p.1).collect();
        assert_eq!(ys.len(), 4);
        let (y0, y1) = (*ys.first().unwrap(), *ys.last().unwrap());
        assert!((7..=9).contains(&(y1 - y0)), "{y0}..{y1}");
        // Lines across a vertical cap are wider than tall; along it, taller.
        f.style = "Lines".into();
        let l = magenta(&draw(&s, &f), 15);
        f.style = "Along".into();
        let a = magenta(&draw(&s, &f), 15);
        let span = |v: &[(usize, usize)]| (v.iter().map(|p| p.0).max().unwrap() - v.iter().map(|p| p.0).min().unwrap(), v.iter().map(|p| p.1).max().unwrap() - v.iter().map(|p| p.1).min().unwrap());
        assert!(span(&l).0 > span(&l).1 && span(&a).1 > span(&a).0, "{:?} {:?}", span(&l), span(&a));
    }

    #[test]
    fn text_glow_embolden_and_vertical_offset() {
        let base = [("colText", "255, 255, 255, 255"), ("colFill", "0, 0, 0, 255"), ("optDrawBorder", "false")];
        let s = sheet(&[
            ("Plain", "CtrlStyle", &base),
            ("Glow", "CtrlStyle", &[&base[..], &[("colTextGlow", "0, 0, 255, 255"), ("realTextGlowRadius", "3")]].concat()),
            ("Bold", "CtrlStyle", &[&base[..], &[("realTextEmbolden", "1")]].concat()),
            ("Low", "CtrlStyle", &[&base[..], &[("realTextOffsetY", "6")]].concat()),
        ]);
        let label = |st: &str| {
            let mut c = ctrl("CtrlLabel", st, 80, 30, &[]);
            c.text = "IIII".into();
            draw(&s, &c)
        };
        let plain = label("Plain");
        if painted(&plain) == 0 {
            return; // (no font on this machine)
        }
        let blue = |px: &[u8]| px.chunks(4).filter(|p| p[2] > p[0].saturating_add(40)).count();
        let glow = label("Glow");
        assert_eq!(blue(&plain), 0);
        assert!(blue(&glow) > 20, "a blue halo round the letters");
        assert!(painted(&label("Bold")) > painted(&plain), "{} {}", painted(&label("Bold")), painted(&plain));
        // Lower by 6 px: the first lit row moves down.
        let top = |px: &[u8]| px.chunks(4).position(|p| p[0] > 128).map(|i| i / 80).unwrap();
        assert_eq!(top(&label("Low")), top(&plain) + 6);
    }

    #[test]
    fn knob_pointers_and_faces() {
        let pointer = [COLOURS, &[("optPointer", "true"), ("colPointer", "255, 0, 255, 255"), ("realPointerWidth", "2")]].concat();
        let face = [&pointer[..], &[("colFace", "200, 200, 200, 255"), ("colFaceGradientTo", "90, 90, 90, 255")]].concat();
        let s = sheet(&[("Knob", "CtrlStyle", COLOURS), ("Pointer", "CtrlStyle", &pointer), ("Face", "CtrlStyle", &face)]);
        let magenta = |px: &[u8]| px.chunks(4).filter(|p| p[0] > 200 && p[1] < 60 && p[2] > 200).count();
        let plain = draw(&s, &ctrl("CtrlKnob", "Knob", 40, 40, &[]));
        let (a, b) = check(&s, ctrl("CtrlKnob", "Pointer", 40, 40, &[]), 0.0, 1.0);
        assert!(magenta(&a) > 0 && magenta(&b) > 0 && magenta(&plain) == 0);
        // Value 0.5 points straight up from the centre.
        let mut up = ctrl("CtrlKnob", "Pointer", 40, 40, &[]);
        up.value = 0.5;
        let upx = draw(&s, &up);
        assert_eq!(px(&upx, 40, 20, 12), [255, 0, 255]);
        assert_ne!(px(&upx, 40, 20, 28), [255, 0, 255]);
        // Small knobs get it too (no dot under 33 px).
        check(&s, ctrl("CtrlKnob", "Pointer", 24, 24, &[]), 0.1, 0.9);
        // A face: lighter at the top than at the bottom.
        let f = draw(&s, &up);
        let faced = draw(&s, &{
            let mut k = up.clone();
            k.style = "Face".into();
            k
        });
        assert_ne!(f, faced);
        assert!(px(&faced, 40, 14, 11)[0] > px(&faced, 40, 14, 29)[0]);
    }

    #[test]
    fn dashed_borders_lines_and_dividers() {
        let border = [("colFill", "0, 0, 0, 255"), ("colBorder", "255, 255, 255, 255")];
        let s = sheet(&[
            ("Solid", "CtrlStyle", &border),
            ("Dashed", "CtrlStyle", &[&border[..], &[("intDashLength", "2"), ("intDashGap", "2")]].concat()),
            ("Line", "CtrlStyle", &[("colFill", "255, 255, 255, 255"), ("optDrawBorder", "false"), ("intDashLength", "3"), ("intDashGap", "1")]),
        ]);
        let white = |px: &[u8]| px.chunks(4).filter(|p| p[0] > 128).count();
        let solid = draw(&s, &ctrl("CtrlPanel", "Solid", 40, 20, &[]));
        let dashed = draw(&s, &ctrl("CtrlPanel", "Dashed", 40, 20, &[]));
        assert!(white(&dashed) > 0 && white(&dashed) < white(&solid) * 3 / 4);
        // A 1 px panel is a dashed line: 3 on, 1 off.
        let line = draw(&s, &ctrl("CtrlPanel", "Line", 1, 16, &[]));
        let on: Vec<bool> = (0..16).map(|y| px(&line, 1, 0, y)[0] > 128).collect();
        assert_eq!(on, [true, true, true, false].repeat(4));
        let wide = draw(&s, &ctrl("CtrlPanel", "Line", 16, 1, &[]));
        assert!(px(&wide, 16, 2, 0)[0] > 128 && px(&wide, 16, 3, 0)[0] < 128);
        // Table dividers.
        let t = [COLOURS, &[("colRow", "30, 30, 120, 255"), ("colRowDivider", "255, 255, 255, 255"), ("optRowDividers", "true"), ("intRowHeight", "10"), ("optHeader", "false")]].concat();
        let td = [&t[..], &[("intDividerDashLength", "2"), ("intDividerDashGap", "3")]].concat();
        let s = sheet(&[("T", "CtrlTableViewStyle", &t), ("TD", "CtrlTableViewStyle", &td)]);
        let rows = [("rows", "a\nb")];
        let plain = draw(&s, &ctrl("CtrlTableView", "T", 50, 30, &rows));
        let dash = draw(&s, &ctrl("CtrlTableView", "TD", 50, 30, &rows));
        assert!(px(&plain, 50, 4, 9)[0] > 128 && px(&plain, 50, 6, 9)[0] > 128);
        assert!(px(&dash, 50, 1, 9)[0] > 128 && px(&dash, 50, 3, 9)[0] < 128, "row divider dashed");
    }

    #[test]
    fn check_box_tick_outline_is_opt_in() {
        let base: &[(&str, &str)] =
            &[("colFill", "0, 0, 0, 255"), ("colAltFill", "0, 0, 0, 255"), ("colBorder", "0, 0, 0, 0"), ("colAltBorder", "0, 0, 0, 0"), ("colTick", "200, 200, 200, 255"), ("realTickLineWidth", "3")];
        let s = sheet(&[("Plain", "CheckBoxButtonRendererStyle", base), ("Outlined", "CheckBoxButtonRendererStyle", &[base, &[("colTickOutline", "0, 0, 255, 255")]].concat())]);
        let blue = |px: &[u8]| px.chunks(4).filter(|p| p[2] > 120 && p[0] < 100).count();
        let mut c = ctrl("CtrlButton", "Plain", 16, 16, &[("renderer", "CheckBoxButtonRenderer")]);
        c.on = true;
        assert_eq!(blue(&draw(&s, &c)), 0, "no outline by default");
        c.style = "Outlined".into();
        assert!(blue(&draw(&s, &c)) > 0, "the outline round the tick");
    }

    #[test]
    fn search_box_magnifier_is_opt_out() {
        let clear: &[(&str, &str)] = &[("colFill", "0, 0, 0, 0"), ("colBorder", "0, 0, 0, 0"), ("colText", "200, 200, 0, 255")];
        let s = sheet(&[("Box", "CtrlStyle", clear), ("Bare", "CtrlStyle", &[clear, &[("optSearchIcon", "false")]].concat())]);
        let yellow = |px: &[u8]| px.chunks(4).filter(|p| p[0] > 40).count();
        assert!(yellow(&draw(&s, &ctrl("CtrlSearchBox", "Box", 80, 18, &[]))) > 0, "the magnifier by default");
        assert_eq!(yellow(&draw(&s, &ctrl("CtrlSearchBox", "Bare", 80, 18, &[]))), 0, "none with optSearchIcon false");
    }

    #[test]
    fn arrow_buttons() {
        let st = [("colFill", "255, 255, 255, 255"), ("optDrawBorder", "false"), ("enumShape", "Arrow"), ("realArrowDepth", "6")];
        let s = sheet(&[("Rect", "CtrlStyle", &st[..2]), ("Arrow", "CtrlStyle", &st)]);
        let rect = draw(&s, &ctrl("CtrlButton", "Rect", 40, 18, &[]));
        let arrow = draw(&s, &ctrl("CtrlButton", "Arrow", 40, 18, &[]));
        let first = draw(&s, &ctrl("CtrlButton", "Arrow", 40, 18, &[("optArrowNotch", "false")]));
        assert_eq!(px(&rect, 40, 39, 0), [255, 255, 255]);
        assert_eq!(px(&arrow, 40, 39, 0), [0, 0, 0], "the point cuts the corners");
        assert_eq!(px(&arrow, 40, 37, 9), [255, 255, 255], "the tip");
        assert_eq!(px(&arrow, 40, 1, 9), [0, 0, 0], "the notch");
        assert_eq!(px(&first, 40, 1, 9), [255, 255, 255], "no notch on the first");
        assert_eq!(px(&arrow, 40, 5, 1), [255, 255, 255]);
    }

    #[test]
    fn button_glyphs_are_opt_in() {
        let base = [("colFill", "0, 0, 255, 255"), ("colAltFill", "0, 0, 255, 255"), ("optDrawBorder", "false"), ("colText", "0, 255, 0, 255")];
        let bar = [&base[..], &[("enumGlyph", "Bar"), ("realGlyphWidth", "4"), ("realGlyphHeight", "10"), ("colGlyph", "255, 255, 255, 255"), ("colAltGlyph", "255, 0, 0, 255")]].concat();
        let tri = [&base[..], &[("enumGlyph", "TriangleLeft"), ("colGlyphBorder", "255, 0, 0, 255")]].concat();
        let s = sheet(&[("Plain", "CtrlStyle", &base), ("Bar", "CtrlStyle", &bar), ("Tri", "CtrlStyle", &tri)]);
        let plain = draw(&s, &ctrl("CtrlButton", "Plain", 16, 16, &[]));
        assert_eq!(px(&plain, 16, 8, 8), [0, 0, 255], "no glyph without enumGlyph");
        let off = draw(&s, &ctrl("CtrlButton", "Bar", 16, 16, &[]));
        assert_eq!(px(&off, 16, 7, 8), [255, 255, 255], "the bar in colGlyph");
        assert_eq!(px(&off, 16, 3, 8), [0, 0, 255], "only 4 px wide");
        assert_eq!(px(&off, 16, 7, 1), [0, 0, 255], "only 10 px high");
        let mut c = ctrl("CtrlButton", "Bar", 16, 16, &[]);
        c.on = true;
        assert_eq!(px(&draw(&s, &c), 16, 7, 8), [255, 0, 0], "colAltGlyph when on");
        let t = draw(&s, &ctrl("CtrlButton", "Tri", 16, 16, &[]));
        assert_eq!(px(&t, 16, 12, 8), [0, 255, 0], "filled in colText by default");
        assert_eq!(px(&t, 16, 2, 2), [0, 0, 255], "pointing left: the top left stays clear");
        assert_eq!(px(&t, 16, 15, 1)[0], 255, "outlined in colGlyphBorder");
    }

    #[test]
    fn button_strips_follow_the_on_state() {
        let st = [
            ("colFill", "40, 40, 40, 255"),
            ("colAltFill", "40, 40, 40, 255"),
            ("optDrawBorder", "false"),
            ("realStripHeight", "3"),
            ("realStripInset", "2"),
            ("colStripOff", "0, 0, 255, 255"),
            ("colStripOn", "255, 200, 0, 255"),
        ];
        let s = sheet(&[("Key", "CtrlStyle", &st), ("Plain", "CtrlStyle", &st[..3])]);
        let mut k = ctrl("CtrlButton", "Key", 20, 30, &[]);
        let off = draw(&s, &k);
        k.on = true;
        let on = draw(&s, &k);
        assert_eq!(px(&off, 20, 10, 26), [0, 0, 255]);
        assert_eq!(px(&on, 20, 10, 26), [255, 200, 0]);
        assert_eq!(px(&on, 20, 10, 28), [40, 40, 40], "inset from the bottom");
        assert_eq!(px(&on, 20, 10, 20), [40, 40, 40]);
        assert_eq!(px(&draw(&s, &ctrl("CtrlButton", "Plain", 20, 30, &[])), 20, 10, 26), [40, 40, 40]);
        // Opt-in side inset: narrower than the bottom inset says.
        assert_eq!(px(&off, 20, 3, 26), [0, 0, 255]);
        let side = draw(&s, &ctrl("CtrlButton", "Key", 20, 30, &[("realStripSideInset", "5"), ("realStripRounding", "0")]));
        assert_eq!(px(&side, 20, 3, 26), [40, 40, 40], "inset from the sides");
        assert_eq!(px(&side, 20, 5, 25), [0, 0, 255]);
    }

    #[test]
    fn icon_panels_follow_their_buttons_on_state() {
        let btn = [("colFill", "255, 255, 255, 255"), ("colAltFill", "0, 0, 0, 255"), ("optDrawBorder", "false")];
        let mark = [("colFill", "0, 0, 255, 255"), ("colParentOnFill", "255, 0, 0, 255"), ("optDrawBorder", "false")];
        let s = sheet(&[("Btn", "CtrlStyle", &btn), ("Mark", "CtrlStyle", &mark), ("Plain", "CtrlStyle", &mark[..1])]);
        let mut b = ctrl("CtrlButton", "Btn", 20, 20, &[]);
        let mut m = ctrl("CtrlPanel", "Mark", 4, 4, &[]);
        m.rect = crate::ctrl::Rect::new(8, 8, 4, 4);
        let mut plain = ctrl("CtrlPanel", "Plain", 4, 4, &[]);
        plain.rect = crate::ctrl::Rect::new(0, 0, 4, 4);
        b.children = vec![m, plain];
        assert_eq!(px(&draw(&s, &b), 20, 9, 9), [0, 0, 255]);
        b.on = true;
        let on = draw(&s, &b);
        assert_eq!(px(&on, 20, 9, 9), [255, 0, 0], "the on colour inside an on button");
        assert_eq!(px(&on, 20, 1, 1), [0, 0, 255], "panels without it unchanged");
    }

    #[test]
    fn scroll_bars_can_frame_the_track_and_show_a_full_thumb() {
        let st = [("colFill", "255, 255, 255, 255"), ("colAltFill", "0, 0, 255, 255")];
        let opt = [st[0], st[1], ("colTrackBorder", "255, 0, 0, 255"), ("realThumbInset", "2"), ("optThumbWhenFull", "true")];
        let s = sheet(&[("Bar", "CtrlStyle", &st), ("Framed", "CtrlStyle", &opt)]);
        let plain = draw(&s, &ctrl("CtrlScrollBar", "Bar", 10, 60, &[("realPage", "1")]));
        assert_eq!(px(&plain, 10, 5, 30), [255, 255, 255], "no thumb when it all fits");
        assert_eq!(px(&plain, 10, 0, 30), [255, 255, 255], "no frame");
        let framed = draw(&s, &ctrl("CtrlScrollBar", "Framed", 10, 60, &[("realPage", "1")]));
        assert_eq!(px(&framed, 10, 0, 30), [255, 0, 0], "the frame");
        assert_eq!(px(&framed, 10, 1, 30), [255, 255, 255], "the inset");
        assert_eq!(px(&framed, 10, 5, 30), [0, 0, 255], "a full-length thumb");
        assert_eq!(px(&framed, 10, 5, 4), [0, 0, 255]);
        assert_eq!(px(&framed, 10, 5, 56), [0, 0, 255]);
        // A half page sits at the end for value 1.
        let half = draw(&s, &ctrl("CtrlScrollBar", "Framed", 60, 10, &[("realPage", "0.5")]));
        let mut half_end = ctrl("CtrlScrollBar", "Framed", 60, 10, &[("realPage", "0.5")]);
        half_end.value = 1.0;
        let end = draw(&s, &half_end);
        assert_eq!(px(&half, 60, 10, 5), [0, 0, 255]);
        assert_eq!(px(&half, 60, 50, 5), [255, 255, 255]);
        assert_eq!(px(&end, 60, 50, 5), [0, 0, 255]);
    }

    #[test]
    fn range_sliders_light_between_their_handles() {
        let s = sheet(&[("Range", "CtrlStyle", &[COLOURS, &[("enumHandleShape", "Rect"), ("realHandleLength", "8")]].concat())]);
        let red = |px: &[u8]| px.chunks(4).filter(|p| p[..3] == [255, 0, 0]).count();
        let narrow = draw(&s, &ctrl("CtrlRangeSlider", "Range", 107, 19, &[("valueLow", "0.4"), ("valueHigh", "0.6")]));
        let wide = draw(&s, &ctrl("CtrlRangeSlider", "Range", 107, 19, &[("valueLow", "0.1"), ("valueHigh", "0.9")]));
        assert!(red(&wide) > red(&narrow) && red(&narrow) > 0);
        assert_eq!(px(&narrow, 107, 53, 9), [255, 0, 0], "lit in the middle");
        assert_ne!(px(&narrow, 107, 20, 9), [255, 0, 0], "not outside the range");
        // Swapped values draw the same; vertical ones work too.
        assert_eq!(narrow, draw(&s, &ctrl("CtrlRangeSlider", "Range", 107, 19, &[("valueLow", "0.6"), ("valueHigh", "0.4")])));
        let v = draw(&s, &ctrl("CtrlRangeSlider", "Range", 19, 107, &[("valueLow", "0.0"), ("valueHigh", "0.5")]));
        assert_eq!(px(&v, 19, 9, 80), [255, 0, 0], "lower half lit");
        assert_ne!(px(&v, 19, 9, 20), [255, 0, 0]);
    }

    #[test]
    fn range_slider_triangle_markers_are_opt_in() {
        let st = [COLOURS, &[("enumHandleShape", "Triangles"), ("realHandleLength", "11")]].concat();
        let s = sheet(&[("Tri", "CtrlStyle", &st)]);
        let d = draw(&s, &ctrl("CtrlRangeSlider", "Tri", 107, 19, &[("valueLow", "0.0"), ("valueHigh", "1.0")]));
        // (the high marker at the right, from the top; the low one at the
        // left, from the bottom; both colAltFill)
        assert_eq!(px(&d, 107, 101, 2), [0, 255, 0], "▼ at the top right");
        assert_eq!(px(&d, 107, 5, 16), [0, 255, 0], "▲ at the bottom left");
        assert_ne!(px(&d, 107, 5, 2), [0, 255, 0], "no marker at the top left");
        assert_eq!(px(&d, 107, 53, 9), [255, 0, 0], "lit between");
        assert_eq!(px(&d, 107, 53, 1), [40, 40, 40], "the track fills the rect");
        // Vertical ones keep the stock handles.
        let v = draw(&s, &ctrl("CtrlRangeSlider", "Tri", 19, 107, &[("valueLow", "0.0"), ("valueHigh", "0.5")]));
        assert_eq!(px(&v, 19, 9, 80), [255, 0, 0]);
    }

    #[test]
    fn text_spacing_scale_and_slant() {
        let base = [("colFill", "0, 0, 0, 255"), ("optDrawBorder", "false"), ("colText", "255, 255, 255, 255"), ("enumTextHorizontalAlign", "left")];
        let s = sheet(&[("T", "CtrlStyle", &base)]);
        let mut l = ctrl("CtrlLabel", "T", 120, 20, &[]);
        l.text = "MIXER".into();
        let plain = draw(&s, &l);
        if painted(&plain) == 0 {
            return; // no system font to draw with
        }
        let extent = |img: &[u8]| (0..120).filter(|x| (0..20).any(|y| px(img, 120, *x, y)[0] > 64)).max().unwrap_or(0);
        let with = |k: &str, v: &str| {
            let mut c = l.clone();
            c.set_attr(k, v.into());
            draw(&s, &c)
        };
        let spaced = with("realLetterSpacing", "3");
        let narrow = with("realTextScaleX", "0.7");
        let slanted = with("realTextSlant", "12");
        assert!(extent(&spaced) >= extent(&plain) + 10, "4 gaps of 3 px");
        assert!(extent(&narrow) + 5 < extent(&plain), "condensed");
        assert_ne!(slanted, plain);
        assert!(painted(&slanted) > 0 && painted(&narrow) > 0);
        // The neutral values draw exactly as plain text.
        assert_eq!(with("realTextScaleX", "1"), plain);
        assert_eq!(with("realLetterSpacing", "0"), plain);
    }

    #[test]
    fn slider_grooves_frames_caps_and_grips() {
        let base = [COLOURS, &[("enumHandleShape", "Rect"), ("realHandleLength", "15"), ("colFillHighlight", "0, 0, 0, 0")]].concat();
        let deco = [
            &base[..],
            &[
                ("colGroove", "255, 0, 255, 255"),
                ("colTrackBorder", "0, 0, 255, 255"),
                ("colGrip", "255, 255, 0, 255"),
                ("intGripLines", "2"),
                ("realHandleInset", "3"),
                ("colAltFillGradientTo", "0, 0, 0, 255"),
            ],
        ]
        .concat();
        let s = sheet(&[("Plain", "CtrlStyle", &base), ("Deco", "CtrlStyle", &deco)]);
        let mut c = ctrl("CtrlSlider", "Plain", 15, 75, &[("realTrackInset", "0")]);
        c.value = 1.0;
        let plain = draw(&s, &c);
        c.style = "Deco".into();
        let deco = draw(&s, &c);
        assert_ne!(plain, deco);
        assert_eq!(px(&deco, 15, 7, 50), [255, 0, 255], "groove in the middle");
        assert_eq!(px(&deco, 15, 0, 50), [0, 0, 255], "track frame");
        assert_eq!(px(&deco, 15, 1, 7), [40, 40, 40], "the cap is inset across");
        let yellow = |px: &[u8]| px.chunks(4).filter(|p| p[..3] == [255, 255, 0]).count();
        assert!(yellow(&deco) >= 2 * 4, "two grip lines");
        // The cap's gradient: lighter at its top than at its bottom.
        assert!(px(&deco, 15, 4, 2)[1] > px(&deco, 15, 4, 12)[1]);
        // Pan and range sliders take the groove too.
        for class in ["CtrlPanSlider", "CtrlRangeSlider"] {
            let a = draw(&s, &ctrl(class, "Plain", 75, 12, &[]));
            let b = draw(&s, &ctrl(class, "Deco", 75, 12, &[]));
            assert_ne!(a, b, "{class}");
        }
    }

    #[test]
    fn gradient_shades_follow_the_fill() {
        let s = sheet(&[
            ("Shade", "CtrlStyle", &[("colFill", "200, 200, 200, 255"), ("optDrawBorder", "false"), ("realGradientShade", "1")]),
            ("Light", "CtrlStyle", &[("colFill", "100, 100, 100, 255"), ("optDrawBorder", "false"), ("realGradientShade", "-1")]),
        ]);
        let d = draw(&s, &ctrl("CtrlPanel", "Shade", 10, 20, &[]));
        assert!(px(&d, 10, 5, 0)[0] > 190 && px(&d, 10, 5, 19)[0] < 110, "darkened to half");
        let l = draw(&s, &ctrl("CtrlPanel", "Light", 10, 20, &[]));
        assert!(px(&l, 10, 5, 19)[0] > 160);
    }

    #[test]
    fn popup_arrows_and_dividers() {
        let st = [("colFill", "0, 0, 0, 255"), ("colBorder", "255, 255, 255, 255"), ("colText", "0, 255, 0, 255")];
        let s = sheet(&[("P", "CtrlStyle", &st), ("Q", "CtrlStyle", &[&st[..], &[("optPopupDivider", "false"), ("realPopupArrowSize", "0.2")]].concat())]);
        let a = draw(&s, &ctrl("CtrlPopupButton", "P", 60, 20, &[]));
        let b = draw(&s, &ctrl("CtrlPopupButton", "Q", 60, 20, &[]));
        assert_eq!(px(&a, 60, 40, 10), [255, 255, 255], "divider");
        assert_eq!(px(&b, 60, 40, 10), [0, 0, 0], "no divider");
        let green = |px: &[u8]| px.chunks(4).filter(|p| p[1] > 128 && p[0] < 64).count();
        assert!(green(&b) > 0 && green(&b) < green(&a), "smaller arrow");
        // `realPopupArrowBoxWidth`: the arrow sits in a narrower box at the
        // right edge.
        let s = sheet(&[("N", "CtrlStyle", &[&st[..], &[("optPopupDivider", "false"), ("realPopupArrowBoxWidth", "10")]].concat())]);
        let c = draw(&s, &ctrl("CtrlPopupButton", "N", 60, 20, &[]));
        let first_green = |img: &[u8]| {
            (0..60).find(|&x| {
                (0..20).any(|y| {
                    let p = px(img, 60, x, y);
                    p[1] > 128 && p[0] < 64
                })
            })
        };
        assert!(first_green(&c).unwrap() > first_green(&b).unwrap(), "arrow moved right");
    }

    #[test]
    fn knob_face_rings_and_pointer_dots() {
        let k = [
            COLOURS,
            &[
                ("optPointer", "true"),
                ("colPointer", "255, 0, 255, 255"),
                ("colFace", "200, 200, 200, 255"),
                ("realFaceInset", "-8"),
                ("colFaceBorder", "0, 0, 255, 255"),
                ("realFaceBorderWidth", "4"),
            ],
        ]
        .concat();
        let kd = [&k[..], &[("optPointerDot", "true")]].concat();
        let s = sheet(&[("K", "CtrlStyle", &k), ("KD", "CtrlStyle", &kd)]);
        let mut c = ctrl("CtrlKnob", "K", 40, 40, &[]);
        c.value = 0.5;
        let a = draw(&s, &c);
        assert_eq!(px(&a, 40, 20, 1), [0, 0, 255], "the face's border covers the ring");
        c.style = "KD".into();
        let b = draw(&s, &c);
        assert_ne!(a, b, "the dot is drawn too");
    }
}
