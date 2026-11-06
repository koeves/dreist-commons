//! Drags out of the window ([re:e36b25]): the
//! files an internal drag stands for become an `NSDraggingSession` from the
//! editor's view (file URLs, the Finder icon of each file, copy operation).

/// The class of the drag's source (NSDraggingSource: copy everywhere); its
/// name is unique to this copy of the library (`objc_class`).
#[cfg(all(feature = "window", target_os = "macos"))]
#[allow(unexpected_cfgs)]
pub(crate) fn source_class() -> &'static objc::runtime::Class {
    use objc::runtime::{Class, Object, Sel};
    use objc::{class, sel, sel_impl};

    extern "C" fn mask(_: &Object, _: Sel, _: *mut Object, _: isize) -> usize {
        1
    }
    static CLASS: std::sync::OnceLock<&'static Class> = std::sync::OnceLock::new();
    CLASS.get_or_init(|| {
        let mut decl = crate::objc_class::declare(&format!("{}DragOutSource", dreist_formats::profile::get().product), class!(NSObject));
        unsafe {
            decl.add_method(sel!(draggingSession:sourceOperationMaskForDraggingContext:), mask as extern "C" fn(&Object, Sel, *mut Object, isize) -> usize);
        }
        decl.register()
    })
}

/// Start an OS drag of `files` from `ns_view` at the current event; false
/// when it could not start.
#[cfg(all(feature = "window", target_os = "macos"))]
#[allow(unexpected_cfgs)]
pub fn begin(ns_view: *mut std::ffi::c_void, files: &[std::path::PathBuf]) -> bool {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

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

    fn nsstring(s: &str) -> *mut Object {
        let c = std::ffi::CString::new(s).unwrap_or_default();
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
    }

    if ns_view.is_null() || files.is_empty() {
        return false;
    }
    unsafe {
        let view = ns_view as *mut Object;
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let event: *mut Object = msg_send![app, currentEvent];
        if event.is_null() {
            return false;
        }
        let window: *mut Object = msg_send![view, window];
        let loc: NSPoint = msg_send![event, locationInWindow];
        let at: NSPoint = if window.is_null() { loc } else { msg_send![view, convertPoint: loc fromView: std::ptr::null_mut::<Object>()] };
        let items: *mut Object = msg_send![class!(NSMutableArray), array];
        let ws: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        for (i, f) in files.iter().enumerate() {
            let path = nsstring(&f.to_string_lossy());
            let url: *mut Object = msg_send![class!(NSURL), fileURLWithPath: path];
            let item: *mut Object = msg_send![class!(NSDraggingItem), alloc];
            let item: *mut Object = msg_send![item, initWithPasteboardWriter: url];
            let icon: *mut Object = msg_send![ws, iconForFile: path];
            let frame = NSRect { x: at.x - 16.0 + i as f64 * 4.0, y: at.y - 16.0 - i as f64 * 4.0, w: 32.0, h: 32.0 };
            let _: () = msg_send![item, setDraggingFrame: frame contents: icon];
            let _: () = msg_send![items, addObject: item];
            let _: () = msg_send![item, release];
        }
        let source: *mut Object = msg_send![source_class(), new];
        let session: *mut Object = msg_send![view, beginDraggingSessionWithItems: items event: event source: source];
        !session.is_null()
    }
}

#[cfg(not(all(feature = "window", target_os = "macos")))]
pub fn begin(_ns_view: *mut std::ffi::c_void, _files: &[std::path::PathBuf]) -> bool {
    false
}
