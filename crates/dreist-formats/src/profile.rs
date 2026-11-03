//! The app profile: what an app built on these crates (Dreist, Einst)
//! supplies — names, theme, settings folder, where its resources live, the
//! file extensions it saves, its effect menu and how it reads documents.
//!
//! An app calls [`install`] once at start-up, before opening any window or
//! document; until then (and in this workspace's own tests) [`get`] returns
//! [`BASE`]. The effect nodes have their own hook in
//! `dreist_dsp::fx::install_factory`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::Node;

/// Where one set of resources is found: `env` (an override), then
/// `Contents/Resources/<bundle_dir>` of the bundle holding this code (or
/// `Resources/<bundle_dir>` next to a bare Windows binary), then the first
/// existing `dev_dirs` entry (development builds and tests).
#[derive(Debug, Clone, Copy)]
pub struct Resources {
    pub env: &'static str,
    pub bundle_dir: &'static str,
    pub dev_dirs: &'static [&'static str],
}

/// One family of document extensions (project, kit, engine, pad, pattern,
/// bank, MIDI binding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extensions {
    pub project: &'static str,
    pub kit: &'static str,
    pub engine: &'static str,
    pub pad: &'static str,
    pub pattern: &'static str,
    pub bank: &'static str,
    pub binding: &'static str,
}

/// The kinds of document a file extension names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    Project,
    Kit,
    Engine,
    Pad,
    Pattern,
    Bank,
    Binding,
}

impl Extensions {
    /// The extension of `kind` in this family.
    pub fn of(&self, kind: DocKind) -> &'static str {
        match kind {
            DocKind::Project => self.project,
            DocKind::Kit => self.kit,
            DocKind::Engine => self.engine,
            DocKind::Pad => self.pad,
            DocKind::Pattern => self.pattern,
            DocKind::Bank => self.bank,
            DocKind::Binding => self.binding,
        }
    }

    fn kind(&self, ext: &str) -> Option<DocKind> {
        use DocKind::*;
        [Project, Kit, Engine, Pad, Pattern, Bank, Binding].into_iter().find(|&k| self.of(k).eq_ignore_ascii_case(ext))
    }
}

/// The legacy family (the original instruments' files).
pub const LEGACY: Extensions = Extensions { project: "geist", kit: "geistkit", engine: "geistengine", pad: "geistpad", pattern: "geistpattern", bank: "geistbank", binding: "geistbinding" };

/// Dreist's family.
pub const DREIST: Extensions = Extensions { project: "dreist", kit: "dreistkit", engine: "dreistengine", pad: "dreistpad", pattern: "dreistpattern", bank: "dreistbank", binding: "dreistbinding" };

/// Einst's family.
pub const EINST: Extensions = Extensions { project: "einst", kit: "einstkit", engine: "einstengine", pad: "einstpad", pattern: "einstpattern", bank: "einstbank", binding: "einstbinding" };

/// Every family; every app opens all of them.
pub const FAMILIES: [Extensions; 3] = [LEGACY, DREIST, EINST];

/// The Open dialog's extensions: projects of every family (the app's own
/// first), then the profile's [`Profile::open_also`].
pub fn open_extensions() -> Vec<&'static str> {
    let mut v = extensions_of(DocKind::Project);
    v.extend(get().open_also);
    v
}

/// Short legacy kit extension, also opened as a kit.
pub const LEGACY_KIT_SHORT: &str = "gkit";

/// The kind of document `ext` names in any family (case-insensitive).
pub fn doc_kind(ext: &str) -> Option<DocKind> {
    if ext.eq_ignore_ascii_case(LEGACY_KIT_SHORT) {
        return Some(DocKind::Kit);
    }
    FAMILIES.iter().find_map(|f| f.kind(ext))
}

/// Whether `ext` is a project extension of any family.
pub fn is_project_ext(ext: &str) -> bool {
    doc_kind(ext) == Some(DocKind::Project)
}

/// Every extension of `kind`, across the families (the app's own first).
pub fn extensions_of(kind: DocKind) -> Vec<&'static str> {
    let own = get().save.of(kind);
    let mut v = vec![own];
    v.extend(FAMILIES.iter().map(|f| f.of(kind)).filter(|e| *e != own));
    if kind == DocKind::Kit {
        v.push(LEGACY_KIT_SHORT);
    }
    v
}

#[derive(Debug, Clone, Copy)]
pub struct Profile {
    /// Product name (window titles, menus, dialogs).
    pub product: &'static str,
    /// The About text.
    pub about: &'static str,
    /// The default skin theme (`Skin/Styles/<theme>.style`).
    pub theme: &'static str,
    /// The stylesheets' text family, drawn with Liberation Sans.
    pub ui_family: &'static str,
    /// The icon font (`<icon_family>.ttf` in the UI resources' `Fonts`).
    pub icon_family: &'static str,
    /// Settings folder name (`~/Library/Application Support/<name>`,
    /// `%APPDATA%\<name>`).
    pub settings_dir: &'static str,
    /// Earlier settings folder names: the first one found is moved to
    /// [`Profile::settings_dir`] when that does not exist yet.
    pub legacy_settings_dirs: &'static [&'static str],
    /// Environment variable naming a separate settings folder.
    pub settings_env: &'static str,
    /// UI graphics: `Skin/`, `Fonts/`, [re:038bdf], `Metronomes/`.
    pub ui: Resources,
    /// Factory data: `Effects/`, `Effect Chains/`, `Pattern Templates/`,
    /// `Swing Templates/`.
    pub factory: Resources,
    /// The extension family the app saves.
    pub save: Extensions,
    /// Further extensions the Open dialog offers besides projects (files
    /// `read_project` turns into a project).
    pub open_also: &'static [&'static str],
    /// The effect types offered in effect menu category `c` (0..6), in menu order.
    pub effect_menu: fn(usize) -> Vec<usize>,
    /// A document file's root element (the editor's part loads); `None`
    /// when it cannot be read.
    pub read_document: fn(&Path) -> Option<Node>,
    /// A project to open and whether it was converted from another format
    /// (a converted project is saved through Save As).
    pub read_project: fn(&Path) -> Option<(Node, bool)>,
}

/// The document as stored.
pub fn plain_document(path: &Path) -> Option<Node> {
    Node::parse(&std::fs::read_to_string(path).ok()?).ok()
}

/// The project as stored.
pub fn plain_project(path: &Path) -> Option<(Node, bool)> {
    crate::Project::load(path).ok().map(|p| (p.raw, false))
}

/// The workspace's reference profile: Dreist's names and menu. Its
/// resources are looked up in a Dreist checkout next to this repository
/// (`../dreist`), so this workspace's tests draw Dreist's UI.
pub const BASE: Profile = Profile {
    product: "Dreist",
    about: "Dreist: drum sampler, sequencer and performance instrument. Native for macOS and Windows, written in Rust.",
    theme: "Dreist",
    ui_family: "Dreist Sans",
    icon_family: "Dreist-Icons",
    settings_dir: "Dreist",
    legacy_settings_dirs: &[],
    settings_env: "DREIST_SETTINGS_DIR",
    ui: Resources { env: "DREIST_UI", bundle_dir: "Dreist", dev_dirs: &[concat!(env!("CARGO_MANIFEST_DIR"), "/../../../dreist/assets/ui")] },
    factory: Resources { env: "DREIST_RESOURCES", bundle_dir: "Factory", dev_dirs: &[concat!(env!("CARGO_MANIFEST_DIR"), "/../../../dreist/assets/factory")] },
    save: DREIST,
    open_also: &[],
    effect_menu: crate::params::effect_menu,
    read_document: plain_document,
    read_project: plain_project,
};

static PROFILE: OnceLock<&'static Profile> = OnceLock::new();

/// Install the app's profile. The first call wins; later calls with
/// another profile are ignored and return `false`.
pub fn install(p: &'static Profile) -> bool {
    std::ptr::eq(*PROFILE.get_or_init(|| p), p)
}

/// The installed profile, or [`BASE`].
pub fn get() -> &'static Profile {
    PROFILE.get().copied().unwrap_or(&BASE)
}

impl Resources {
    /// The folder, by the lookup order above (the last `dev_dirs` entry
    /// when none exists).
    pub fn dir(&self) -> PathBuf {
        if let Some(p) = std::env::var_os(self.env) {
            return PathBuf::from(p);
        }
        if let Some(p) = crate::paths::bundled_dir(self.bundle_dir) {
            return p;
        }
        let dev = self.dev_dirs.iter().map(PathBuf::from);
        dev.clone().find(|p| p.is_dir()).or_else(|| dev.clone().next_back()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_is_recognised_and_apps_save_their_own() {
        for f in FAMILIES {
            assert!(is_project_ext(f.project));
            assert_eq!(doc_kind(f.kit), Some(DocKind::Kit));
            assert_eq!(doc_kind(f.pattern), Some(DocKind::Pattern));
            assert_eq!(doc_kind(&f.bank.to_uppercase()), Some(DocKind::Bank));
        }
        assert_eq!(doc_kind("gkit"), Some(DocKind::Kit));
        assert_eq!(doc_kind("wav"), None);
        assert_eq!(get().save, DREIST);
        assert_eq!(extensions_of(DocKind::Project), ["dreist", "geist", "einst"]);
        assert_eq!(extensions_of(DocKind::Kit), ["dreistkit", "geistkit", "einstkit", "gkit"]);
    }

    #[test]
    fn families_do_not_overlap() {
        let mut all: Vec<&str> = FAMILIES.iter().flat_map(|f| [f.project, f.kit, f.engine, f.pad, f.pattern, f.bank, f.binding]).collect();
        let n = all.len();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), n);
    }
}
