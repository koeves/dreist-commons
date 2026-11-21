//! Drags out of the window ([re:e36b25]): the
//! files an internal drag stands for become an OS drag from the editor's
//! view, copied where they land.
//!
//! - macOS: an `NSDraggingSession` (file URLs, the Finder icon of each file).
//! - Windows: an OLE drag (`DoDragDrop`) of a data object offering the files
//!   as `CF_HDROP`.
//! - Linux (X11): the drag stays inside the window.

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

/// The `CF_HDROP` block for `files`: a `DROPFILES` header (20 bytes, wide
/// names) and the paths as NUL-terminated UTF-16, ending with an empty one.
pub fn hdrop_bytes(files: &[std::path::PathBuf]) -> Vec<u8> {
    const HEADER: u32 = 20;
    let mut b = Vec::new();
    // pFiles, pt.x, pt.y, fNC, fWide.
    for v in [HEADER, 0, 0, 0, 1] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    for f in files {
        for u in f.as_os_str().to_string_lossy().encode_utf16().chain([0]) {
            b.extend_from_slice(&u.to_le_bytes());
        }
    }
    b.extend_from_slice(&0u16.to_le_bytes());
    b
}

/// Start an OLE drag of `files` from the window `hwnd`; it runs until the
/// drop or Escape (`DoDragDrop` is modal and takes the button's release).
/// False when it could not start.
#[cfg(all(feature = "window", windows))]
pub fn begin(hwnd: *mut std::ffi::c_void, files: &[std::path::PathBuf]) -> bool {
    if hwnd.is_null() || files.is_empty() {
        return false;
    }
    win::drag(&hdrop_bytes(files))
}

/// The OLE side of a drag out: a data object holding one `CF_HDROP` block
/// and a drop source that drops on the button's release, both minimal COM
/// objects (raw vtables; winapi lacks these declarations).
#[cfg(all(feature = "window", windows))]
mod win {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};

    type Hresult = i32;
    const S_OK: Hresult = 0;
    const S_FALSE: Hresult = 1;
    const E_NOINTERFACE: Hresult = 0x8000_4002u32 as i32;
    const E_NOTIMPL: Hresult = 0x8000_4001u32 as i32;
    const E_POINTER: Hresult = 0x8000_4003u32 as i32;
    const E_OUTOFMEMORY: Hresult = 0x8007_000Eu32 as i32;
    const DV_E_FORMATETC: Hresult = 0x8004_0064u32 as i32;
    const OLE_E_ADVISENOTSUPPORTED: Hresult = 0x8004_0003u32 as i32;
    const DRAGDROP_S_DROP: Hresult = 0x0004_0100;
    const DRAGDROP_S_CANCEL: Hresult = 0x0004_0101;
    const DRAGDROP_S_USEDEFAULTCURSORS: Hresult = 0x0004_0102;
    const CF_HDROP: u16 = 15;
    const TYMED_HGLOBAL: u32 = 1;
    const DVASPECT_CONTENT: u32 = 1;
    const DATADIR_GET: u32 = 1;
    const DROPEFFECT_COPY: u32 = 1;
    const MK_LBUTTON: u32 = 1;
    const GMEM_MOVEABLE: u32 = 0x2;

    #[repr(C)]
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct Guid(u32, u16, u16, [u8; 8]);
    const IID_IUNKNOWN: Guid = Guid(0, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
    const IID_IDATAOBJECT: Guid = Guid(0x0000_010E, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
    const IID_IDROPSOURCE: Guid = Guid(0x0000_0121, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct FormatEtc {
        pub cf_format: u16,
        pub ptd: *mut c_void,
        pub dw_aspect: u32,
        pub lindex: i32,
        pub tymed: u32,
    }

    #[repr(C)]
    pub struct StgMedium {
        pub tymed: u32,
        pub h_global: *mut c_void,
        pub unk_for_release: *mut c_void,
    }

    const HDROP_FORMAT: FormatEtc = FormatEtc { cf_format: CF_HDROP, ptd: std::ptr::null_mut(), dw_aspect: DVASPECT_CONTENT, lindex: -1, tymed: TYMED_HGLOBAL };

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn OleInitialize(reserved: *mut c_void) -> Hresult;
        fn OleUninitialize();
        fn DoDragDrop(data: *mut c_void, source: *mut c_void, ok_effects: u32, effect: *mut u32) -> Hresult;
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHCreateStdEnumFmtEtc(n: u32, formats: *const FormatEtc, out: *mut *mut c_void) -> Hresult;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
        fn GlobalLock(h: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(h: *mut c_void) -> i32;
    }

    /// A moveable global block holding `bytes` (the caller owns it).
    fn global(bytes: &[u8]) -> *mut c_void {
        // SAFETY: a fresh block of `bytes.len()` bytes, written while locked.
        unsafe {
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if h.is_null() {
                return h;
            }
            let p = GlobalLock(h) as *mut u8;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            }
            GlobalUnlock(h);
            h
        }
    }

    #[repr(C)]
    struct DataObjectVtbl {
        query_interface: unsafe extern "system" fn(*mut DataObject, *const Guid, *mut *mut c_void) -> Hresult,
        add_ref: unsafe extern "system" fn(*mut DataObject) -> u32,
        release: unsafe extern "system" fn(*mut DataObject) -> u32,
        get_data: unsafe extern "system" fn(*mut DataObject, *const FormatEtc, *mut StgMedium) -> Hresult,
        get_data_here: unsafe extern "system" fn(*mut DataObject, *const FormatEtc, *mut StgMedium) -> Hresult,
        query_get_data: unsafe extern "system" fn(*mut DataObject, *const FormatEtc) -> Hresult,
        get_canonical_format_etc: unsafe extern "system" fn(*mut DataObject, *const FormatEtc, *mut FormatEtc) -> Hresult,
        set_data: unsafe extern "system" fn(*mut DataObject, *const FormatEtc, *mut StgMedium, i32) -> Hresult,
        enum_format_etc: unsafe extern "system" fn(*mut DataObject, u32, *mut *mut c_void) -> Hresult,
        d_advise: unsafe extern "system" fn(*mut DataObject, *const FormatEtc, u32, *mut c_void, *mut u32) -> Hresult,
        d_unadvise: unsafe extern "system" fn(*mut DataObject, u32) -> Hresult,
        enum_d_advise: unsafe extern "system" fn(*mut DataObject, *mut *mut c_void) -> Hresult,
    }

    /// The files of the drag, as a `CF_HDROP` block; each `GetData` hands
    /// out a fresh copy for the receiver to free.
    #[repr(C)]
    pub struct DataObject {
        vtbl: *const DataObjectVtbl,
        refs: AtomicU32,
        hdrop: Vec<u8>,
    }

    fn is_hdrop(f: &FormatEtc) -> bool {
        f.cf_format == CF_HDROP && f.dw_aspect == DVASPECT_CONTENT && f.tymed & TYMED_HGLOBAL != 0
    }

    unsafe extern "system" fn data_qi(this: *mut DataObject, iid: *const Guid, out: *mut *mut c_void) -> Hresult {
        if out.is_null() || iid.is_null() {
            return E_POINTER;
        }
        // SAFETY: COM hands valid pointers; `this` lives while referenced.
        unsafe {
            if *iid == IID_IUNKNOWN || *iid == IID_IDATAOBJECT {
                data_add_ref(this);
                *out = this.cast();
                S_OK
            } else {
                *out = std::ptr::null_mut();
                E_NOINTERFACE
            }
        }
    }
    unsafe extern "system" fn data_add_ref(this: *mut DataObject) -> u32 {
        // SAFETY: `this` is a live object.
        unsafe { (*this).refs.fetch_add(1, Ordering::AcqRel) + 1 }
    }
    unsafe extern "system" fn data_release(this: *mut DataObject) -> u32 {
        // SAFETY: the last reference frees the box made in `DataObject::new`.
        unsafe {
            let left = (*this).refs.fetch_sub(1, Ordering::AcqRel) - 1;
            if left == 0 {
                drop(Box::from_raw(this));
            }
            left
        }
    }
    unsafe extern "system" fn data_get(this: *mut DataObject, f: *const FormatEtc, m: *mut StgMedium) -> Hresult {
        if f.is_null() || m.is_null() {
            return E_POINTER;
        }
        // SAFETY: valid pointers from the caller.
        unsafe {
            if !is_hdrop(&*f) {
                return DV_E_FORMATETC;
            }
            let h = global(&(*this).hdrop);
            if h.is_null() {
                return E_OUTOFMEMORY;
            }
            *m = StgMedium { tymed: TYMED_HGLOBAL, h_global: h, unk_for_release: std::ptr::null_mut() };
        }
        S_OK
    }
    unsafe extern "system" fn data_get_here(_: *mut DataObject, _: *const FormatEtc, _: *mut StgMedium) -> Hresult {
        E_NOTIMPL
    }
    unsafe extern "system" fn data_query(_: *mut DataObject, f: *const FormatEtc) -> Hresult {
        if f.is_null() {
            return E_POINTER;
        }
        // SAFETY: a valid pointer from the caller.
        if is_hdrop(unsafe { &*f }) { S_OK } else { DV_E_FORMATETC }
    }
    unsafe extern "system" fn data_canonical(_: *mut DataObject, _: *const FormatEtc, out: *mut FormatEtc) -> Hresult {
        if !out.is_null() {
            // SAFETY: a valid pointer from the caller.
            unsafe { (*out).ptd = std::ptr::null_mut() };
        }
        E_NOTIMPL
    }
    unsafe extern "system" fn data_set(_: *mut DataObject, _: *const FormatEtc, _: *mut StgMedium, _: i32) -> Hresult {
        E_NOTIMPL
    }
    unsafe extern "system" fn data_enum(_: *mut DataObject, direction: u32, out: *mut *mut c_void) -> Hresult {
        if out.is_null() {
            return E_POINTER;
        }
        if direction != DATADIR_GET {
            // SAFETY: a valid pointer from the caller.
            unsafe { *out = std::ptr::null_mut() };
            return E_NOTIMPL;
        }
        // SAFETY: one format, copied by the shell's enumerator.
        unsafe { SHCreateStdEnumFmtEtc(1, &HDROP_FORMAT, out) }
    }
    unsafe extern "system" fn data_advise(_: *mut DataObject, _: *const FormatEtc, _: u32, _: *mut c_void, _: *mut u32) -> Hresult {
        OLE_E_ADVISENOTSUPPORTED
    }
    unsafe extern "system" fn data_unadvise(_: *mut DataObject, _: u32) -> Hresult {
        OLE_E_ADVISENOTSUPPORTED
    }
    unsafe extern "system" fn data_enum_advise(_: *mut DataObject, _: *mut *mut c_void) -> Hresult {
        OLE_E_ADVISENOTSUPPORTED
    }

    static DATA_VTBL: DataObjectVtbl = DataObjectVtbl {
        query_interface: data_qi,
        add_ref: data_add_ref,
        release: data_release,
        get_data: data_get,
        get_data_here: data_get_here,
        query_get_data: data_query,
        get_canonical_format_etc: data_canonical,
        set_data: data_set,
        enum_format_etc: data_enum,
        d_advise: data_advise,
        d_unadvise: data_unadvise,
        enum_d_advise: data_enum_advise,
    };

    impl DataObject {
        /// A new object with one reference.
        pub fn new(hdrop: Vec<u8>) -> *mut DataObject {
            Box::into_raw(Box::new(DataObject { vtbl: &DATA_VTBL, refs: AtomicU32::new(1), hdrop }))
        }
    }

    #[repr(C)]
    struct DropSourceVtbl {
        query_interface: unsafe extern "system" fn(*mut DropSource, *const Guid, *mut *mut c_void) -> Hresult,
        add_ref: unsafe extern "system" fn(*mut DropSource) -> u32,
        release: unsafe extern "system" fn(*mut DropSource) -> u32,
        query_continue_drag: unsafe extern "system" fn(*mut DropSource, i32, u32) -> Hresult,
        give_feedback: unsafe extern "system" fn(*mut DropSource, u32) -> Hresult,
    }

    /// Drops when the left button goes up, cancels on Escape; the system's
    /// cursors.
    #[repr(C)]
    pub struct DropSource {
        vtbl: *const DropSourceVtbl,
        refs: AtomicU32,
    }

    unsafe extern "system" fn source_qi(this: *mut DropSource, iid: *const Guid, out: *mut *mut c_void) -> Hresult {
        if out.is_null() || iid.is_null() {
            return E_POINTER;
        }
        // SAFETY: COM hands valid pointers; `this` lives while referenced.
        unsafe {
            if *iid == IID_IUNKNOWN || *iid == IID_IDROPSOURCE {
                source_add_ref(this);
                *out = this.cast();
                S_OK
            } else {
                *out = std::ptr::null_mut();
                E_NOINTERFACE
            }
        }
    }
    unsafe extern "system" fn source_add_ref(this: *mut DropSource) -> u32 {
        // SAFETY: `this` is a live object.
        unsafe { (*this).refs.fetch_add(1, Ordering::AcqRel) + 1 }
    }
    unsafe extern "system" fn source_release(this: *mut DropSource) -> u32 {
        // SAFETY: the last reference frees the box made in `DropSource::new`.
        unsafe {
            let left = (*this).refs.fetch_sub(1, Ordering::AcqRel) - 1;
            if left == 0 {
                drop(Box::from_raw(this));
            }
            left
        }
    }
    unsafe extern "system" fn source_continue(_: *mut DropSource, escape: i32, keys: u32) -> Hresult {
        continue_drag(escape != 0, keys)
    }
    unsafe extern "system" fn source_feedback(_: *mut DropSource, _: u32) -> Hresult {
        DRAGDROP_S_USEDEFAULTCURSORS
    }

    /// `QueryContinueDrag`: cancel on Escape, drop once the left button is
    /// up, else go on.
    pub fn continue_drag(escape: bool, keys: u32) -> Hresult {
        if escape {
            DRAGDROP_S_CANCEL
        } else if keys & MK_LBUTTON == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    static SOURCE_VTBL: DropSourceVtbl =
        DropSourceVtbl { query_interface: source_qi, add_ref: source_add_ref, release: source_release, query_continue_drag: source_continue, give_feedback: source_feedback };

    impl DropSource {
        /// A new object with one reference.
        pub fn new() -> *mut DropSource {
            Box::into_raw(Box::new(DropSource { vtbl: &SOURCE_VTBL, refs: AtomicU32::new(1) }))
        }
    }

    /// Run the drag of the `CF_HDROP` block `hdrop` on this (the window's)
    /// thread; true when it ran (dropped or cancelled).
    pub fn drag(hdrop: &[u8]) -> bool {
        // SAFETY: OLE on the window's thread (baseview initialised it there
        // too; this call is balanced); the objects are released below, and
        // DoDragDrop holds its own references while it runs.
        unsafe {
            let ole = OleInitialize(std::ptr::null_mut());
            if ole != S_OK && ole != S_FALSE {
                return false;
            }
            let data = DataObject::new(hdrop.to_vec());
            let source = DropSource::new();
            let mut effect = 0u32;
            let hr = DoDragDrop(data.cast(), source.cast(), DROPEFFECT_COPY, &mut effect);
            data_release(data);
            source_release(source);
            OleUninitialize();
            hr == DRAGDROP_S_DROP || hr == DRAGDROP_S_CANCEL
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_data_object_offers_the_files_as_hdrop() {
            let bytes = crate::dragout::hdrop_bytes(&[std::path::PathBuf::from("C:\\x\\a.wav")]);
            let d = DataObject::new(bytes.clone());
            // SAFETY: the object made above, used through its vtable.
            unsafe {
                let vt = &*(*d).vtbl;
                assert_eq!((vt.query_get_data)(d, &HDROP_FORMAT), S_OK);
                let text = FormatEtc { cf_format: 1, ..HDROP_FORMAT };
                assert_eq!((vt.query_get_data)(d, &text), DV_E_FORMATETC);
                let mut m = StgMedium { tymed: 0, h_global: std::ptr::null_mut(), unk_for_release: std::ptr::null_mut() };
                assert_eq!((vt.get_data)(d, &HDROP_FORMAT, &mut m), S_OK);
                assert_eq!(m.tymed, TYMED_HGLOBAL);
                let p = GlobalLock(m.h_global) as *const u8;
                assert_eq!(std::slice::from_raw_parts(p, bytes.len()), &bytes[..]);
                GlobalUnlock(m.h_global);
                let mut iface = std::ptr::null_mut();
                assert_eq!((vt.query_interface)(d, &IID_IDROPSOURCE, &mut iface), E_NOINTERFACE);
                assert_eq!((vt.query_interface)(d, &IID_IDATAOBJECT, &mut iface), S_OK);
                assert_eq!((vt.release)(d), 1);
                assert_eq!((vt.release)(d), 0);
            }
        }

        #[test]
        fn the_drop_comes_with_the_button() {
            assert_eq!(continue_drag(false, MK_LBUTTON), S_OK);
            assert_eq!(continue_drag(false, 0), DRAGDROP_S_DROP);
            assert_eq!(continue_drag(true, MK_LBUTTON), DRAGDROP_S_CANCEL);
        }
    }
}

/// Elsewhere (X11) the drag stays inside the window: the XDND protocol
/// would need its own source, so the internal drag carries on and drops
/// nothing outside.
#[cfg(not(all(feature = "window", any(target_os = "macos", windows))))]
pub fn begin(_view: *mut std::ffi::c_void, _files: &[std::path::PathBuf]) -> bool {
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_hdrop_block_lists_the_files_in_utf16() {
        let b = super::hdrop_bytes(&[std::path::PathBuf::from("a"), std::path::PathBuf::from("é")]);
        assert_eq!(&b[..4], &20u32.to_le_bytes());
        assert_eq!(&b[16..20], &1u32.to_le_bytes());
        let names: Vec<u16> = b[20..].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(names, [b'a' as u16, 0, 0xe9, 0, 0]);
    }
}
