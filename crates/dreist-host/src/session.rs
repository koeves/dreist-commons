//! The session file a relaunched standalone starts from (nih-plug opens
//! its audio devices once, at start-up, so an audio restart relaunches the
//! app with the current document).

/// Hand `session` (as JSON, in `<temp>/<stem>-<pid>.json`) to a new
/// instance of this executable through environment variable `env`, and
/// exit once it started. Returns when that fails.
pub fn relaunch<T: serde::Serialize>(env: &str, stem: &str, session: &T) {
    let file = std::env::temp_dir().join(format!("{stem}-{}.json", std::process::id()));
    let Ok(json) = serde_json::to_string(session) else { return };
    if std::fs::write(&file, json).is_err() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    if std::process::Command::new(exe).env(env, &file).spawn().is_ok() {
        std::process::exit(0);
    }
}

/// The session a relaunch handed over through `env` (read once, then the
/// file is removed).
pub fn take<T: serde::de::DeserializeOwned>(env: &str) -> Option<T> {
    let file = std::env::var_os(env)?;
    let s = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    serde_json::from_str(&s).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_session_file_is_read_once() {
        let env = "DREIST_HOST_TEST_SESSION";
        let file = std::env::temp_dir().join(format!("dreist-host-session-{}.json", std::process::id()));
        std::fs::write(&file, serde_json::to_string(&(1u32, "doc")).unwrap()).unwrap();
        // SAFETY: no other thread of this test binary reads this variable.
        unsafe { std::env::set_var(env, &file) };
        assert_eq!(super::take::<(u32, String)>(env), Some((1, "doc".to_string())));
        assert!(!file.exists());
        assert_eq!(super::take::<(u32, String)>(env), None);
    }
}
