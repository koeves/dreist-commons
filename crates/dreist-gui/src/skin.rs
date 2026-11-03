//! Locating and loading the GUI skin: `project.gltinfo` lists the default
//! stylesheets; themes are [re:96fe82] folders; panels are
//! `.skin` files.
//!
//! Dreist's own skin (`dreist_formats::paths::ui_dir()/Skin`, see
//! [`Skin::ours`]) is searched first, then [`Skin::dir`] (the VST2-style
//! skin folder, which no longer ships).

use std::path::{Path, PathBuf};

use dreist_formats::Node;

use crate::ctrl::Ctrl;
use crate::style::StyleSheet;

/// The theme drawn when none (or an unknown one) is chosen: the profile's.
pub fn default_theme() -> &'static str {
    dreist_formats::profile::get().theme
}

#[derive(Clone, Debug)]
pub struct Skin {
    /// The VST2-style skin folder (`dreist_formats::paths::skin_dir`): no
    /// skins left there, but the factory data sits beside it (presets,
    /// templates).
    pub dir: PathBuf,
    /// Dreist's own skin folder, searched first.
    pub ours: PathBuf,
    pub theme: String,
    pub styles: StyleSheet,
}

#[derive(Debug)]
pub enum SkinError {
    Io(PathBuf, std::io::Error),
    Parse(PathBuf, String),
}

impl std::fmt::Display for SkinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkinError::Io(p, e) => write!(f, "{}: {e}", p.display()),
            SkinError::Parse(p, e) => write!(f, "{}: {e}", p.display()),
        }
    }
}

impl std::error::Error for SkinError {}

pub fn read_node(path: &Path) -> Result<Node, SkinError> {
    let text = std::fs::read_to_string(path).map_err(|e| SkinError::Io(path.to_path_buf(), e))?;
    Node::parse(&text).map_err(|e| SkinError::Parse(path.to_path_buf(), e.to_string()))
}

impl Skin {
    /// The skin folder: Dreist's own (`assets/dreist/Skin`).
    pub fn resources_dir() -> PathBuf {
        Self::ours_dir()
    }

    /// Dreist's own skin folder (`assets/dreist/Skin` or the bundle's).
    pub fn ours_dir() -> PathBuf {
        dreist_formats::paths::ui_dir().join("Skin")
    }

    /// The themes on offer: the `.style` folders of our skin.
    pub fn themes() -> Vec<String> {
        let mut v: Vec<String> =
            std::fs::read_dir(Self::ours_dir().join("Styles")).into_iter().flatten().flatten().filter_map(|e| e.file_name().to_str()?.strip_suffix(".style").map(str::to_string)).collect();
        v.sort();
        v
    }

    /// `rel` (relative to the skin folder): ours first, then `dir` when
    /// a caller loaded the skin from somewhere else.
    pub fn resolve(&self, rel: &str) -> PathBuf {
        let rel = rel.trim_start_matches('/');
        let mine = self.ours.join(rel);
        if mine.exists() { mine } else { self.dir.join(rel) }
    }

    /// Load the default stylesheets (`project.gltinfo`) with `theme`'s
    /// overrides; an unknown theme draws the default one.
    pub fn load(dir: &Path, theme: &str) -> Result<Skin, SkinError> {
        let ours = Self::ours_dir();
        let theme = if Self::themes().iter().any(|t| t == theme) { theme } else { default_theme() };
        let mut skin = Skin { dir: dir.to_path_buf(), ours, theme: theme.to_string(), styles: StyleSheet::new() };
        let info = read_node(&skin.resolve("project.gltinfo"))?;
        let defaults: Vec<String> =
            info.children.iter().filter(|n| n.name.as_deref() == Some("defaultStyles")).flat_map(|v| v.children.iter().filter_map(|s| s.attr("string").map(str::to_string))).collect();
        for rel in &defaults {
            let p = skin.resolve(rel);
            if p.extension().is_some_and(|e| e == "stylesheet") {
                skin.styles.load(&read_node(&p)?);
            }
        }
        // Theme stylesheets override the defaults (usually only colours).
        if theme != default_theme() {
            if let Ok(rd) = std::fs::read_dir(skin.theme_dir()) {
                let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "stylesheet")).collect();
                files.sort();
                for f in files {
                    skin.styles.load(&read_node(&f)?);
                }
            }
        }
        Ok(skin)
    }

    pub fn theme_dir(&self) -> PathBuf {
        self.ours.join("Styles").join(format!("{}.style", self.theme))
    }

    /// Where the UI fonts live (Dreist's `Fonts` folder).
    pub fn font_dir(&self) -> PathBuf {
        dreist_formats::paths::ui_dir().join("Fonts")
    }

    /// Path (relative to the resources folder) of a skin given by bare file
    /// name, as the editor code refers to them (`"[re:a7493d].skin"`).
    pub fn find(&self, name: &str) -> Option<String> {
        fn walk(dir: &Path, base: &Path, name: &str) -> Option<String> {
            let rd = std::fs::read_dir(dir).ok()?;
            let mut subdirs = Vec::new();
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    subdirs.push(p);
                } else if p.file_name().is_some_and(|f| f == name) {
                    return Some(p.strip_prefix(base).ok()?.to_string_lossy().into_owned());
                }
            }
            subdirs.sort();
            subdirs.iter().find_map(|d| walk(d, base, name))
        }
        walk(&self.ours, &self.ours, name).or_else(|| walk(&self.dir, &self.dir, name))
    }

    /// Load the skin named `name` and mount its top-level control into
    /// `host`, sized to it (anchoring adapts the layout).
    pub fn mount(&mut self, host: &mut Ctrl, name: &str) -> Result<(), SkinError> {
        let rel = self.find(name).unwrap_or_else(|| name.to_string());
        let sub = self.load_skin(&rel)?;
        for mut c in sub.children {
            c.set_rect(crate::ctrl::Rect::new(0, 0, host.rect.w, host.rect.h));
            host.children.push(c);
        }
        Ok(())
    }

    /// Load a `.skin` (path relative to the resources folder): registers
    /// its inline styles and returns the root controls as one container.
    pub fn load_skin(&mut self, rel: &str) -> Result<Ctrl, SkinError> {
        let root = read_node(&self.resolve(rel))?;
        self.styles.load(&root);
        let mut c = Ctrl::from_node(&root);
        c.class = "root".into();
        if let Some(first) = c.children.first() {
            if c.children.len() == 1 {
                c.rect = first.rect;
            }
        }
        Ok(c)
    }
}
