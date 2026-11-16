//! The standalone app's menu bar (macOS `NSMenu`). Each item carries a tag;
//! choosing it queues the tag for the owner to drain with [`take`] once per
//! frame.

use std::sync::Mutex;

/// One menu item: a title ("" = separator), its key equivalent (only
/// given for keys with ⌘, so plain keys keep reaching the window) and a
/// tag. `submenu` items open further items instead.
#[derive(Clone, Debug, Default)]
pub struct Item {
    pub title: String,
    pub key: String,
    pub shift: bool,
    pub cmd: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub tag: isize,
    pub submenu: Vec<Item>,
}

impl Item {
    pub fn separator() -> Item {
        Item::default()
    }
}

static CHOSEN: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// Tags of the items chosen since the last call.
pub fn take() -> Vec<isize> {
    CHOSEN.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
}

/// The class of the menu items' target, which records the chosen tags; its
/// name is unique to this copy of the library (`objc_class`).
#[cfg(all(feature = "window", target_os = "macos"))]
#[allow(unexpected_cfgs)]
pub(crate) fn target_class() -> &'static objc::runtime::Class {
    use objc::runtime::{Class, Object, Sel};
    use objc::{class, msg_send, sel, sel_impl};

    extern "C" fn chosen(_: &Object, _: Sel, sender: *mut Object) {
        let tag: isize = unsafe { msg_send![sender, tag] };
        if let Ok(mut v) = CHOSEN.lock() {
            v.push(tag);
        }
    }
    static CLASS: std::sync::OnceLock<&'static Class> = std::sync::OnceLock::new();
    CLASS.get_or_init(|| {
        let mut decl = crate::objc_class::declare(&format!("{}MenuTarget", dreist_formats::profile::get().product), class!(NSObject));
        unsafe {
            decl.add_method(sel!(menuChosen:), chosen as extern "C" fn(&Object, Sel, *mut Object));
        }
        decl.register()
    })
}

/// Replace the application's main menu: the app menu (About, Preferences,
/// the given extra items, Hide / Hide Others / Show All, Quit) followed by
/// `menus`.
#[cfg(all(feature = "window", target_os = "macos"))]
#[allow(unexpected_cfgs, unsafe_op_in_unsafe_fn)]
pub fn install(app_name: &str, app_items: &[Item], menus: &[(String, Vec<Item>)]) {
    use objc::runtime::{Object, Sel};
    use objc::{class, msg_send, sel, sel_impl};

    fn nsstring(s: &str) -> *mut Object {
        let c = std::ffi::CString::new(s).unwrap_or_default();
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
    }
    unsafe fn menu(title: &str, items: &[Item], target: *mut Object) -> *mut Object {
        let m: *mut Object = msg_send![class!(NSMenu), alloc];
        let m: *mut Object = msg_send![m, initWithTitle: nsstring(title)];
        let _: () = msg_send![m, setAutoenablesItems: false];
        for it in items {
            let _: () = msg_send![m, addItem: item(it, target)];
        }
        m
    }
    unsafe fn item(it: &Item, target: *mut Object) -> *mut Object {
        if it.title.is_empty() {
            return msg_send![class!(NSMenuItem), separatorItem];
        }
        let i: *mut Object = msg_send![class!(NSMenuItem), alloc];
        let key = if it.cmd { it.key.as_str() } else { "" };
        let action = if it.submenu.is_empty() { sel!(menuChosen:) } else { Sel::from_ptr(std::ptr::null()) };
        let i: *mut Object = msg_send![i, initWithTitle: nsstring(&it.title) action: action keyEquivalent: nsstring(key)];
        if it.submenu.is_empty() {
            let _: () = msg_send![i, setTarget: target];
            let _: () = msg_send![i, setTag: it.tag];
        } else {
            let _: () = msg_send![i, setSubmenu: menu(&it.title, &it.submenu, target)];
        }
        if !key.is_empty() {
            // NSEventModifierFlag Shift / Control / Option / Command.
            let mask: usize = (it.shift as usize) << 17 | (it.ctrl as usize) << 18 | (it.alt as usize) << 19 | (it.cmd as usize) << 20;
            let _: () = msg_send![i, setKeyEquivalentModifierMask: mask];
        }
        let _: () = msg_send![i, setEnabled: true];
        i
    }
    unsafe fn plain(title: &str, action: Sel, key: &str, mask: usize) -> *mut Object {
        let i: *mut Object = msg_send![class!(NSMenuItem), alloc];
        let i: *mut Object = msg_send![i, initWithTitle: nsstring(title) action: action keyEquivalent: nsstring(key)];
        let _: () = msg_send![i, setKeyEquivalentModifierMask: mask];
        i
    }

    unsafe {
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let target: *mut Object = msg_send![target_class(), new];
        let bar: *mut Object = msg_send![class!(NSMenu), alloc];
        let bar: *mut Object = msg_send![bar, initWithTitle: nsstring("")];
        // The app menu.
        let appm = menu(app_name, app_items, target);
        let _: () = msg_send![appm, addItem: item(&Item::separator(), target)];
        let _: () = msg_send![appm, addItem: plain(&format!("Hide {app_name}"), sel!(hide:), "h", 1 << 20)];
        let _: () = msg_send![appm, addItem: plain("Hide Others", sel!(hideOtherApplications:), "h", 1 << 20 | 1 << 19)];
        let _: () = msg_send![appm, addItem: plain("Show All", sel!(unhideAllApplications:), "", 0)];
        let _: () = msg_send![appm, addItem: item(&Item::separator(), target)];
        let _: () = msg_send![appm, addItem: plain(&format!("Quit {app_name}"), sel!(terminate:), "q", 1 << 20)];
        let holder: *mut Object = msg_send![class!(NSMenuItem), new];
        let _: () = msg_send![holder, setSubmenu: appm];
        let _: () = msg_send![bar, addItem: holder];
        for (title, items) in menus {
            let holder: *mut Object = msg_send![class!(NSMenuItem), new];
            let _: () = msg_send![holder, setSubmenu: menu(title, items, target)];
            let _: () = msg_send![bar, addItem: holder];
        }
        let _: () = msg_send![app, setMainMenu: bar];
    }
}

/// Windows and Linux have no app-wide menu bar: the app's own menus in the
/// window hold the same items.
#[cfg(not(all(feature = "window", target_os = "macos")))]
pub fn install(_app_name: &str, _app_items: &[Item], _menus: &[(String, Vec<Item>)]) {}
