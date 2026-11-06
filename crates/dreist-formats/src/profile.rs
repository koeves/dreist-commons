//! The app profile: what an app built on these crates (Dreist, Einst)
//! supplies to them — names, theme, fonts, logo, where its resources live
//! and the file extensions it saves. Each app's editor holds the rest
//! (About text, settings folder, effect menu, document readers).
//!
//! An app calls [`install`] once at start-up, before opening any window or
//! document; until then (and in this workspace's own tests) [`get`] returns
//! [`BASE`]. The effect nodes have their own hook in
//! `dreist_dsp::fx::install_factory`.

use std::path::PathBuf;
use std::sync::OnceLock;

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
    /// The default skin theme (`Skin/Styles/<theme>.style`).
    pub theme: &'static str,
    /// The stylesheets' text family, drawn with Liberation Sans.
    pub ui_family: &'static str,
    /// The icon font (`<icon_family>.ttf` in the UI resources' `Fonts`).
    pub icon_family: &'static str,
    /// The header logo's product mark: filled polygons in the logo's
    /// 50 × 50 box (inside its 3 px frame).
    pub logo_mark: &'static [&'static [(f32, f32)]],
    /// UI graphics: `Skin/`, `Fonts/`, [re:038bdf], `Metronomes/`.
    pub ui: Resources,
    /// Factory data: `Effects/`, `Effect Chains/`, `Pattern Templates/`,
    /// `Swing Templates/`.
    pub factory: Resources,
    /// The extension family the app saves.
    pub save: Extensions,
}

/// Dreist's logo mark, a "D": a stem and a bowl.
pub const DREIST_LOGO_MARK: &[&[(f32, f32)]] = &[
    &[(9.0, 9.0), (16.0, 9.0), (16.0, 41.0), (9.0, 41.0)],
    &[
        (18.0, 9.0),
        (28.0, 9.0),
        (36.0, 13.0),
        (41.0, 20.0),
        (41.0, 30.0),
        (36.0, 37.0),
        (28.0, 41.0),
        (18.0, 41.0),
        (18.0, 34.0),
        (27.0, 34.0),
        (32.0, 31.0),
        (34.0, 27.0),
        (34.0, 23.0),
        (32.0, 19.0),
        (27.0, 16.0),
        (18.0, 16.0),
    ],
];

/// The workspace's reference profile: Dreist's names and menu. Its
/// resources are looked up in a Dreist checkout next to this repository
/// (`../dreist`), so this workspace's tests draw Dreist's UI.
pub const BASE: Profile = Profile {
    product: "Dreist",
    theme: "Dreist",
    ui_family: "Dreist Sans",
    icon_family: "Dreist-Icons",
    logo_mark: DREIST_LOGO_MARK,
    ui: Resources { env: "DREIST_UI", bundle_dir: "Dreist", dev_dirs: &[concat!(env!("CARGO_MANIFEST_DIR"), "/../../../dreist/assets/ui")] },
    factory: Resources { env: "DREIST_RESOURCES", bundle_dir: "Factory", dev_dirs: &[concat!(env!("CARGO_MANIFEST_DIR"), "/../../../dreist/assets/factory")] },
    save: DREIST,
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
