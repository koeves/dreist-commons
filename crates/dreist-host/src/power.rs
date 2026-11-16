//! Keep the CPU at full speed while the standalone app plays (Linux): with
//! power saving the cores clock down between audio callbacks and a heavy
//! block can start at a third of the speed, long enough to drop out. While
//! playback runs, a watcher thread holds power-profiles-daemon's
//! "performance" profile (D-Bus `HoldProfile`, libdbus loaded at run time)
//! and releases it when playback stops; the daemon also drops the hold if
//! the app quits or dies, since the hold lives with the D-Bus connection.
//!
//! The audio thread only reports playback ([`set_playing`], an atomic); the
//! app starts the watcher ([`start`]) and the editor turns it on or off
//! ([`set_enabled`]). Elsewhere (macOS, Windows, plugins) nothing happens.

use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

static PLAYING: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(true);

/// The audio thread: whether the transport plays.
pub fn set_playing(playing: bool) {
    PLAYING.store(playing, Relaxed);
}

/// Whether the profile is held while playing (on by default).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Relaxed)
}

/// What a hold or release did, for the app's log.
pub type Note = Box<dyn Fn(&str) + Send>;

/// Start the watcher for `app_id` (Linux; elsewhere nothing). `note`
/// receives one line per hold, release or failure.
pub fn start(app_id: &str, note: Note) {
    #[cfg(target_os = "linux")]
    {
        let app = app_id.to_string();
        let _ = std::thread::Builder::new().name("power-hold".into()).spawn(move || watch(&app, note));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (app_id, note);
}

#[cfg(target_os = "linux")]
fn watch(app: &str, note: Note) {
    let mut held: Option<dbus::Hold> = None;
    let mut unavailable = false;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let want = ENABLED.load(Relaxed) && PLAYING.load(Relaxed);
        match (&held, want) {
            (None, true) if !unavailable => match dbus::Hold::performance(app) {
                Ok(h) => {
                    note(&format!("power: holding the performance profile while playing (cookie {})", h.cookie));
                    held = Some(h);
                }
                Err(e) => {
                    // Once: a system without the daemon stays without it.
                    note(&format!("power: cannot hold the performance profile ({e}); for steadier audio, set Power Mode to Performance"));
                    unavailable = true;
                }
            },
            (Some(_), false) => {
                if let Some(h) = held.take() {
                    let cookie = h.cookie;
                    match h.release() {
                        Ok(()) => note(&format!("power: released the performance profile (cookie {cookie})")),
                        Err(e) => note(&format!("power: release failed ({e}); the daemon drops it when the app quits")),
                    }
                }
            }
            _ => {}
        }
    }
}

/// power-profiles-daemon over libdbus (`libdbus-1.so.3`, loaded at run
/// time: no build dependency).
#[cfg(target_os = "linux")]
mod dbus {
    use std::ffi::{CStr, c_char, c_int, c_void};

    #[repr(C)]
    struct DBusError {
        name: *const c_char,
        message: *const c_char,
        dummy: u32,
        padding: *const c_void,
    }

    type Conn = c_void;
    type Msg = c_void;
    const DBUS_BUS_SYSTEM: c_int = 1;
    const TYPE_STRING: c_int = b's' as c_int;
    const TYPE_UINT32: c_int = b'u' as c_int;
    const TYPE_INVALID: c_int = 0;

    /// The daemon's names, newest first.
    const SERVICES: [(&CStr, &CStr, &CStr); 2] = [
        (c"org.freedesktop.UPower.PowerProfiles", c"/org/freedesktop/UPower/PowerProfiles", c"org.freedesktop.UPower.PowerProfiles"),
        (c"net.hadess.PowerProfiles", c"/net/hadess/PowerProfiles", c"net.hadess.PowerProfiles"),
    ];

    struct Api {
        error_init: unsafe extern "C" fn(*mut DBusError),
        error_free: unsafe extern "C" fn(*mut DBusError),
        is_set: unsafe extern "C" fn(*const DBusError) -> u32,
        bus_get: unsafe extern "C" fn(c_int, *mut DBusError) -> *mut Conn,
        close: unsafe extern "C" fn(*mut Conn),
        conn_unref: unsafe extern "C" fn(*mut Conn),
        new_call: unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, *const c_char) -> *mut Msg,
        append: unsafe extern "C" fn(*mut Msg, c_int, ...) -> u32,
        send: unsafe extern "C" fn(*mut Conn, *mut Msg, c_int, *mut DBusError) -> *mut Msg,
        get_args: unsafe extern "C" fn(*mut Msg, *mut DBusError, c_int, ...) -> u32,
        msg_unref: unsafe extern "C" fn(*mut Msg),
    }

    fn api() -> Result<Api, String> {
        // SAFETY: the symbols are libdbus's documented C API with these
        // signatures; the library stays loaded for the process.
        unsafe {
            let lib = libc::dlopen(c"libdbus-1.so.3".as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
            if lib.is_null() {
                return Err("libdbus-1 not found".into());
            }
            macro_rules! sym {
                ($name:literal, $ty:ty) => {{
                    let p = libc::dlsym(lib, concat!($name, "\0").as_ptr() as *const c_char);
                    if p.is_null() {
                        return Err(concat!("libdbus-1 has no ", $name).into());
                    }
                    std::mem::transmute::<*mut c_void, $ty>(p)
                }};
            }
            Ok(Api {
                error_init: sym!("dbus_error_init", unsafe extern "C" fn(*mut DBusError)),
                error_free: sym!("dbus_error_free", unsafe extern "C" fn(*mut DBusError)),
                is_set: sym!("dbus_error_is_set", unsafe extern "C" fn(*const DBusError) -> u32),
                bus_get: sym!("dbus_bus_get_private", unsafe extern "C" fn(c_int, *mut DBusError) -> *mut Conn),
                close: sym!("dbus_connection_close", unsafe extern "C" fn(*mut Conn)),
                conn_unref: sym!("dbus_connection_unref", unsafe extern "C" fn(*mut Conn)),
                new_call: sym!("dbus_message_new_method_call", unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, *const c_char) -> *mut Msg),
                append: sym!("dbus_message_append_args", unsafe extern "C" fn(*mut Msg, c_int, ...) -> u32),
                send: sym!("dbus_connection_send_with_reply_and_block", unsafe extern "C" fn(*mut Conn, *mut Msg, c_int, *mut DBusError) -> *mut Msg),
                get_args: sym!("dbus_message_get_args", unsafe extern "C" fn(*mut Msg, *mut DBusError, c_int, ...) -> u32),
                msg_unref: sym!("dbus_message_unref", unsafe extern "C" fn(*mut Msg)),
            })
        }
    }

    /// A held profile: the private system-bus connection it lives with.
    pub struct Hold {
        api: Api,
        conn: *mut Conn,
        service: usize,
        pub cookie: u32,
    }

    // SAFETY: the connection is only used by the watcher thread that owns it.
    unsafe impl Send for Hold {}

    impl Hold {
        /// Hold "performance" for `app` (reason "Audio playback").
        pub fn performance(app: &str) -> Result<Hold, String> {
            let api = api()?;
            let app = std::ffi::CString::new(app).map_err(|_| "bad app id".to_string())?;
            // SAFETY: libdbus calls on objects created and freed here; the
            // connection is kept for the hold and closed in `release`/drop.
            unsafe {
                let mut err: DBusError = std::mem::zeroed();
                (api.error_init)(&mut err);
                let conn = (api.bus_get)(DBUS_BUS_SYSTEM, &mut err);
                if conn.is_null() || (api.is_set)(&err) != 0 {
                    let m = message(&err);
                    (api.error_free)(&mut err);
                    return Err(format!("no system bus: {m}"));
                }
                let mut last = String::from("no power-profiles-daemon");
                for (k, (dest, path, iface)) in SERVICES.iter().enumerate() {
                    let msg = (api.new_call)(dest.as_ptr(), path.as_ptr(), iface.as_ptr(), c"HoldProfile".as_ptr());
                    if msg.is_null() {
                        continue;
                    }
                    let (profile, reason, id) = (c"performance".as_ptr(), c"Audio playback".as_ptr(), app.as_ptr());
                    (api.append)(msg, TYPE_STRING, &profile as *const *const c_char, TYPE_STRING, &reason as *const *const c_char, TYPE_STRING, &id as *const *const c_char, TYPE_INVALID);
                    let reply = (api.send)(conn, msg, 2000, &mut err);
                    (api.msg_unref)(msg);
                    if reply.is_null() {
                        last = message(&err);
                        (api.error_free)(&mut err);
                        (api.error_init)(&mut err);
                        continue;
                    }
                    let mut cookie = 0u32;
                    let ok = (api.get_args)(reply, &mut err, TYPE_UINT32, &mut cookie as *mut u32, TYPE_INVALID);
                    (api.msg_unref)(reply);
                    if ok == 0 {
                        last = message(&err);
                        (api.error_free)(&mut err);
                        (api.error_init)(&mut err);
                        continue;
                    }
                    return Ok(Hold { api, conn, service: k, cookie });
                }
                (api.close)(conn);
                (api.conn_unref)(conn);
                Err(last)
            }
        }

        /// Release the hold and close its connection.
        pub fn release(self) -> Result<(), String> {
            let (dest, path, iface) = SERVICES[self.service];
            // SAFETY: as in `performance`; `self` is consumed, so its drop
            // closes the connection afterwards.
            unsafe {
                let api = &self.api;
                let mut err: DBusError = std::mem::zeroed();
                (api.error_init)(&mut err);
                let msg = (api.new_call)(dest.as_ptr(), path.as_ptr(), iface.as_ptr(), c"ReleaseProfile".as_ptr());
                if msg.is_null() {
                    return Err("out of memory".into());
                }
                let cookie = self.cookie;
                (api.append)(msg, TYPE_UINT32, &cookie as *const u32, TYPE_INVALID);
                let reply = (api.send)(self.conn, msg, 2000, &mut err);
                (api.msg_unref)(msg);
                if reply.is_null() {
                    let m = message(&err);
                    (api.error_free)(&mut err);
                    return Err(m);
                }
                (api.msg_unref)(reply);
                Ok(())
            }
        }
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            // SAFETY: the connection was opened by `performance` and is
            // closed exactly once here.
            unsafe {
                (self.api.close)(self.conn);
                (self.api.conn_unref)(self.conn);
            }
        }
    }

    unsafe fn message(err: &DBusError) -> String {
        if err.message.is_null() {
            String::from("unknown error")
        } else {
            // SAFETY: libdbus sets a NUL-terminated message with the error.
            unsafe { CStr::from_ptr(err.message) }.to_string_lossy().into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn playback_and_the_setting_are_plain_flags() {
        super::set_playing(true);
        assert!(super::enabled());
        super::set_enabled(false);
        assert!(!super::enabled());
        super::set_enabled(true);
        super::set_playing(false);
    }
}
