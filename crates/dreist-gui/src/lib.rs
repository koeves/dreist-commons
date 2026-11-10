//! [re:32e6ed]-compatible GUI toolkit for the v2 rebuild: loads the installed
//! `.skin`/`.stylesheet` resources at runtime and draws them with tiny-skia.
//! See the dreist-re GUI notes.

pub mod app;
pub mod collapse;
pub mod colour;
pub mod ctrl;
pub mod cursor;
pub mod dragout;
pub mod menu;
pub mod menubar;
pub mod meter;
#[cfg(all(feature = "window", target_os = "macos"))]
mod objc_class;
pub mod paint;
pub mod render;
pub mod scroll;
pub mod skin;
pub mod splitter;
pub mod style;
pub mod text;
pub mod tooltip;
pub mod v2ctl;
#[cfg(feature = "window")]
pub mod window;

pub use colour::Colour;
pub use ctrl::{Anchor, Ctrl, Rect, anchors_fill};
pub use skin::Skin;
pub use tiny_skia;

/// Render `root` into a new pixmap of its size at `scale`.
pub fn render_ctrl(skin: &Skin, fonts: &mut text::Fonts, root: &Ctrl, scale: f32) -> tiny_skia::Pixmap {
    render_ctrl_with(skin, fonts, root, scale, None)
}

/// [`render_ctrl`] with custom drawing for overload controls.
pub fn render_ctrl_with(skin: &Skin, fonts: &mut text::Fonts, root: &Ctrl, scale: f32, custom: Option<&dyn render::CustomDraw>) -> tiny_skia::Pixmap {
    render_with_overlay(skin, fonts, root, scale, custom, None, None)
}

/// Repaint the root-coordinate rectangle `r` of an existing frame `pm`:
/// the backdrop, then the tree drawn into a pixmap of just that size
/// (tiny-skia stops at a pixmap's edges, so large shapes crossing the
/// region cost only what they cover) and copied in. Gives the same
/// pixels as a full render (up to anti-aliasing rounding).
pub fn render_region(
    skin: &Skin,
    fonts: &mut text::Fonts,
    root: &Ctrl,
    scale: f32,
    custom: Option<&dyn render::CustomDraw>,
    menu: Option<&menu::Menu>,
    tip: Option<&tooltip::Tip>,
    pm: &mut tiny_skia::Pixmap,
    r: ctrl::Rect,
) {
    let x0 = r.x.max(0);
    let y0 = r.y.max(0);
    let x1 = (r.x + r.w).min(root.rect.w);
    let y1 = (r.y + r.h).min(root.rect.h);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let back = skin.styles.colours.get("PanelDarkest").copied().unwrap_or(Colour::BLACK);
    let (dx0, dy0) = (x0 as f32 * scale, y0 as f32 * scale);
    let (dx1, dy1) = ((x1 as f32 * scale).ceil().min(pm.width() as f32), (y1 as f32 * scale).ceil().min(pm.height() as f32));
    // Device-aligned regions only (any integer scale); otherwise draw in place.
    let aligned = dx0.fract() == 0.0 && dy0.fract() == 0.0;
    let (w, h) = ((dx1 - dx0) as u32, (dy1 - dy0) as u32);
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(back.r, back.g, back.b, 255);
    paint.anti_alias = false;
    if aligned && w > 0 && h > 0 {
        // Drawn with a margin and copied back without it: a shape crossing
        // a pixmap's edge gets wrong coverage in the edge pixels.
        const MARGIN: i32 = 4;
        let (px0, py0) = ((x0 - MARGIN).max(0), (y0 - MARGIN).max(0));
        let (px1, py1) = ((x1 + MARGIN).min(root.rect.w), (y1 + MARGIN).min(root.rect.h));
        let (tw, th) = (((px1 - px0) as f32 * scale).ceil() as u32, ((py1 - py0) as f32 * scale).ceil() as u32);
        let Some(mut tmp) = tiny_skia::Pixmap::new(tw, th) else { return };
        tmp.fill(tiny_skia::Color::from_rgba8(back.r, back.g, back.b, 255));
        {
            let mut p = paint::Painter::new(&mut tmp, fonts, scale);
            // The padded region's corner becomes the origin.
            p.tx = -(root.rect.x + px0) as f32;
            p.ty = -(root.rect.y + py0) as f32;
            p.clip = (0, 0, px1 - px0, py1 - py0);
            let mut rd = render::Renderer::new(&skin.styles);
            rd.custom = custom;
            rd.draw(&mut p, root);
            // The open menu on top, in the same shifted space.
            p.tx = -px0 as f32;
            p.ty = -py0 as f32;
            p.clip = (0, 0, px1 - px0, py1 - py0);
            if let Some(m) = menu {
                m.draw(&skin.styles, &mut p);
            }
            if let Some(t) = tip {
                t.draw(&skin.styles, &mut p);
            }
        }
        // Copy the region's rows (both pixmaps are premultiplied RGBA).
        let (ox, oy) = (((x0 - px0) as f32 * scale) as usize, ((y0 - py0) as f32 * scale) as usize);
        let (pw, tw) = (pm.width() as usize, tw as usize);
        let src = tmp.data();
        let dst = pm.data_mut();
        for row in 0..h as usize {
            let s0 = ((oy + row) * tw + ox) * 4;
            let d0 = ((dy0 as usize + row) * pw + dx0 as usize) * 4;
            dst[d0..d0 + w as usize * 4].copy_from_slice(&src[s0..s0 + w as usize * 4]);
        }
        return;
    }
    if let Some(rect) = tiny_skia::Rect::from_ltrb(dx0.floor(), dy0.floor(), dx1, dy1) {
        pm.fill_rect(rect, &paint, tiny_skia::Transform::identity(), None);
    }
    let mut p = paint::Painter::new(pm, fonts, scale);
    p.tx = -root.rect.x as f32;
    p.ty = -root.rect.y as f32;
    p.clip = (x0, y0, x1 - x0, y1 - y0);
    let mut rd = render::Renderer::new(&skin.styles);
    rd.custom = custom;
    rd.draw(&mut p, root);
    p.tx = 0.0;
    p.ty = 0.0;
    p.clip = (x0, y0, x1 - x0, y1 - y0);
    if let Some(m) = menu {
        m.draw(&skin.styles, &mut p);
    }
    if let Some(t) = tip {
        t.draw(&skin.styles, &mut p);
    }
}

/// Render with an open popup menu on top.
pub fn render_with_overlay(
    skin: &Skin,
    fonts: &mut text::Fonts,
    root: &Ctrl,
    scale: f32,
    custom: Option<&dyn render::CustomDraw>,
    menu: Option<&menu::Menu>,
    tip: Option<&tooltip::Tip>,
) -> tiny_skia::Pixmap {
    render_full(skin, fonts, root, scale, custom, menu, tip, None)
}

/// [`render_with_overlay`], collecting the rects custom drawers painted
/// (root coordinates) into `custom_drawn`.
#[allow(clippy::too_many_arguments)]
pub fn render_full(
    skin: &Skin,
    fonts: &mut text::Fonts,
    root: &Ctrl,
    scale: f32,
    custom: Option<&dyn render::CustomDraw>,
    menu: Option<&menu::Menu>,
    tip: Option<&tooltip::Tip>,
    custom_drawn: Option<&std::cell::RefCell<Vec<ctrl::Rect>>>,
) -> tiny_skia::Pixmap {
    let w = ((root.rect.w.max(1)) as f32 * scale).ceil() as u32;
    let h = ((root.rect.h.max(1)) as f32 * scale).ceil() as u32;
    let mut pm = tiny_skia::Pixmap::new(w, h).expect("pixmap");
    render_full_into(skin, fonts, root, scale, custom, menu, tip, custom_drawn, None, &mut pm);
    pm
}

/// [`render_full`] into an existing pixmap of the window's size; with
/// `custom_paths`, the custom-drawn controls outside layers are listed
/// there by path instead of in `custom_drawn` (see
/// [`render::replay_custom`]).
#[allow(clippy::too_many_arguments)]
pub fn render_full_into(
    skin: &Skin,
    fonts: &mut text::Fonts,
    root: &Ctrl,
    scale: f32,
    custom: Option<&dyn render::CustomDraw>,
    menu: Option<&menu::Menu>,
    tip: Option<&tooltip::Tip>,
    custom_drawn: Option<&std::cell::RefCell<Vec<ctrl::Rect>>>,
    custom_paths: Option<&std::cell::RefCell<Vec<Vec<usize>>>>,
    pm: &mut tiny_skia::Pixmap,
) {
    // The window backdrop ([re:8c39a3] fill) behind transparent panels.
    let back = skin.styles.colours.get("PanelDarkest").copied().unwrap_or(Colour::BLACK);
    pm.fill(tiny_skia::Color::from_rgba8(back.r, back.g, back.b, 255));
    let mut p = paint::Painter::new(pm, fonts, scale);
    p.tx = -root.rect.x as f32;
    p.ty = -root.rect.y as f32;
    let mut r = render::Renderer::new(&skin.styles);
    r.custom = custom;
    r.custom_drawn = custom_drawn;
    r.custom_paths = custom_paths;
    r.draw(&mut p, root);
    p.tx = 0.0;
    p.ty = 0.0;
    p.clip = (0, 0, root.rect.w, root.rect.h);
    if let Some(m) = menu {
        m.draw(&skin.styles, &mut p);
    }
    if let Some(t) = tip {
        t.draw(&skin.styles, &mut p);
    }
}

/// The device-pixel rectangle of the root-coordinate rectangle `r` in a
/// frame of `root` at `scale`, when it lies on whole device pixels (any
/// integer scale): `(x, y, w, h)`.
pub fn device_rect(root: &Ctrl, scale: f32, r: ctrl::Rect) -> Option<(u32, u32, u32, u32)> {
    let (x0, y0) = (r.x.max(0), r.y.max(0));
    let (x1, y1) = ((r.x + r.w).min(root.rect.w), (r.y + r.h).min(root.rect.h));
    let (dx0, dy0, dx1, dy1) = (x0 as f32 * scale, y0 as f32 * scale, x1 as f32 * scale, y1 as f32 * scale);
    let whole = |v: f32| v.fract() == 0.0;
    (x1 > x0 && y1 > y0 && whole(dx0) && whole(dy0) && whole(dx1) && whole(dy1)).then_some((dx0 as u32, dy0 as u32, (dx1 - dx0) as u32, (dy1 - dy0) as u32))
}

/// Copy the device rectangle `d` of `src` (a whole frame) into `dst` (just
/// that rectangle's size), or back with `to_frame`.
pub fn copy_device_rect(frame: &mut tiny_skia::Pixmap, part: &mut tiny_skia::Pixmap, d: (u32, u32, u32, u32), to_frame: bool) {
    let (x, y, w, h) = (d.0 as usize, d.1 as usize, d.2 as usize, d.3 as usize);
    let fw = frame.width() as usize;
    if part.width() as usize != w || part.height() as usize != h || x + w > fw || y + h > frame.height() as usize {
        return;
    }
    for row in 0..h {
        let f0 = ((y + row) * fw + x) * 4;
        let p0 = row * w * 4;
        if to_frame {
            frame.data_mut()[f0..f0 + w * 4].copy_from_slice(&part.data()[p0..p0 + w * 4]);
        } else {
            part.data_mut()[p0..p0 + w * 4].copy_from_slice(&frame.data()[f0..f0 + w * 4]);
        }
    }
}

/// Draw the open menu over `under`, the pixels of root-coordinate
/// rectangle `r` (device rectangle `d`, [`device_rect`]) without it, into
/// that rectangle of `frame`: a menu's hover repaints the menu alone. The
/// menu must lie inside `r`. Gives the pixels [`render_region`] gives.
#[allow(clippy::too_many_arguments)]
pub fn render_menu_over(skin: &Skin, fonts: &mut text::Fonts, scale: f32, menu: &menu::Menu, under: &tiny_skia::Pixmap, frame: &mut tiny_skia::Pixmap, r: ctrl::Rect, d: (u32, u32, u32, u32)) {
    let mut tmp = under.clone();
    {
        let mut p = paint::Painter::new(&mut tmp, fonts, scale);
        let (x0, y0) = (r.x.max(0), r.y.max(0));
        p.tx = -x0 as f32;
        p.ty = -y0 as f32;
        p.clip = (0, 0, (d.2 as f32 / scale) as i32, (d.3 as f32 / scale) as i32);
        menu.draw(&skin.styles, &mut p);
    }
    copy_device_rect(frame, &mut tmp, d, true);
}
