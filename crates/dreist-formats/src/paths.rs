//! Where the app's resources live, by the installed profile's lookup
//! (`profile::Resources`): its factory data (effect presets, effect chains,
//! pattern and swing templates) and its UI graphics (skins, stylesheets,
//! fonts, cursors, metronomes).

use std::path::PathBuf;

/// The factory data folder (`Effects/`, `Effect Chains/`, `Pattern
/// Templates/`, `Swing Templates/`).
pub fn resources_dir() -> PathBuf {
    crate::profile::get().factory.dir()
}

/// `Resources/` inside [`resources_dir`]: callers take its parent for the
/// factory data.
pub fn skin_dir() -> PathBuf {
    resources_dir().join("Resources")
}

/// The UI graphics folder (`Skin/`, `Fonts/`, [re:038bdf], `Metronomes/`).
pub fn ui_dir() -> PathBuf {
    crate::profile::get().ui.dir()
}

/// The user's home folder (`HOME`; `USERPROFILE` on Windows).
pub fn home() -> PathBuf {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default()
}

/// `<bundle>/Contents/Resources/<name>` of the bundle holding this image.
pub fn bundled_dir(name: &str) -> Option<PathBuf> {
    let image = own_image()?;
    // <bundle>/Contents/MacOS/<binary>
    let bundle = image.parent()?.parent().map(|c| c.join("Resources").join(name));
    // Windows `.exe` and `.clap` are bare files: `Resources/` sits next to them.
    let beside = image.parent().map(|d| d.join("Resources").join(name));
    bundle.into_iter().chain(beside).find(|p| p.is_dir())
}

#[cfg(unix)]
fn own_image() -> Option<PathBuf> {
    #[repr(C)]
    struct DlInfo {
        dli_fname: *const std::os::raw::c_char,
        dli_fbase: *mut std::ffi::c_void,
        dli_sname: *const std::os::raw::c_char,
        dli_saddr: *mut std::ffi::c_void,
    }
    unsafe extern "C" {
        fn dladdr(addr: *const std::ffi::c_void, info: *mut DlInfo) -> std::os::raw::c_int;
    }
    let mut info = DlInfo { dli_fname: std::ptr::null(), dli_fbase: std::ptr::null_mut(), dli_sname: std::ptr::null(), dli_saddr: std::ptr::null_mut() };
    let addr = own_image as *const std::ffi::c_void;
    // SAFETY: dladdr only reads the address and fills `info`.
    let ok = unsafe { dladdr(addr, &mut info) };
    if ok == 0 || info.dli_fname.is_null() {
        return std::env::current_exe().ok();
    }
    // SAFETY: dli_fname is a NUL-terminated path owned by the loader.
    let s = unsafe { std::ffi::CStr::from_ptr(info.dli_fname) };
    Some(PathBuf::from(s.to_string_lossy().into_owned()))
}

#[cfg(windows)]
fn own_image() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    const FROM_ADDRESS: u32 = 0x4;
    const UNCHANGED_REFCOUNT: u32 = 0x2;
    unsafe extern "system" {
        fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut *mut std::ffi::c_void) -> i32;
        fn GetModuleFileNameW(module: *mut std::ffi::c_void, name: *mut u16, size: u32) -> u32;
    }
    let mut module = std::ptr::null_mut();
    let addr = own_image as *const u16;
    // SAFETY: looks up the module containing this function; no refcount taken.
    if unsafe { GetModuleHandleExW(FROM_ADDRESS | UNCHANGED_REFCOUNT, addr, &mut module) } == 0 {
        return std::env::current_exe().ok();
    }
    let mut buf = vec![0u16; 32768];
    // SAFETY: `buf` holds `buf.len()` UTF-16 units.
    let n = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n >= buf.len() {
        return std::env::current_exe().ok();
    }
    Some(PathBuf::from(std::ffi::OsString::from_wide(&buf[..n])))
}

#[cfg(not(any(unix, windows)))]
fn own_image() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

#[cfg(test)]
mod tests {
    /// The reference profile finds a Dreist checkout's UI (skipped without one).
    #[test]
    fn ui_dir_holds_the_skins_icons_and_cursors() {
        let ui = super::ui_dir();
        if !ui.is_dir() {
            return;
        }
        assert!(ui.join("Skin").join("HeaderBar.skin").is_file());
        assert!(ui.join("Fonts").join(format!("{}.ttf", crate::profile::get().icon_family)).is_file());
        assert!(ui.join("Cursors").join("Cursors.hotspots").is_file());
    }
}
