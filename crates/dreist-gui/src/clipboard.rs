//! The system clipboard's text for the text boxes: the pasteboard on
//! macOS, elsewhere the system's own tools (Windows `clip` and PowerShell's
//! `Get-Clipboard`; Linux `wl-copy` / `wl-paste`, `xclip` or `xsel`, the
//! first one there). Best effort: without any of them the text copied last
//! in this process is pasted.

use std::sync::Mutex;

/// The text copied last, for systems without a reachable clipboard.
static LOCAL: Mutex<String> = Mutex::new(String::new());

/// Put `text` on the clipboard.
pub fn set(text: &str) {
    if let Ok(mut l) = LOCAL.lock() {
        *l = text.to_string();
    }
    system::set(text);
}

/// The clipboard's text.
pub fn get() -> Option<String> {
    system::get().or_else(|| LOCAL.lock().ok().map(|l| l.clone())).filter(|s| !s.is_empty())
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
mod system {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    fn nsstring(s: &str) -> *mut Object {
        let c = std::ffi::CString::new(s.replace('\0', "")).unwrap_or_default();
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
    }

    pub fn set(text: &str) {
        unsafe {
            let pb: *mut Object = msg_send![class!(NSPasteboard), generalPasteboard];
            if pb.is_null() {
                return;
            }
            let _: isize = msg_send![pb, clearContents];
            let _: bool = msg_send![pb, setString: nsstring(text) forType: nsstring("public.utf8-plain-text")];
        }
    }

    pub fn get() -> Option<String> {
        unsafe {
            let pb: *mut Object = msg_send![class!(NSPasteboard), generalPasteboard];
            if pb.is_null() {
                return None;
            }
            let s: *mut Object = msg_send![pb, stringForType: nsstring("public.utf8-plain-text")];
            if s.is_null() {
                return None;
            }
            let utf8: *const std::os::raw::c_char = msg_send![s, UTF8String];
            (!utf8.is_null()).then(|| std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod system {
    use std::io::Write;
    use std::process::{Command, Stdio};

    type Tools = &'static [(&'static str, &'static [&'static str])];

    #[cfg(windows)]
    const COPY: Tools = &[("clip", &[])];
    #[cfg(windows)]
    const PASTE: Tools = &[("powershell", &["-NoProfile", "-Command", "Get-Clipboard -Raw"])];
    #[cfg(not(windows))]
    const COPY: Tools = &[("wl-copy", &[]), ("xclip", &["-selection", "clipboard"]), ("xsel", &["--clipboard", "--input"])];
    #[cfg(not(windows))]
    const PASTE: Tools = &[("wl-paste", &["--no-newline"]), ("xclip", &["-selection", "clipboard", "-o"]), ("xsel", &["--clipboard", "--output"])];

    /// The tool's command, without a console window of its own.
    fn command(tool: &str, args: &[&str]) -> Command {
        let mut c = Command::new(tool);
        c.args(args).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x0800_0000);
        }
        c
    }

    pub fn set(text: &str) {
        for (tool, args) in COPY {
            let Ok(mut c) = command(tool, args).stdin(Stdio::piped()).stdout(Stdio::null()).spawn() else { continue };
            if let Some(mut i) = c.stdin.take() {
                let _ = i.write_all(text.as_bytes());
            }
            if c.wait().is_ok_and(|s| s.success()) {
                return;
            }
        }
    }

    pub fn get() -> Option<String> {
        PASTE.iter().find_map(|(tool, args)| {
            let out = command(tool, args).stdin(Stdio::null()).output().ok().filter(|o| o.status.success())?;
            Some(String::from_utf8_lossy(&out.stdout).trim_end_matches(['\r', '\n']).to_string())
        })
    }
}
