//! The desktop's file manager and clipboard, through the system's own
//! tools: show files in the file manager ([`reveal`]) and put text on the
//! clipboard ([`copy_text`]). Both are best effort and fail quietly (no
//! tool, no desktop session).

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Show `paths` selected in the file manager (macOS: Finder, `open -R`;
/// Windows: Explorer, the first path; Linux: the desktop's
/// `org.freedesktop.FileManager1` service, else the first path's folder
/// through `xdg-open`). Returns at once.
pub fn reveal<P: AsRef<Path>>(paths: &[P]) {
    let Some(first) = paths.first().map(AsRef::as_ref) else { return };
    if cfg!(target_os = "macos") {
        let _ = Command::new("open").arg("-R").args(paths.iter().map(AsRef::as_ref)).spawn();
    } else if cfg!(windows) {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(first);
        let _ = Command::new("explorer").arg(arg).spawn();
    } else {
        let uris: Vec<String> = paths.iter().map(|p| file_uri(p.as_ref())).collect();
        let shown = Command::new("dbus-send")
            .args(["--session", "--print-reply", "--dest=org.freedesktop.FileManager1", "--type=method_call", "/org/freedesktop/FileManager1", "org.freedesktop.FileManager1.ShowItems"])
            .arg(format!("array:string:{}", uris.join(",")))
            .arg("string:")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !shown {
            let dir = if first.is_dir() { first } else { first.parent().unwrap_or(first) };
            let _ = Command::new("xdg-open").arg(dir).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        }
    }
}

/// Put `text` on the clipboard (macOS `pbcopy`, Windows `clip`, Linux
/// `wl-copy`, `xclip` or `xsel`, the first one there).
pub fn copy_text(text: &str) {
    let tools: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(windows) {
        &[("clip", &[])]
    } else {
        &[("wl-copy", &[]), ("xclip", &["-selection", "clipboard"]), ("xsel", &["--clipboard", "--input"])]
    };
    for (tool, args) in tools {
        let Ok(mut c) = Command::new(tool).args(*args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { continue };
        if let Some(mut i) = c.stdin.take() {
            let _ = i.write_all(text.as_bytes());
        }
        if c.wait().is_ok_and(|s| s.success()) {
            return;
        }
    }
}

/// `file://` URI of an absolute path (bytes outside the unreserved set
/// and `/` percent-encoded).
fn file_uri(p: &Path) -> String {
    let mut s = String::from("file://");
    for b in p.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_uris_escape_reserved_bytes() {
        assert_eq!(super::file_uri(std::path::Path::new("/a b/ü,x.wav")), "file:///a%20b/%C3%BC%2Cx.wav");
    }
}
