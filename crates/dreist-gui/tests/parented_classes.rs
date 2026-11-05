//! Opening the editor view inside a parent while classes with the names
//! other copies of the toolkit (another plugin, or the other format of this
//! one) would register already exist: the view and its drawing surface
//! register their own classes instead of failing on the taken names or
//! reusing the other copy's. Runs on the main thread (`harness = false`),
//! as views need it.

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn main() {
    use dreist_gui::app::App;
    use dreist_gui::style::StyleSheet;
    use dreist_gui::window::{Hooked, open_parented};
    use dreist_gui::{Ctrl, Rect, Skin};
    use objc::declare::ClassDecl;
    use objc::runtime::{Class, Object};
    use objc::{class, msg_send, sel, sel_impl};
    use raw_window_handle_05 as rwh05;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }
    unsafe impl objc::Encode for NSRect {
        fn encode() -> objc::Encoding {
            unsafe { objc::Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
        }
    }

    struct Parent(*mut Object);
    unsafe impl rwh05::HasRawWindowHandle for Parent {
        fn raw_window_handle(&self) -> rwh05::RawWindowHandle {
            let mut h = rwh05::AppKitWindowHandle::empty();
            h.ns_view = self.0.cast();
            rwh05::RawWindowHandle::AppKit(h)
        }
    }

    // Nothing this test opens may come to the front or take focus.
    unsafe { std::env::set_var(baseview::BACKGROUND_GUI_ENV, "1") };

    // The fixed name the drawing surface's observer class used to have.
    let taken = "SoftbufferObserver";
    if Class::get(taken).is_none() {
        ClassDecl::new(taken, class!(NSObject)).expect("free name").register();
    }

    let classes = || -> Vec<String> {
        let mut n = 0u32;
        unsafe {
            let list = objc::runtime::objc_copyClassList(&mut n);
            let names = (0..n as usize).map(|i| (**list.add(i)).name().to_string()).filter(|s| s.starts_with(taken)).collect();
            libc_free(list.cast());
            names
        }
    };
    unsafe extern "C" {
        #[link_name = "free"]
        fn libc_free(p: *mut std::ffi::c_void);
    }

    unsafe {
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, setActivationPolicy: 1isize]; // accessory: no Dock icon, never active
        for round in 0..2 {
            let win: *mut Object = msg_send![class!(NSWindow), alloc];
            let frame = NSRect { x: -30000.0, y: -30000.0, w: 200.0, h: 100.0 };
            let win: *mut Object = msg_send![win, initWithContentRect: frame styleMask: 1usize backing: 2usize defer: false];
            // Off screen and never shown, so never key.
            let view: *mut Object = msg_send![win, contentView];
            let skin = Skin { dir: "/nonexistent".into(), ours: "/nonexistent".into(), theme: String::new(), styles: StyleSheet::new() };
            let root = Ctrl { class: "root".into(), rect: Rect::new(0, 0, 200, 100), visible: true, enabled: true, ..Ctrl::default() };
            let mut handle = open_parented(&Parent(view), Hooked(App::new(skin, root), Box::new(|_, _| {})));
            let subviews: *mut Object = msg_send![view, subviews];
            let count: usize = msg_send![subviews, count];
            assert_eq!(count, 1, "round {round}: the editor view is in the parent");
            handle.close();
            let _: () = msg_send![win, close];
        }
    }
    // The taken name plus this copy's own observer class, registered once.
    let names = classes();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.iter().any(|n| n.starts_with(&format!("{taken}_"))), "{names:?}");
    println!("parented_classes: ok ({names:?})");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
