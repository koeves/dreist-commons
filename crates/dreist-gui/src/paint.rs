//! [re:030164] primitives on a tiny-skia pixmap: rectangles,
//! (partially) rounded rectangles, ellipses, arcs, lines and text.
//!
//! Angles follow the original: degrees, 0 = +x, increasing clockwise on
//! screen (y down).

use tiny_skia::{FillRule, GradientStop, LinearGradient, Mask, Paint, PathBuilder, Pixmap, Point, RadialGradient, SpreadMode, Stroke, StrokeDash, Transform};

use crate::colour::Colour;
use crate::style::{FontSpec, HAlign, VAlign};
use crate::text::Fonts;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectF {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> RectF {
        RectF { x, y, w, h }
    }

    pub fn inset(self, d: f32) -> RectF {
        RectF { x: self.x + d, y: self.y + d, w: self.w - 2.0 * d, h: self.h - 2.0 * d }
    }

    /// [re:7cb0a3]: snap to half pixels for odd line widths.
    pub fn pixel_align(self) -> RectF {
        RectF { x: self.x.floor() + 0.5, y: self.y.floor() + 0.5, w: self.w.round(), h: self.h.round() }
    }
}

fn paint(c: Colour) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.r, c.g, c.b, c.a);
    p.anti_alias = true;
    p
}

/// A linear gradient paint from `p0` to `p1` through `stops` (offset
/// 0..1, colour); a solid paint when the gradient degenerates.
fn gradient_paint(p0: (f32, f32), p1: (f32, f32), stops: &[(f32, Colour)]) -> Paint<'static> {
    let col = |c: Colour| tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a);
    let first = stops.first().map(|s| s.1).unwrap_or(Colour::TRANSPARENT);
    let stops: Vec<GradientStop> = stops.iter().map(|(o, c)| GradientStop::new(*o, col(*c))).collect();
    let mut p = paint(first);
    if let Some(shader) = LinearGradient::new(Point::from_xy(p0.0, p0.1), Point::from_xy(p1.0, p1.1), stops, SpreadMode::Pad, Transform::identity()) {
        p.shader = shader;
    }
    p
}

/// A fill gradient: from the fill colour to `to`, top to bottom (or left
/// to right when `horizontal`) across the shape's bounds, or outwards from
/// a centre when `radial` is set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gradient {
    pub to: Colour,
    pub horizontal: bool,
    pub radial: Option<Radial>,
}

/// A radial gradient's centre (`cx`, `cy`, fractions of the shape's bounds
/// from its top left) and radii (`r`, `r0`: fractions of the bounds' width
/// across and of their height along, so it is elliptical in non-square
/// shapes): the fill colour up to `r0`, blending to the end colour at `r`
/// and beyond.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radial {
    pub cx: f32,
    pub cy: f32,
    pub r: f32,
    pub r0: f32,
}

/// A radial gradient paint centred at `(cx, cy)` with radii `rx` / `ry`.
fn radial_paint(cx: f32, cy: f32, rx: f32, ry: f32, stops: &[(f32, Colour)]) -> Paint<'static> {
    let col = |c: Colour| tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a);
    let first = stops.first().map(|s| s.1).unwrap_or(Colour::TRANSPARENT);
    let stops: Vec<GradientStop> = stops.iter().map(|(o, c)| GradientStop::new(*o, col(*c))).collect();
    let mut p = paint(first);
    if rx > 0.0 && ry > 0.0 {
        let t = Transform::from_row(rx, 0.0, 0.0, ry, cx, cy);
        if let Some(shader) = RadialGradient::new(Point::from_xy(0.0, 0.0), Point::from_xy(0.0, 0.0), 1.0, stops, SpreadMode::Pad, t) {
            p.shader = shader;
        }
    }
    p
}

/// A small deterministic hash (seeded noise for textures).
fn hash3(a: u32, b: u32, c: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ c.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// The noise value (-1..1) of `hash3(seed, a, b)`.
fn unit(seed: u32, a: u32, b: u32) -> f32 {
    (hash3(seed, a, b) & 0xFFFF) as f32 / 32767.5 - 1.0
}

/// Box-blur premultiplied RGBA `data` (`w` x `h`) by `r` px, three passes
/// each way (close to a gaussian).
fn box_blur(data: &mut [u8], w: usize, h: usize, r: usize) {
    if r == 0 || w == 0 || h == 0 {
        return;
    }
    let mut line = vec![0u32; w.max(h) * 4];
    let n = (2 * r + 1) as u32;
    for _ in 0..3 {
        for pass in 0..2 {
            let (len, count) = if pass == 0 { (w, h) } else { (h, w) };
            for j in 0..count {
                let idx = |i: usize| if pass == 0 { (j * w + i) * 4 } else { (i * w + j) * 4 };
                for i in 0..len {
                    let k = idx(i);
                    for ch in 0..4 {
                        line[i * 4 + ch] = data[k + ch] as u32;
                    }
                }
                let mut sum = [0u32; 4];
                // Window [i - r, i + r], zero outside.
                for i in 0..r.min(len) {
                    for ch in 0..4 {
                        sum[ch] += line[i * 4 + ch];
                    }
                }
                for i in 0..len {
                    if i + r < len {
                        for ch in 0..4 {
                            sum[ch] += line[(i + r) * 4 + ch];
                        }
                    }
                    let k = idx(i);
                    for ch in 0..4 {
                        data[k + ch] = (sum[ch] / n) as u8;
                    }
                    if i >= r {
                        for ch in 0..4 {
                            sum[ch] -= line[(i - r) * 4 + ch];
                        }
                    }
                }
            }
        }
    }
}

/// Text drawing options: extra space between letters (logical px), a
/// horizontal scale (below 1 condenses) and a slant (degrees, positive
/// leans right). The default draws plain text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextFx {
    pub spacing: f32,
    pub scale_x: f32,
    pub slant: f32,
}

impl Default for TextFx {
    fn default() -> TextFx {
        TextFx { spacing: 0.0, scale_x: 1.0, slant: 0.0 }
    }
}

impl TextFx {
    /// Only letter spacing (or nothing): drawn without a transform.
    fn upright(&self) -> bool {
        (self.scale_x - 1.0).abs() < 1e-4 && self.slant.abs() < 1e-4
    }
}

/// Cubic Bézier approximation of a circular arc (centre, radii, degrees).
fn arc_to(pb: &mut PathBuilder, cx: f32, cy: f32, rx: f32, ry: f32, start: f32, sweep: f32, move_first: bool) {
    let segs = (sweep.abs() / 90.0).ceil().max(1.0) as usize;
    let step = sweep.to_radians() / segs as f32;
    let mut a0 = start.to_radians();
    let pt = |a: f32| (cx + rx * a.cos(), cy + ry * a.sin());
    let (x0, y0) = pt(a0);
    if move_first {
        pb.move_to(x0, y0);
    } else {
        pb.line_to(x0, y0);
    }
    for _ in 0..segs {
        let a1 = a0 + step;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        let (x1, y1) = pt(a0);
        let (x2, y2) = pt(a1);
        pb.cubic_to(x1 - k * rx * a0.sin(), y1 + k * ry * a0.cos(), x2 + k * rx * a1.sin(), y2 - k * ry * a1.cos(), x2, y2);
        a0 = a1;
    }
}

/// [re:33681b] with per-corner flags (TL, TR, BL, BR).
pub fn rounded_rect_path(r: RectF, radius: f32, corners: [bool; 4]) -> Option<tiny_skia::Path> {
    let rad = radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    let mut pb = PathBuilder::new();
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    let c = |i: usize| if corners[i] { rad } else { 0.0 };
    pb.move_to(x0 + c(0), y0);
    pb.line_to(x1 - c(1), y0);
    if c(1) > 0.0 {
        arc_to(&mut pb, x1 - c(1), y0 + c(1), c(1), c(1), 270.0, 90.0, false);
    }
    pb.line_to(x1, y1 - c(3));
    if c(3) > 0.0 {
        arc_to(&mut pb, x1 - c(3), y1 - c(3), c(3), c(3), 0.0, 90.0, false);
    }
    pb.line_to(x0 + c(2), y1);
    if c(2) > 0.0 {
        arc_to(&mut pb, x0 + c(2), y1 - c(2), c(2), c(2), 90.0, 90.0, false);
    }
    pb.line_to(x0, y0 + c(0));
    if c(0) > 0.0 {
        arc_to(&mut pb, x0 + c(0), y0 + c(0), c(0), c(0), 180.0, 90.0, false);
    }
    pb.close();
    pb.finish()
}

pub struct Painter<'a> {
    pub pm: &'a mut Pixmap,
    pub fonts: &'a mut Fonts,
    /// Translation of the current control's origin.
    pub tx: f32,
    pub ty: f32,
    /// Absolute clip rectangle.
    pub clip: (i32, i32, i32, i32),
    /// The mask of `mask_clip`, built on demand (`mask_for`).
    pub mask: Option<Mask>,
    mask_clip: Option<(i32, i32, i32, i32)>,
    pub scale: f32,
    /// Dash pattern (dash, gap) of the strokes drawn while set.
    pub dash: Option<(f32, f32)>,
}

/// Set the device pixels of the clip rectangle `(x, y, w, h)` (logical
/// units at `scale`) to `v`, the pixels the non-anti-aliased rasterizer
/// fills for it.
fn set_rect(m: &mut Mask, (x, y, w, h): (i32, i32, i32, i32), scale: f32, v: u8) {
    if w <= 0 || h <= 0 {
        return;
    }
    // Edges round half up (the rasterizer's pixel-centre rule).
    let edge = |e: f32, n: u32| (((e + 0.5).floor().max(0.0)) as u32).min(n) as usize;
    let (mw, mh) = (m.width(), m.height());
    let (x0, x1) = (edge(x as f32 * scale, mw), edge((x + w) as f32 * scale, mw));
    let (y0, y1) = (edge(y as f32 * scale, mh), edge((y + h) as f32 * scale, mh));
    let stride = mw as usize;
    let data = m.data_mut();
    for row in y0..y1 {
        data[row * stride + x0..row * stride + x1].fill(v);
    }
}

impl<'a> Painter<'a> {
    pub fn new(pm: &'a mut Pixmap, fonts: &'a mut Fonts, scale: f32) -> Painter<'a> {
        let clip = (0, 0, (pm.width() as f32 / scale) as i32, (pm.height() as f32 / scale) as i32);
        Painter { pm, fonts, tx: 0.0, ty: 0.0, clip, mask: None, mask_clip: None, scale, dash: None }
    }

    fn transform(&self) -> Transform {
        Transform::from_translate(self.tx, self.ty).post_scale(self.scale, self.scale)
    }

    /// Restrict drawing to the absolute rectangle (intersected with the
    /// current clip); returns the previous clip state. The clip mask is
    /// only built when a draw actually crosses the clip ([`Self::need_mask`]).
    pub fn push_clip(&mut self, x: i32, y: i32, w: i32, h: i32) -> ((i32, i32, i32, i32), Option<Mask>) {
        let (cx, cy, cw, ch) = self.clip;
        let nx = x.max(cx);
        let ny = y.max(cy);
        let nr = (x + w).min(cx + cw);
        let nb = (y + h).min(cy + ch);
        let prev = self.clip;
        self.clip = (nx, ny, (nr - nx).max(0), (nb - ny).max(0));
        (prev, None)
    }

    pub fn pop_clip(&mut self, prev: ((i32, i32, i32, i32), Option<Mask>)) {
        self.clip = prev.0;
    }

    /// Whether a draw covering the device-pixel rectangle `b` needs the
    /// clip mask: not when `b` lies inside the clip (or the clip is the
    /// whole pixmap). When it does, `self.mask` holds the current clip's
    /// mask afterwards (rebuilt only when the clip changed).
    pub fn need_mask(&mut self, b: (f32, f32, f32, f32)) -> bool {
        let s = self.scale;
        let (cx, cy, cw, ch) = self.clip;
        let (x0, y0, x1, y1) = (cx as f32 * s, cy as f32 * s, (cx + cw) as f32 * s, (cy + ch) as f32 * s);
        let whole = x0 <= 0.0 && y0 <= 0.0 && x1 >= self.pm.width() as f32 && y1 >= self.pm.height() as f32;
        if whole || (b.0 >= x0 - 0.01 && b.1 >= y0 - 0.01 && b.0 + b.2 <= x1 + 0.01 && b.1 + b.3 <= y1 + 0.01) {
            return false;
        }
        if self.mask_clip != Some(self.clip) || self.mask.is_none() {
            let (w, h) = (self.pm.width(), self.pm.height());
            let mut m = match self.mask.take() {
                // The mask only ever holds the last clip: unset that, not
                // the whole window.
                Some(mut m) if m.width() == w && m.height() == h => {
                    if let Some(old) = self.mask_clip {
                        set_rect(&mut m, old, s, 0);
                    }
                    m
                }
                _ => Mask::new(w, h).expect("mask"),
            };
            set_rect(&mut m, self.clip, s, 255);
            self.mask = Some(m);
            self.mask_clip = Some(self.clip);
        }
        true
    }

    /// Device-pixel bounds of a path drawn at the current origin, grown by
    /// `pad` (half a stroke width).
    fn device_bounds(&self, path: &tiny_skia::Path, pad: f32) -> (f32, f32, f32, f32) {
        let b = path.bounds();
        let s = self.scale;
        ((b.x() + self.tx - pad) * s, (b.y() + self.ty - pad) * s, (b.width() + 2.0 * pad) * s, (b.height() + 2.0 * pad) * s)
    }

    pub fn clip_is_empty(&self) -> bool {
        self.clip.2 <= 0 || self.clip.3 <= 0
    }

    pub fn fill_path(&mut self, path: &tiny_skia::Path, c: Colour) {
        // A flat path covers nothing (tiny-skia would log a warning).
        let b = path.bounds();
        if c.a == 0 || b.width() <= 0.0 || b.height() <= 0.0 {
            return;
        }
        let t = self.transform();
        let b = self.device_bounds(path, 0.0);
        let m = if self.need_mask(b) { self.mask.as_ref() } else { None };
        self.pm.fill_path(path, &paint(c), FillRule::Winding, t, m);
    }

    pub fn stroke_path(&mut self, path: &tiny_skia::Path, c: Colour, width: f32) {
        if c.a == 0 || width <= 0.0 {
            return;
        }
        self.stroke_path_with(path, &paint(c), width);
    }

    fn stroke_path_with(&mut self, path: &tiny_skia::Path, paint: &Paint, width: f32) {
        let t = self.transform();
        let dash = self.dash.filter(|(d, g)| *d > 0.0 && *g > 0.0).and_then(|(d, g)| StrokeDash::new(vec![d, g], 0.0));
        let s = Stroke { width, dash, ..Stroke::default() };
        let b = self.device_bounds(path, width / 2.0);
        let m = if self.need_mask(b) { self.mask.as_ref() } else { None };
        self.pm.stroke_path(path, paint, &s, t, m);
    }

    /// Fill `path` with a linear gradient from `from` to `g.to` across its
    /// bounds.
    pub fn fill_path_gradient(&mut self, path: &tiny_skia::Path, from: Colour, g: Gradient) {
        let b = path.bounds();
        if (from.a == 0 && g.to.a == 0) || b.width() <= 0.0 || b.height() <= 0.0 {
            return;
        }
        if from == g.to {
            return self.fill_path(path, from);
        }
        let stops = [(0.0, from), (1.0, g.to)];
        let paint = match g.radial {
            Some(r) => {
                let inner = if r.r > 0.0 { (r.r0 / r.r).clamp(0.0, 0.999) } else { 0.0 };
                radial_paint(b.left() + r.cx * b.width(), b.top() + r.cy * b.height(), r.r * b.width(), r.r * b.height(), &[(inner, from), (1.0, g.to)])
            }
            None => {
                let (p0, p1) = if g.horizontal { ((b.left(), 0.0), (b.right(), 0.0)) } else { ((0.0, b.top()), (0.0, b.bottom())) };
                gradient_paint(p0, p1, &stops)
            }
        };
        let t = self.transform();
        let db = self.device_bounds(path, 0.0);
        let m = if self.need_mask(db) { self.mask.as_ref() } else { None };
        self.pm.fill_path(path, &paint, FillRule::Winding, t, m);
    }

    /// Fill `path` in `c`, or with a gradient from `c` when `g` is set.
    pub fn fill_path_opt(&mut self, path: &tiny_skia::Path, c: Colour, g: Option<Gradient>) {
        match g {
            Some(g) => self.fill_path_gradient(path, c, g),
            None => self.fill_path(path, c),
        }
    }

    /// Stroke `path` with a vertical gradient through `stops` (offsets
    /// 0..1 from the top of its bounds to the bottom).
    pub fn stroke_path_vgradient(&mut self, path: &tiny_skia::Path, stops: &[(f32, Colour)], width: f32) {
        let b = path.bounds();
        if width <= 0.0 || b.height() <= 0.0 {
            return;
        }
        self.stroke_path_with(path, &gradient_paint((0.0, b.top()), (0.0, b.bottom()), stops), width);
    }

    /// Overlay `path` with a deterministic brushed texture: light and dark
    /// streaks along x (`streak` logical px long, smoothly interpolated)
    /// plus a per-row shade, `amount` (0..1) the strongest alpha. `seed`
    /// picks the pattern; the same inputs always draw the same pixels.
    pub fn fill_path_noise(&mut self, path: &tiny_skia::Path, amount: f32, streak: f32, seed: u32) {
        let b = path.bounds();
        let amount = amount.clamp(0.0, 1.0);
        if amount <= 0.0 || b.width() <= 0.0 || b.height() <= 0.0 {
            return;
        }
        let s = self.scale;
        let (w, h) = ((b.right() * s).ceil().max(1.0) as u32, (b.bottom() * s).ceil().max(1.0) as u32);
        let Some(mut tex) = Pixmap::new(w, h) else { return };
        let len = (streak * s).max(1.0);
        let data = tex.data_mut();
        for y in 0..h {
            let row = unit(seed, y, 0x0F0F);
            for x in 0..w {
                let f = x as f32 / len;
                let k = f.floor();
                let t = f - k;
                let t = t * t * (3.0 - 2.0 * t);
                let a0 = unit(seed, y, k as u32 * 2 + 1);
                let a1 = unit(seed, y, k as u32 * 2 + 3);
                let n = (0.45 * row + 0.55 * (a0 + (a1 - a0) * t)).clamp(-1.0, 1.0);
                let a = (n.abs() * amount * 255.0).round() as u8;
                let v = if n > 0.0 { a } else { 0 };
                let i = ((y * w + x) * 4) as usize;
                data[i..i + 4].copy_from_slice(&[v, v, v, a]);
            }
        }
        let shader = tiny_skia::Pattern::new(tex.as_ref(), SpreadMode::Pad, tiny_skia::FilterQuality::Nearest, 1.0, Transform::from_scale(1.0 / s, 1.0 / s));
        let paint = Paint { anti_alias: true, shader, ..Default::default() };
        let t = self.transform();
        let db = self.device_bounds(path, 0.0);
        let m = if self.need_mask(db) { self.mask.as_ref() } else { None };
        self.pm.fill_path(path, &paint, FillRule::Winding, t, m);
    }

    /// A soft glow behind text: `s` drawn as [`text_fx`](Self::text_fx)
    /// would in `c`'s colour, blurred by `radius` logical px and
    /// multiplied by `strength` (above 1 brightens the blur's core).
    #[allow(clippy::too_many_arguments)]
    pub fn text_glow(&mut self, r: RectF, s: &str, c: Colour, font: &FontSpec, h: HAlign, v: VAlign, fx: TextFx, radius: f32, strength: f32) {
        if s.is_empty() || c.a == 0 || r.w <= 0.0 || r.h <= 0.0 || radius <= 0.0 {
            return;
        }
        let sc = self.scale;
        let pad = (radius * 2.0).ceil() + 1.0;
        let (pw, ph) = (((r.w + 2.0 * pad) * sc).ceil() as u32, ((r.h + 2.0 * pad) * sc).ceil() as u32);
        let Some(mut tmp) = Pixmap::new(pw.max(1), ph.max(1)) else { return };
        {
            let mut tp = Painter::new(&mut tmp, &mut *self.fonts, sc);
            tp.text_fx(RectF::new(pad, pad, r.w, r.h), s, c.with_alpha(1.0), font, h, v, fx);
        }
        let (w, hh) = (tmp.width() as usize, tmp.height() as usize);
        box_blur(tmp.data_mut(), w, hh, ((radius * sc) / 1.7).round().max(1.0) as usize);
        let k = strength.max(0.0) * c.f_alpha();
        for px in tmp.data_mut().iter_mut() {
            *px = (*px as f32 * k).round().min(255.0) as u8;
        }
        let tx = ((r.x - pad + self.tx) * sc).round();
        let ty = ((r.y - pad + self.ty) * sc).round();
        let b = (tx, ty, pw as f32, ph as f32);
        let m = if self.need_mask(b) { self.mask.as_ref() } else { None };
        self.pm.draw_pixmap(0, 0, tmp.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::from_translate(tx, ty), m);
    }

    pub fn fill_rect(&mut self, r: RectF, c: Colour) {
        if c.a == 0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let s = self.scale;
        // Device rectangle, cut to the clip: a rectangle needs no mask
        // (large panels behind a small repaint stay cheap).
        let (x0, y0, x1, y1) = ((r.x + self.tx) * s, (r.y + self.ty) * s, (r.x + r.w + self.tx) * s, (r.y + r.h + self.ty) * s);
        let (cx, cy, cw, ch) = self.clip;
        let (cx0, cy0, cx1, cy1) = (cx as f32 * s, cy as f32 * s, (cx + cw) as f32 * s, (cy + ch) as f32 * s);
        let (l, t, rr, b) = (x0.max(cx0), y0.max(cy0), x1.min(cx1), y1.min(cy1));
        // Nothing (or a sliver tiny-skia's hairline path rejects) left.
        if rr - l < 0.01 || b - t < 0.01 {
            return;
        }
        let Some(rect) = tiny_skia::Rect::from_ltrb(l, t, rr, b) else { return };
        if rr - l < 2.0 || b - t < 2.0 {
            // Thin rectangles: tiny-skia's anti-aliased rect fast path
            // trips on slivers; the path filler draws them the same.
            self.pm.fill_path(&PathBuilder::from_rect(rect), &paint(c), FillRule::Winding, Transform::identity(), None);
            return;
        }
        // `Pixmap::fill_rect`: no path rasterizer for the commonest shape.
        self.pm.fill_rect(rect, &paint(c), Transform::identity(), None);
    }

    pub fn stroke_rect(&mut self, r: RectF, c: Colour, width: f32) {
        if let Some(rect) = tiny_skia::Rect::from_xywh(r.x, r.y, r.w, r.h) {
            self.stroke_path(&PathBuilder::from_rect(rect), c, width);
        }
    }

    pub fn fill_rounded(&mut self, r: RectF, radius: f32, corners: [bool; 4], c: Colour) {
        if let Some(p) = rounded_rect_path(r, radius, corners) {
            self.fill_path(&p, c);
        }
    }

    pub fn stroke_rounded(&mut self, r: RectF, radius: f32, corners: [bool; 4], c: Colour, width: f32) {
        if let Some(p) = rounded_rect_path(r, radius, corners) {
            self.stroke_path(&p, c, width);
        }
    }

    pub fn fill_ellipse(&mut self, r: RectF, c: Colour) {
        if let Some(rect) = tiny_skia::Rect::from_xywh(r.x, r.y, r.w, r.h) {
            if let Some(p) = PathBuilder::from_oval(rect) {
                self.fill_path(&p, c);
            }
        }
    }

    pub fn stroke_ellipse(&mut self, r: RectF, c: Colour, width: f32) {
        if let Some(rect) = tiny_skia::Rect::from_xywh(r.x, r.y, r.w, r.h) {
            if let Some(p) = PathBuilder::from_oval(rect) {
                self.stroke_path(&p, c, width);
            }
        }
    }

    /// Arc stroke (rect, start, sweep, clockwise, colour, width).
    pub fn stroke_arc(&mut self, r: RectF, start: f32, sweep: f32, clockwise: bool, c: Colour, width: f32) {
        if sweep.abs() < 1e-3 {
            return;
        }
        let mut pb = PathBuilder::new();
        let s = if clockwise { sweep } else { -sweep };
        arc_to(&mut pb, r.x + r.w / 2.0, r.y + r.h / 2.0, r.w / 2.0, r.h / 2.0, start, s, true);
        if let Some(p) = pb.finish() {
            self.stroke_path(&p, c, width);
        }
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Colour, width: f32) {
        let mut pb = PathBuilder::new();
        pb.move_to(x0, y0);
        pb.line_to(x1, y1);
        if let Some(p) = pb.finish() {
            self.stroke_path(&p, c, width);
        }
    }

    pub fn polyline(&mut self, pts: &[(f32, f32)], c: Colour, width: f32) {
        let mut pb = PathBuilder::new();
        for (i, (x, y)) in pts.iter().enumerate() {
            if i == 0 {
                pb.move_to(*x, *y);
            } else {
                pb.line_to(*x, *y);
            }
        }
        if let Some(p) = pb.finish() {
            self.stroke_path(&p, c, width);
        }
    }

    pub fn fill_polygon(&mut self, pts: &[(f32, f32)], c: Colour) {
        let mut pb = PathBuilder::new();
        for (i, (x, y)) in pts.iter().enumerate() {
            if i == 0 {
                pb.move_to(*x, *y);
            } else {
                pb.line_to(*x, *y);
            }
        }
        pb.close();
        if let Some(p) = pb.finish() {
            self.fill_path(&p, c);
        }
    }

    /// Text rotated −90° (reading upwards): laid out in a `len` × `thick`
    /// rect whose left edge sits on (`x`, `bottom`) and runs up.
    #[allow(clippy::too_many_arguments)]
    pub fn text_up(&mut self, x: f32, bottom: f32, len: f32, thick: f32, s: &str, c: Colour, font: &FontSpec, h: HAlign, v: VAlign) {
        let sc = self.scale;
        let (pw, ph) = ((len * sc).ceil().max(1.0) as u32, (thick * sc).ceil().max(1.0) as u32);
        let Some(mut tmp) = Pixmap::new(pw, ph) else { return };
        {
            let mut tp = Painter::new(&mut tmp, &mut *self.fonts, sc);
            tp.text(RectF::new(0.0, 0.0, len, thick), s, c, font, h, v);
        }
        // temp (u, v) → (x + v, bottom − u), in device pixels.
        let t = Transform::from_row(0.0, -1.0, 1.0, 0.0, (x + self.tx) * sc, (bottom + self.ty) * sc);
        let b = ((x + self.tx) * sc, (bottom + self.ty - len) * sc, thick * sc, len * sc);
        let m = if self.need_mask(b) { self.mask.as_ref() } else { None };
        self.pm.draw_pixmap(0, 0, tmp.as_ref(), &tiny_skia::PixmapPaint::default(), t, m);
    }

    /// [re:128485].
    pub fn text(&mut self, r: RectF, s: &str, c: Colour, font: &FontSpec, h: HAlign, v: VAlign) {
        self.text_spaced(r, s, c, font, h, v, 0.0);
    }

    /// The width of `s` in `font` with `spacing` between letters.
    pub fn text_width(&mut self, font: &FontSpec, s: &str, spacing: f32) -> f32 {
        let n = s.chars().count();
        self.fonts.measure(font, s).0 + spacing * n.saturating_sub(1) as f32
    }

    /// [`text`](Self::text) with [`TextFx`]: letter spacing is drawn
    /// directly; a horizontal scale or a slant draws the text into a layer
    /// and transforms it into place (scaled from its aligned edge, slanted
    /// about the rect's vertical middle).
    #[allow(clippy::too_many_arguments)]
    pub fn text_fx(&mut self, r: RectF, s: &str, c: Colour, font: &FontSpec, h: HAlign, v: VAlign, fx: TextFx) {
        if fx.upright() {
            return self.text_spaced(r, s, c, font, h, v, fx.spacing);
        }
        if s.is_empty() || c.a == 0 || r.h <= 0.0 {
            return;
        }
        let sc = self.scale;
        let sx = fx.scale_x.clamp(0.1, 4.0);
        let k = fx.slant.clamp(-45.0, 45.0).to_radians().tan();
        let tw = self.text_width(font, s, fx.spacing);
        const PAD: f32 = 2.0;
        let lw = tw + 2.0 * PAD;
        let (pw, ph) = ((lw * sc).ceil().max(1.0) as u32, (r.h * sc).ceil().max(1.0) as u32);
        let Some(mut tmp) = Pixmap::new(pw, ph) else { return };
        {
            let mut tp = Painter::new(&mut tmp, &mut *self.fonts, sc);
            tp.text_spaced(RectF::new(PAD, 0.0, tw + PAD, r.h), s, c, font, HAlign::Left, v, fx.spacing);
        }
        let sw = tw * sx;
        let start = match h {
            HAlign::Left => r.x,
            HAlign::Center => r.x + (r.w - sw) / 2.0,
            HAlign::Right => r.x + r.w - sw,
        };
        // Layer (u, v) -> (start + sx (u - pad) - k (v - mid), r.y + v), device px.
        let (pad, mid) = (PAD * sc, r.h * sc / 2.0);
        let tx = (start + self.tx) * sc - sx * pad + k * mid;
        let ty = (r.y + self.ty) * sc;
        let t = Transform::from_row(sx, 0.0, -k, 1.0, tx, ty);
        let lean = k.abs() * mid;
        let b = (tx.min(tx - k * r.h * sc) - 1.0, ty, lw * sc * sx + lean * 2.0 + 2.0, r.h * sc);
        let m = if self.need_mask(b) { self.mask.as_ref() } else { None };
        let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
        self.pm.draw_pixmap(0, 0, tmp.as_ref(), &paint, t, m);
    }

    /// [`text`](Self::text) with `spacing` logical px added after every
    /// letter but the last.
    #[allow(clippy::too_many_arguments)]
    pub fn text_spaced(&mut self, r: RectF, s: &str, c: Colour, font: &FontSpec, h: HAlign, v: VAlign, spacing: f32) {
        if s.is_empty() || c.a == 0 {
            return;
        }
        let scale = self.scale;
        let (tx, ty) = (self.tx, self.ty);
        let clip = self.clip;
        self.fonts.prepare(font);
        let fonts = &*self.fonts;
        let size = fonts.size(font) * scale;
        let Some(primary) = fonts.font_for(font, 'A') else { return };
        let mut width: f32 = s.chars().map(|ch| fonts.font_for(font, ch).map(|f| f.metrics(ch, size).advance_width).unwrap_or(0.0)).sum();
        let gap = spacing * scale;
        if gap != 0.0 {
            width += gap * s.chars().count().saturating_sub(1) as f32;
        }
        let (asc, desc) = primary.horizontal_line_metrics(size).map(|m| (m.ascent, m.descent)).unwrap_or((size * 0.8, -size * 0.2));
        let (rx, ry, rw, rh) = ((r.x + tx) * scale, (r.y + ty) * scale, r.w * scale, r.h * scale);
        let x0 = match h {
            HAlign::Left => rx,
            HAlign::Center => rx + (rw - width) / 2.0,
            HAlign::Right => rx + rw - width,
        };
        let text_h = asc - desc;
        let baseline = match v {
            VAlign::Top => ry + asc,
            VAlign::Center => ry + (rh - text_h) / 2.0 + asc,
            VAlign::Bottom => ry + rh + desc,
        };
        // Floors (not truncation): a region repaint's pixmap puts controls
        // left of or above it at negative coordinates.
        let (cx0, cy0) = ((clip.0 as f32 * scale).floor() as i32, (clip.1 as f32 * scale).floor() as i32);
        let (cx1, cy1) = (((clip.0 + clip.2) as f32 * scale).floor() as i32, ((clip.1 + clip.3) as f32 * scale).floor() as i32);
        let pw = self.pm.width() as i32;
        let ph = self.pm.height() as i32;
        let mut pen = x0.round();
        let baseline = baseline.round();
        let mut glyphs = Vec::new();
        for ch in s.chars() {
            let Some(f) = fonts.font_for(font, ch) else { continue };
            let (m, bmp) = f.rasterize(ch, size);
            glyphs.push((pen.floor() as i32 + m.xmin, baseline as i32 - m.height as i32 - m.ymin, m.width, m.height, bmp));
            pen += m.advance_width + gap;
        }
        let data = self.pm.pixels_mut();
        for (gx, gy, gw, gh, bmp) in glyphs {
            for yy in 0..gh {
                let py = gy + yy as i32;
                if py < cy0.max(0) || py >= cy1.min(ph) {
                    continue;
                }
                for xx in 0..gw {
                    let px = gx + xx as i32;
                    if px < cx0.max(0) || px >= cx1.min(pw) {
                        continue;
                    }
                    let cov = bmp[yy * gw + xx] as u32;
                    if cov == 0 {
                        continue;
                    }
                    let a = cov * c.a as u32 / 255;
                    let d = &mut data[(py * pw + px) as usize];
                    let inv = 255 - a;
                    let blend = |s: u8, dd: u8| ((s as u32 * a + dd as u32 * inv) / 255) as u8;
                    *d = tiny_skia::PremultipliedColorU8::from_rgba(
                        blend(c.r, d.red()).min(blend(255, d.alpha())),
                        blend(c.g, d.green()).min(blend(255, d.alpha())),
                        blend(c.b, d.blue()).min(blend(255, d.alpha())),
                        blend(255, d.alpha()),
                    )
                    .unwrap_or(*d);
                }
            }
        }
    }
}

#[cfg(test)]
mod mask_tests {
    use super::*;

    /// The incremental clip mask equals the rasterized rectangle.
    #[test]
    fn clip_masks_match_the_rasterizer() {
        for scale in [1.0f32, 1.5, 2.0, 1.25] {
            let mut prev: Option<(i32, i32, i32, i32)> = None;
            let mut m = Mask::new(300, 200).unwrap();
            for (i, clip) in [(0, 0, 50, 40), (13, 7, 61, 33), (100, 90, 1, 1), (3, 150, 200, 49), (0, 0, 300, 200), (199, 0, 3, 199)].into_iter().enumerate() {
                if let Some(old) = prev {
                    set_rect(&mut m, old, scale, 0);
                }
                set_rect(&mut m, clip, scale, 255);
                prev = Some(clip);
                let mut r = Mask::new(300, 200).unwrap();
                let rect = tiny_skia::Rect::from_xywh(clip.0 as f32, clip.1 as f32, clip.2 as f32, clip.3 as f32).unwrap();
                r.fill_path(&PathBuilder::from_rect(rect), FillRule::Winding, false, Transform::from_scale(scale, scale));
                if m.data() != r.data() {
                    let span = |d: &[u8]| {
                        let cols: Vec<usize> = (0..300).filter(|c| (0..200).any(|rw| d[rw * 300 + c] != 0)).collect();
                        let rows: Vec<usize> = (0..200).filter(|rw| (0..300).any(|c| d[rw * 300 + c] != 0)).collect();
                        (cols.first().copied(), cols.last().copied(), rows.first().copied(), rows.last().copied())
                    };
                    panic!("scale {scale} clip {i}: {clip:?} ours {:?} raster {:?}", span(m.data()), span(r.data()));
                }
            }
        }
    }
}
