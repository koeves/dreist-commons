//! Preferences. The original keeps them in the
//! [re:5cffa6] defaults domain (shared with licensing); the
//! port stores its own `key=value` file so the installed v2's settings
//! are never touched.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// Option `PrerollTime`'s values.
pub const PREROLL_NAMES: [&str; 4] = ["PrerollOff", "Preroll1Bar", "Preroll2Bars", "Preroll4Bars"];

/// [re:0c66d0]: unknown → off.
pub fn preroll_index(name: &str) -> usize {
    PREROLL_NAMES.iter().position(|n| *n == name).unwrap_or(0)
}

/// The options file (`key=value` lines). Copies are loaded and saved by
/// many panels; `save` writes only the keys this copy set, over what the
/// file holds then, so an older copy never reverts another's change.
#[derive(Clone, Debug, Default)]
pub struct Options {
    values: BTreeMap<String, String>,
    path: Option<PathBuf>,
    /// Keys set since the load / last save.
    changed: std::collections::BTreeSet<String>,
}

impl PartialEq for Options {
    fn eq(&self, o: &Options) -> bool {
        self.values == o.values && self.path == o.path
    }
}

// Unit tests never touch the user's settings: each test thread has its own
// in-memory options file.
#[cfg(any(test, feature = "test-settings"))]
thread_local! {
    static TEST_FILE: std::cell::RefCell<BTreeMap<String, String>> = Default::default();
}

fn read_file(path: &Option<PathBuf>) -> BTreeMap<String, String> {
    #[cfg(any(test, feature = "test-settings"))]
    {
        let _ = path;
        TEST_FILE.with(|f| f.borrow().clone())
    }
    #[cfg(not(any(test, feature = "test-settings")))]
    {
        // Read once per change of the file (size and modification time):
        // tooltips and panels look options up on every pointer move.
        let Some(p) = path.as_ref() else { return BTreeMap::new() };
        let stamp = std::fs::metadata(p).ok().map(|m| (m.len(), m.modified().ok()));
        let mut cache = FILE_CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((cp, cs, values)) = cache.as_ref() {
            if cp == p && *cs == stamp {
                return values.clone();
            }
        }
        let mut values = BTreeMap::new();
        if let Ok(text) = std::fs::read_to_string(p) {
            for line in text.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    values.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
        *cache = Some((p.clone(), stamp, values.clone()));
        values
    }
}

/// The options file as last read: its path, (size, modification time) and values.
#[cfg(not(any(test, feature = "test-settings")))]
type FileStamp = Option<(u64, Option<std::time::SystemTime>)>;
#[cfg(not(any(test, feature = "test-settings")))]
static FILE_CACHE: std::sync::Mutex<Option<(PathBuf, FileStamp, BTreeMap<String, String>)>> = std::sync::Mutex::new(None);

/// [re:9d78d9] [re:33133e]: option
/// `LocalDocumentsPath`, default [`dreist_formats::legacy::DOCUMENTS_DIR`] in the home folder. The user
/// Content, Effects and Effect Chains folders live under it.
pub fn documents_dir() -> PathBuf {
    PathBuf::from(Options::load().string("LocalDocumentsPath", &default_documents()))
}

/// [re:857769] [re:a5b222].
pub fn default_documents() -> String {
    dreist_formats::paths::home().join(dreist_formats::legacy::DOCUMENTS_DIR).to_string_lossy().into_owned()
}

/// Engine settings that come from the options ([re:fe3f2a] reads
/// `ForcedLayersFallThrough` when it is built).
pub fn render_options() -> dreist_engine::RenderOptions {
    render_options_from(&Options::load())
}

/// [`render_options`] from loaded options.
pub fn render_options_from(o: &Options) -> dreist_engine::RenderOptions {
    dreist_engine::RenderOptions {
        forced_layers_fall_through: o.bool("ForcedLayersFallThrough", false),
        auxes_pre_fade: o.bool("AuxesArePreFade", false),
        preroll_mode: preroll_index(&o.string("PrerollTime", "PrerollOff")) as u32,
        preroll_only_rec: o.bool("PrerollOnlyOnRecord", true),
        wait_midi: o.bool("WaitForMIDI", false),
        // `kOptionKeyMetroOnWhenRecording` is "PrerollOnlyOnRecord" too.
        metro_only_rec: o.bool("PrerollOnlyOnRecord", false),
        pad_follow: o.bool("PadSelectionFollowsMIDI", true),
        engine_follow: o.bool("EngineSelectionFollowsMIDI", false),
        predelay_ms: o.bool("PreDelayInMsec", true),
        ..Default::default()
    }
}

/// Option `MetronomeSound` (a folder of `Resources/Metronomes`).
pub fn metronome_sound() -> String {
    Options::load().string("MetronomeSound", "Default")
}

/// The app's settings folder (the profile's: `~/Library/Application
/// Support/<settings_dir>`, `%APPDATA%\\<settings_dir>`,
/// `~/.config/<settings_dir>`; `dreist_formats::paths::settings_base`). On first use a
/// folder under one of the profile's earlier names is moved there.
pub fn settings_dir() -> Option<PathBuf> {
    // Unit tests get a scratch folder instead of the user's.
    if let Some(d) = test_settings_dir() {
        return Some(d);
    }
    // A separate settings folder (tools, fuzzing).
    let app = dreist_formats::profile::get();
    if let Some(d) = std::env::var_os(app.settings_env) {
        return Some(PathBuf::from(d));
    }
    let base = dreist_formats::paths::settings_base()?;
    static MIGRATED: std::sync::Once = std::sync::Once::new();
    MIGRATED.call_once(|| {
        migrate_settings(&base, app.settings_dir, app.legacy_settings_dirs);
    });
    Some(base.join(app.settings_dir))
}

/// A unit test's settings folder: one per test thread, like the options
/// file (`TEST_FILE`), so the shortlists, favourites or labels a test
/// writes never show in an editor another test renders at the same time
/// (the screens golden). Removed when the thread ends.
#[cfg(any(test, feature = "test-settings"))]
fn test_settings_dir() -> Option<PathBuf> {
    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    thread_local! {
        static DIR: Dir = Dir(std::env::temp_dir().join(format!("dreist-test-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))));
    }
    Some(DIR.with(|d| d.0.clone()))
}

#[cfg(not(any(test, feature = "test-settings")))]
fn test_settings_dir() -> Option<PathBuf> {
    None
}

/// Move the first existing `legacy` folder under `base` to `name`, unless
/// `name` exists already.
pub fn migrate_settings(base: &std::path::Path, name: &str, legacy: &[&str]) -> bool {
    let to = base.join(name);
    if to.exists() {
        return false;
    }
    legacy.iter().map(|l| base.join(l)).find(|p| p.is_dir()).is_some_and(|from| std::fs::rename(from, &to).is_ok())
}

/// The default preset (`DefaultPreset.<project extension>` in the settings
/// folder): the app's own extension, else one saved under another family.
pub fn default_preset() -> Option<PathBuf> {
    let d = settings_dir()?;
    let exts = dreist_formats::profile::extensions_of(dreist_formats::profile::DocKind::Project);
    let own = d.join(format!("DefaultPreset.{}", exts[0]));
    Some(exts.iter().map(|e| d.join(format!("DefaultPreset.{e}"))).find(|p| p.is_file()).unwrap_or(own))
}

impl Options {
    pub fn load() -> Options {
        // Unit tests never read or write the user's settings.
        let path = if cfg!(any(test, feature = "test-settings")) { None } else { settings_dir().map(|d| d.join("Options.txt")) };
        Options { values: read_file(&path), path, changed: Default::default() }
    }

    /// Write the keys set since the last save into the current file.
    pub fn save(&mut self) {
        let mut values = read_file(&self.path);
        for k in std::mem::take(&mut self.changed) {
            if let Some(v) = self.values.get(&k) {
                values.insert(k, v.clone());
            }
        }
        // Pick up the others' changes too.
        self.values = values.clone();
        #[cfg(any(test, feature = "test-settings"))]
        TEST_FILE.with(|f| *f.borrow_mut() = values);
        #[cfg(not(any(test, feature = "test-settings")))]
        {
            let Some(p) = &self.path else { return };
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let text: String = values.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
            let _ = dreist_formats::file::write_atomic(p, text);
            *FILE_CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    }

    /// Every key and value (the diagnostics export lists them).
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    pub fn bool(&self, key: &str, default: bool) -> bool {
        self.values.get(key).map(|v| v == "true" || v == "1").unwrap_or(default)
    }

    pub fn int(&self, key: &str, default: i64) -> i64 {
        self.values.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    pub fn string(&self, key: &str, default: &str) -> String {
        self.values.get(key).cloned().unwrap_or_else(|| default.to_string())
    }

    pub fn set(&mut self, key: &str, value: impl ToString) {
        self.values.insert(key.to_string(), value.to_string());
        self.changed.insert(key.to_string());
    }
}

#[cfg(test)]
mod migrate_tests {
    #[test]
    fn legacy_settings_folder_moves_once() {
        let base = std::env::temp_dir().join(format!("dreist-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("Old")).unwrap();
        std::fs::write(base.join("Old/Options.txt"), "A=1\n").unwrap();
        assert!(super::migrate_settings(&base, "New", &["Old"]));
        assert!(!base.join("Old").exists());
        assert_eq!(std::fs::read_to_string(base.join("New/Options.txt")).unwrap(), "A=1\n");
        std::fs::create_dir_all(base.join("Old")).unwrap();
        assert!(!super::migrate_settings(&base, "New", &["Old"]), "an existing folder is kept");
        assert!(base.join("Old").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
