//! Mouse cursors ([re:9c94c6], [re:16425e]).
//!
//! - System cursors [re:1e246f]: 0 arrow, 1 busy (the arrow), 2 open hand,
//!   3 up/down, 4 left/right, 5 I-beam, 6 resize / 7 cross-hairs / 8 plus
//!   (all the crosshair), 9 NW-SE resize (HIServices'
//!   `resizenorthwestsoutheast` cursor), 10 operation not allowed.
//! - Loading [re:bc58a6]: every image in `Resources/Cursors` becomes a
//!   named cursor; [re:f09031] ([re:7681c8] x, y, cursorName)
//!   gives hot spots, others use (2, 2).
//!
//! The app records the wanted cursor in [`crate::app::App::cursor`]; the
//! window applies it when it changes (macOS: `NSCursor`).

use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Arrow,
    Busy,
    Hand,
    UpDown,
    LeftRight,
    IBeam,
    Resize,
    CrossHairs,
    Plus,
    ResizeNwse,
    NotAllowed,
    /// A named image cursor from `Resources/Cursors` ("DragPad", ...).
    Image(String),
}

impl Cursor {
    pub fn image(name: &str) -> Cursor {
        Cursor::Image(name.to_string())
    }
}

/// [re:57899c] for a transmod knob /
/// slider under the pointer at (`x`, `y`) (control-relative): the class
/// cursors are TransmodPrimary (ModCursor 0), TransmodSecondary (1) and
/// TransmodBoth (2) (editor open [re:468d3b]). Not modulated (no source
/// selected) the drag mode is 3 → Primary. Modulated: knob inside its
/// centre circle (square knobs only, [re:db47c7]) / slider over
/// its main track (drag mode [re:654080]: x < w − round(w·realModDim),
/// `realModDim` 0.25 when the skin leaves it out, [re:c41e95])
/// → alt ? 1 (Primary) : 3 (Both); elsewhere 2 (Secondary).
pub fn transmod(c: &crate::ctrl::Ctrl, x: f32, y: f32, alt: bool) -> Option<Cursor> {
    let modulated = c.attr("editingDepth") == Some("true");
    let (w, h) = (c.rect.w, c.rect.h);
    let inner = match c.class.as_str() {
        "CtrlTransmodKnob" => {
            if w != h {
                false
            } else {
                let r = w / 2;
                let (dx, dy) = (r - x as i32, r - y as i32);
                (dx * dx + dy * dy) as u32 <= (r * r) as u32
            }
        }
        "CtrlTransmodSlider" => {
            let strip = (w as f32 * c.attr_f32("realModDim", 0.25)).round() as i32;
            (x as i32) < (w - strip).max(0) && (y as i32) < h && x >= 0.0 && y >= 0.0
        }
        _ => return None,
    };
    let name = if !modulated {
        "TransmodPrimary"
    } else if inner {
        if alt { "TransmodPrimary" } else { "TransmodBoth" }
    } else {
        "TransmodSecondary"
    };
    Some(Cursor::image(name))
}

/// Hot spot of a named image cursor ([re:f09031], default (2, 2)).
pub fn hotspot(cursors_dir: &Path, name: &str) -> (i32, i32) {
    let Ok(text) = std::fs::read_to_string(cursors_dir.join("Cursors.hotspots")) else { return (2, 2) };
    let Ok(root) = dreist_formats::Node::parse(&text) else { return (2, 2) };
    fn find(n: &dreist_formats::Node, name: &str) -> Option<(i32, i32)> {
        if n.class == "CursorHotspot" && n.attr("cursorName") == Some(name) {
            let v = |k: &str| n.attr(k).and_then(|s| s.trim().parse().ok()).unwrap_or(2);
            return Some((v("x"), v("y")));
        }
        n.children.iter().find_map(|c| find(c, name))
    }
    find(&root, name).unwrap_or((2, 2))
}

/// Make `c` the current cursor.
#[cfg(all(feature = "window", target_os = "macos"))]
#[allow(unexpected_cfgs)]
pub fn apply(c: &Cursor, cursors_dir: &Path) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSPoint {
        x: f64,
        y: f64,
    }
    unsafe impl objc::Encode for NSPoint {
        fn encode() -> objc::Encoding {
            unsafe { objc::Encoding::from_str("{CGPoint=dd}") }
        }
    }

    thread_local! {
        static CACHE: RefCell<HashMap<String, usize>> = RefCell::new(HashMap::new());
    }

    fn nsstring(s: &str) -> *mut Object {
        let c = std::ffi::CString::new(s).unwrap_or_default();
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
    }

    unsafe fn from_image(path: &str, hot: (i32, i32)) -> *mut Object {
        let img: *mut Object = msg_send![class!(NSImage), alloc];
        let img: *mut Object = msg_send![img, initWithContentsOfFile: nsstring(path)];
        if img.is_null() {
            return std::ptr::null_mut();
        }
        let cur: *mut Object = msg_send![class!(NSCursor), alloc];
        msg_send![cur, initWithImage: img hotSpot: NSPoint { x: hot.0 as f64, y: hot.1 as f64 }]
    }

    let key = format!("{c:?}");
    let cached = CACHE.with(|m| m.borrow().get(&key).copied());
    let cur = match cached {
        Some(p) => p as *mut Object,
        None => {
            let cur: *mut Object = unsafe {
                match c {
                    Cursor::Arrow | Cursor::Busy => msg_send![class!(NSCursor), arrowCursor],
                    Cursor::Hand => msg_send![class!(NSCursor), openHandCursor],
                    Cursor::UpDown => msg_send![class!(NSCursor), resizeUpDownCursor],
                    Cursor::LeftRight => msg_send![class!(NSCursor), resizeLeftRightCursor],
                    Cursor::IBeam => msg_send![class!(NSCursor), IBeamCursor],
                    Cursor::Resize | Cursor::CrossHairs | Cursor::Plus => msg_send![class!(NSCursor), crosshairCursor],
                    Cursor::NotAllowed => msg_send![class!(NSCursor), operationNotAllowedCursor],
                    Cursor::ResizeNwse => {
                        let dir = "/System/Library/Frameworks/ApplicationServices.framework/Versions/A/Frameworks/HIServices.framework/Versions/A/Resources/cursors/resizenorthwestsoutheast";
                        let hot = std::fs::read_to_string(format!("{dir}/info.plist")).ok().map(|t| {
                            let num = |k: &str| {
                                t.split(&format!("<key>{k}</key>")).nth(1).and_then(|r| r.split('>').nth(1)).and_then(|r| r.split('<').next()).and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(8.0)
                            };
                            (num("hotx") as i32, num("hoty") as i32)
                        });
                        let p = from_image(&format!("{dir}/cursor.pdf"), hot.unwrap_or((8, 8)));
                        if p.is_null() { msg_send![class!(NSCursor), crosshairCursor] } else { p }
                    }
                    Cursor::Image(name) => {
                        let p = from_image(&cursors_dir.join(format!("{name}.png")).to_string_lossy(), hotspot(cursors_dir, name));
                        if p.is_null() { msg_send![class!(NSCursor), arrowCursor] } else { p }
                    }
                }
            };
            if !cur.is_null() {
                let _: *mut Object = unsafe { msg_send![cur, retain] };
                CACHE.with(|m| m.borrow_mut().insert(key, cur as usize));
            }
            cur
        }
    };
    if !cur.is_null() {
        let _: () = unsafe { msg_send![cur, set] };
    }
}

#[cfg(not(all(feature = "window", target_os = "macos")))]
pub fn apply(_c: &Cursor, _cursors_dir: &Path) {}

#[cfg(test)]
mod tests {
    #[test]
    fn transmod_cursors_follow_the_drag_modes() {
        use super::{Cursor, transmod};
        use crate::ctrl::{Ctrl, Rect};
        let mut k = Ctrl { class: "CtrlTransmodKnob".into(), rect: Rect::new(0, 0, 32, 32), ..Ctrl::default() };
        assert_eq!(transmod(&k, 16.0, 16.0, false), Some(Cursor::image("TransmodPrimary")));
        k.set_attr("editingDepth", "true".into());
        assert_eq!(transmod(&k, 16.0, 16.0, false), Some(Cursor::image("TransmodBoth")));
        assert_eq!(transmod(&k, 16.0, 16.0, true), Some(Cursor::image("TransmodPrimary")));
        assert_eq!(transmod(&k, 0.0, 0.0, false), Some(Cursor::image("TransmodSecondary")));
        let mut s = Ctrl { class: "CtrlTransmodSlider".into(), rect: Rect::new(0, 0, 20, 128), ..Ctrl::default() };
        s.set_attr("editingDepth", "true".into());
        assert_eq!(transmod(&s, 5.0, 50.0, false), Some(Cursor::image("TransmodBoth")));
        assert_eq!(transmod(&s, 18.0, 50.0, false), Some(Cursor::image("TransmodSecondary")));
        assert_eq!(transmod(&Ctrl::default(), 0.0, 0.0, false), None);
    }

    #[test]
    fn hotspots_from_the_ui_folder() {
        let dir = dreist_formats::paths::ui_dir().join("Cursors");
        if !dir.is_dir() {
            return;
        }
        assert_eq!(super::hotspot(&dir, "ResizeLeft"), (3, 10));
        assert_eq!(super::hotspot(&dir, "DragPad"), (8, 4));
        assert_eq!(super::hotspot(&dir, "NoSuchCursor"), (2, 2));
    }

    /// Every image cursor the GUI code asks for is drawn (the app's
    /// `Cursors`; each app's editor checks its own).
    #[test]
    fn every_named_cursor_has_an_image() {
        let dir = dreist_formats::paths::ui_dir().join("Cursors");
        if !dir.is_dir() {
            return;
        }
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut names = Vec::new();
        for e in std::fs::read_dir(src).unwrap().flatten() {
            let text = std::fs::read_to_string(e.path()).unwrap_or_default();
            for call in text.split("Cursor::image(").skip(1) {
                let args = call.split(')').next().unwrap_or("");
                names.extend(args.split('"').skip(1).step_by(2).filter(|n| !n.is_empty() && !n.contains(['{', ' '])).map(str::to_string));
            }
        }
        names.sort();
        names.dedup();
        assert!(!names.is_empty());
        for n in names {
            assert!(dir.join(format!("{n}.png")).is_file(), "no cursor image {n}");
        }
    }
}
