//! Drawing of the stock [re:4a79c9] controls, following the decompiled `Draw` /
//! `DrawLayer` methods. Custom the original panels (`overloadClass`) get their
//! skin children drawn here and their own content from the editor layer.

use crate::colour::{self, Colour};
use crate::ctrl::Ctrl;
use crate::paint::{Painter, RectF};
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
}

fn std_col(name: &str) -> Colour {
    colour::standard(name)
}

impl<'s> Renderer<'s> {
    pub fn new(styles: &'s StyleSheet) -> Renderer<'s> {
        Renderer { styles, custom: None, custom_drawn: None, in_layer: std::cell::Cell::new(0) }
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
    /// its children (`Ctrl::DrawChildren` [re:0cca49]: visible children in
    /// order, each translated and clipped to its rect).
    pub fn draw(&self, p: &mut Painter, c: &Ctrl) {
        if !c.visible {
            return;
        }
        // `Ctrl::SetAlpha` (vfunc 0x1b8): the control's own alpha, on top
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
            if c.children.windows(2).all(|w| w[0].zorder <= w[1].zorder) {
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
    /// [re:4a9597] / `DrawLogo` [re:a75af4] draw a 3 px frame inset 1.5 and the
    /// product mark in colFill, then again in colAltFill at the master
    /// level's alpha, `overlayAlpha` = min(level·0.75, 1)). The port's mark
    /// is a "D": a stem and a bowl.
    fn logo_meter(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let draw = |p: &mut Painter, col: Colour| {
            p.stroke_rect(RectF::new(1.5, 1.5, w - 3.0, h - 3.0), col, 3.0);
            p.fill_polygon(&[(9.0, 9.0), (16.0, 9.0), (16.0, 41.0), (9.0, 41.0)], col);
            p.fill_polygon(
                &[
                    (18.0, 9.0),
                    (28.0, 9.0),
                    (36.0, 13.0),
                    (41.0, 20.0),
                    (41.0, 30.0),
                    (36.0, 37.0),
                    (28.0, 41.0),
                    (18.0, 41.0),
                    (18.0, 34.0),
                    (27.0, 34.0),
                    (32.0, 31.0),
                    (34.0, 27.0),
                    (34.0, 23.0),
                    (32.0, 19.0),
                    (27.0, 16.0),
                    (18.0, 16.0),
                ],
                col,
            );
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
        if let Some(a) = c.attr("overlayAlpha").and_then(|v| v.parse::<f32>().ok()).filter(|a| *a > 0.0) {
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
                let page = c.attr_f32("realPage", 1.0).clamp(0.0, 1.0);
                if page < 1.0 {
                    let v = c.value.clamp(0.0, 1.0);
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
            "root" => {}
            _ => self.default_fill(p, c, "colFill", "colBorder", false),
        }
    }

    /// [re:33ddf4] (ev_db5615b5… `DrawBackground` [re:7fa1f2] / `DrawBar`
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

    /// `Ctrl::DrawDefaultFill` [re:1efccd] with colour attributes.
    pub fn default_fill(&self, p: &mut Painter, c: &Ctrl, fill: &str, border: &str, shadow: bool) {
        let st = self.style(c);
        let f = self.colour_attr(st, fill);
        let b = self.colour_attr(st, border);
        self.default_fill_colours(p, c, f, b, shadow);
    }

    pub fn default_fill_colours(&self, p: &mut Painter, c: &Ctrl, fill: Colour, border: Colour, _shadow: bool) {
        let st = self.style(c);
        let s = self.styles;
        let bw = s.float(st, "realBorderWidth", 1.0);
        let draw_border = s.bool(st, "optDrawBorder", true) && bw.abs() >= 1e-6;
        let corners = s.rounding_options(st);
        let radius = s.float(st, "realCornerRounding", 0.0).max(0.0);
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let full = RectF::new(0.0, 0.0, w, h);
        let border_rect = |bw: f32| {
            let r = full.inset(bw * 0.5);
            if (bw as i32) & 1 != 0 { r.pixel_align() } else { r }
        };
        let none = corners.iter().all(|x| !x);
        let all = corners.iter().all(|x| *x);
        if radius < 1e-6 || none {
            p.fill_rect(full, fill);
            if draw_border {
                p.stroke_rect(border_rect(bw), border, bw);
            }
        } else if all {
            let (wi, hi) = ((w as i32) as f32, (h as i32) as f32);
            if (wi - hi).abs() > 1e-6 || (radius - wi * 0.5).abs() > 1e-6 {
                p.fill_rounded(RectF::new(0.0, 0.0, wi, hi), radius, corners, fill);
                if draw_border {
                    let r = RectF::new(bw * 0.5, bw * 0.5, wi - bw, hi - bw);
                    p.stroke_rounded(if (bw as i32) & 1 != 0 { r.pixel_align() } else { r }, radius, corners, border, bw);
                }
            } else {
                p.fill_ellipse(full, fill);
                if draw_border {
                    p.stroke_ellipse(border_rect(bw), border, bw);
                }
            }
        } else {
            p.fill_rounded(full, radius, corners, fill);
            // Quirk: with partial rounding a border wider than 1 is never
            // stroked (the inset path is built and discarded).
            if draw_border && bw <= 1.0 {
                p.stroke_rounded(full, radius, corners, border, bw);
            }
        }
    }

    /// [re:485453].
    fn text_rect(&self, c: &Ctrl, r: RectF) -> RectF {
        let st = self.style(c);
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
        let mut colour = self.styles.colour(st, colour_attr, std_col("StandardControlText"));
        // Runtime fade (`SetColourAttribute` with an alpha copy).
        if let Some(a) = c.attr("textAlpha").and_then(|v| v.parse::<f32>().ok()) {
            colour = colour.with_alpha(colour.f_alpha() * a.clamp(0.0, 1.0));
        }
        let tr = self.text_rect(c, r);
        let s = p.fonts.shorten(&font, text, tr.w, self.styles.shorten(st));
        p.text(tr, &s, colour, &font, self.styles.h_align(st), self.styles.v_align(st));
    }

    fn bounds(c: &Ctrl) -> RectF {
        RectF::new(0.0, 0.0, c.rect.w as f32, c.rect.h as f32)
    }

    /// [re:b7a6d1] + `DrawText` [re:26c463].
    fn button(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let mut fill = self.colour_attr(st, if c.on { "colAltFill" } else { "colFill" });
        let mut border = self.colour_attr(st, if c.on { "colAltBorder" } else { "colBorder" });
        if c.hover {
            fill = fill.lighten(0.7);
            if fill.a < 255 {
                fill = fill.with_alpha(fill.f_alpha() + fill.f_alpha() * 0.2);
            }
            border = border.darken(0.4);
        }
        self.default_fill_colours(p, c, fill, border, true);
        self.draw_text(p, c, Self::bounds(c), &c.text, if c.on { "colAltText" } else { "colText" });
    }

    /// [re:29f888]: text box with the text between the
    /// h×h search icon (`DrawSearchIcon` [re:a7d6df]: 2 px magnifier and a menu
    /// triangle in colText) and the clear icon (`DrawClearIcon` [re:369e45],
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
        let tr = RectF::new(h, 0.0, (b.w - h - 2.0 - h).max(0.0), h);
        if editing {
            let font = self.styles.font(st);
            p.text(tr, &format!("{}|", c.text), Colour::BLACK, &font, crate::style::HAlign::Left, crate::style::VAlign::Center);
        } else {
            let text = if c.text.is_empty() { c.attr("placeholder").unwrap_or("").to_string() } else { c.text.clone() };
            self.draw_text(p, c, tr, &text, "colText");
        }
        // Magnifier.
        let d = ((h - 4.0) * 0.5).max(1.0);
        let (x, y) = (4.0f32.floor() + 0.5, 4.0f32.floor() + 0.5);
        p.stroke_ellipse(RectF::new(x, y, d, d), text_col, 2.0);
        let a = x + d * 0.5 + d * 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        let e = x + d * 0.5 + d * 1.5 * std::f32::consts::FRAC_1_SQRT_2;
        p.line(a, a, e, e, text_col, 2.0);
        let q = (h - 4.0) * 0.25;
        let ty = 2.0 + (h - 4.0) * 0.5 + q;
        p.fill_polygon(&[(x, ty), (x + q, ty), (x + q * 0.5, ty + q)], text_col);
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
        let arrow_x = b.w - b.h;
        let tr = RectF::new(b.x + 2.0, b.y, arrow_x - b.x - 2.0, b.h);
        self.draw_text(p, c, tr, &c.text, "colText");
        let text = self.styles.colour(st, "colText", std_col("StandardControlText"));
        let border = self.styles.colour(st, "colBorder", std_col("StandardControlBorder"));
        let aw = b.h * 0.4;
        let ax = arrow_x + b.h * 0.5 - aw * 0.5;
        let ay = b.y + b.h * 0.5 - aw * 0.5;
        p.fill_polygon(&[(ax, ay), (ax + aw, ay), (ax + aw * 0.5, ay + aw)], text);
        let lx = arrow_x.floor() + 0.5;
        p.line(lx, b.y, lx, b.y + b.h, border, 1.0);
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
            p.polyline(&[(w * 0.125 + x, h * 0.6 + y), (lw * 0.5 + w * 0.3 + x, lw * 0.5 + h * 0.8 + y), (w * 0.9 + x, h * 0.1 + y)], tick, lw);
        }
    }

    /// [re:7d8258].
    fn knob(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let s = self.styles;
        let b = Self::bounds(c);
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
        if c.rect.w > 32 && c.rect.h > 32 {
            let (cx, cy) = (b.w * 0.5, b.h * 0.5);
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

    /// [re:0c57f8] + `DrawHandle` [re:a65320].
    fn slider(&self, p: &mut Painter, c: &Ctrl) {
        let st = self.style(c);
        let s = self.styles;
        let (w, h) = (c.rect.w, c.rect.h);
        let inset = c.attr_f32("realTrackInset", 0.3);
        let back = s.colour(st, "colFill", std_col("StandardControlBackground"));
        let hl = s.colour(st, "colFillHighlight", back.darken(0.7));
        let radius = s.float(st, "realCornerRounding", 0.0).round();
        let handle_fill = s.colour(st, "colAltFill", Colour::BLACK);
        let handle_ring = s.colour(st, "colAltBorder", Colour::WHITE);
        let v = c.value.clamp(0.0, 1.0);
        if w < h {
            let i = (w as f32 * inset).round();
            let track = RectF::new(i, 0.0, w as f32 - 2.0 * i, h as f32);
            let hs = w as f32;
            let hy = (1.0 - v) * (h as f32 - hs);
            p.fill_rounded(track, radius, [true; 4], back);
            p.fill_rounded(RectF::new(i, hy + hs / 2.0, w as f32 - 2.0 * i, (h as f32 - hy - hs / 2.0).max(0.0)), radius, [true; 4], hl);
            let hr = RectF::new(0.0, hy, hs, hs);
            p.fill_ellipse(hr, handle_ring);
            p.fill_ellipse(hr.inset((hs * 0.125).ceil()), handle_fill);
        } else {
            let i = (h as f32 * inset).round();
            let track = RectF::new(0.0, i, w as f32, h as f32 - 2.0 * i);
            let hs = h as f32;
            let hx = v * (w as f32 - hs);
            p.fill_rounded(track, radius, [true; 4], back);
            p.fill_rounded(RectF::new(0.0, i, hx + hs / 2.0, h as f32 - 2.0 * i), radius, [true; 4], hl);
            let hr = RectF::new(hx, 0.0, hs, hs);
            p.fill_ellipse(hr, handle_ring);
            p.fill_ellipse(hr.inset((hs * 0.125).ceil()), handle_fill);
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
        // Header strip (`SetRect`: width × intIconicDim, or the iconic
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
