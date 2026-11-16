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

/// The folder that holds the apps' settings folders: `~/Library/Application
/// Support` on macOS, `%APPDATA%` on Windows and the XDG config folder
/// elsewhere (`$XDG_CONFIG_HOME`, else `~/.config`).
pub fn settings_base() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        xdg_dir("XDG_CONFIG_HOME", ".config")
    }
}

/// An XDG base folder: `var` when it holds an absolute path (the
/// specification ignores relative ones), else `fallback` in the home folder.
pub fn xdg_dir(var: &str, fallback: &str) -> Option<PathBuf> {
    match std::env::var_os(var).map(PathBuf::from) {
        Some(p) if p.is_absolute() => Some(p),
        _ => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(fallback)),
    }
}

/// `<bundle>/Contents/Resources/<name>` of the bundle holding this image.
pub fn bundled_dir(name: &str) -> Option<PathBuf> {
    let image = own_image()?;
    // <bundle>/Contents/MacOS/<binary>
    let bundle = image.parent()?.parent().map(|c| c.join("Resources").join(name));
    // Windows and Linux `.exe` / app binaries and `.clap` files are bare
    // files: `Resources/` sits next to them. (A Linux VST3 is
    // `<bundle>/Contents/x86_64-linux/<name>.so`, which the first form finds.)
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
    let p = PathBuf::from(s.to_string_lossy().into_owned());
    // glibc names the main program as it was started (argv[0]: relative,
    // bare, or a symlink such as `~/.local/bin/<app>`): the executable's
    // real path then, so `Resources/` beside it is found.
    #[cfg(not(target_os = "macos"))]
    let p = if p.is_absolute() { std::fs::canonicalize(&p).unwrap_or(p) } else { std::env::current_exe().ok().map(|e| std::fs::canonicalize(&e).unwrap_or(e))? };
    Some(p)
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
    /// XDG folders: absolute values only, else the home fallback.
    #[test]
    fn xdg_dirs_take_absolute_values_only() {
        // A variable no other test reads.
        let var = "DREIST_TEST_XDG_DIR";
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        // SAFETY: only this test touches the variable.
        unsafe { std::env::set_var(var, "/tmp/xdg") };
        assert_eq!(super::xdg_dir(var, ".config"), Some(std::path::PathBuf::from("/tmp/xdg")));
        unsafe { std::env::set_var(var, "relative") };
        assert_eq!(super::xdg_dir(var, ".config"), home.as_ref().map(|h| h.join(".config")));
        unsafe { std::env::remove_var(var) };
        assert_eq!(super::xdg_dir(var, ".config"), home.map(|h| h.join(".config")));
    }

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
