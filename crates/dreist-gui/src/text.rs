//! Text: fontdue rasterisation standing in for CoreText. Fonts come from the
//! system (Tahoma) and Dreist's UI folder (the icon font,
//! `dreist_formats::paths::ui_dir`). The skins' icon family names map to
//! app's own font ([`icon_family`]).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fontdue::{Font, FontSettings, Metrics};

use crate::ctrl::FxMap;
use crate::style::{FontSpec, Shorten};

/// Glyph fallbacks for characters Dreist's fonts lack.
/// `seguisym` is Windows' Segoe UI Symbol (its file name).
const FALLBACKS: [&str; 4] = ["Arial Unicode", "Apple Symbols", "DejaVu Sans", "seguisym"];

/// The app's icon font (`<family>.ttf` in the UI folder's `Fonts`): same
/// code points as the skins' icon captions.
pub fn icon_family() -> &'static str {
    dreist_formats::profile::get().icon_family
}

/// The app's text family (the stylesheets' `fontName`): drawn with
/// Liberation Sans from the UI folder.
pub fn ui_family() -> &'static str {
    dreist_formats::profile::get().ui_family
}

/// Liberation Sans standing in for Tahoma draws this much smaller: the
/// same text then takes Tahoma's width (its advances are ~4.5% wider).
const TAHOMA_FIT: f32 = 0.957;

pub struct Fonts {
    dirs: Vec<PathBuf>,
    /// Loaded fonts by family, regular and bold (`None`: not tried yet).
    cache: FxMap<String, [Option<Option<Font>>; 2]>,
    /// Size factors of substituted families, regular and bold (see
    /// [`Fonts::size`]).
    fit: FxMap<String, [f32; 2]>,
    /// Rasterised glyphs by font, character and pixel size: text redraws
    /// every frame something near it changes.
    glyphs: RefCell<HashMap<(usize, char, u32), Arc<(Metrics, Vec<u8>)>>>,
}

/// A font spec's font and the fallbacks, looked up once for a string.
pub struct FontSet<'a> {
    primary: Option<&'a Font>,
    fallbacks: [Option<&'a Font>; FALLBACKS.len()],
}

impl<'a> FontSet<'a> {
    /// Font to draw `ch` with: the spec's own when it has the glyph, else
    /// the first fallback that does.
    pub fn pick(&self, ch: char) -> Option<&'a Font> {
        if let Some(f) = self.primary {
            if f.lookup_glyph_index(ch) != 0 || ch == ' ' {
                return Some(f);
            }
        }
        self.fallbacks.iter().flatten().copied().find(|f| f.lookup_glyph_index(ch) != 0).or(self.primary)
    }
}

impl Fonts {
    /// `theme_dir`: the skin theme's `.style` folder (searched after
    /// Dreist's own fonts).
    pub fn new(theme_dir: &Path) -> Fonts {
        let mut dirs = vec![
            dreist_formats::paths::ui_dir().join("Fonts"),
            theme_dir.to_path_buf(),
            PathBuf::from("/System/Library/Fonts/Supplemental"),
            PathBuf::from("/System/Library/Fonts"),
            PathBuf::from("/Library/Fonts"),
            PathBuf::from(r"C:\Windows\Fonts"),
        ];
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Fonts"));
        }
        Fonts { dirs, cache: FxMap::default(), fit: FxMap::default(), glyphs: RefCell::new(HashMap::new()) }
    }

    fn load(&self, family: &str, bold: bool) -> Option<Font> {
        let family = if dreist_formats::legacy::ICON_FAMILIES.contains(&family) { icon_family() } else { family };
        let names: Vec<String> =
            if bold { vec![format!("{family} Bold.ttf"), format!("{family}-Bold.ttf"), format!("{family}bd.ttf"), format!("{family}.ttf")] } else { vec![format!("{family}.ttf")] };
        for d in &self.dirs {
            for n in &names {
                if let Ok(bytes) = std::fs::read(d.join(n)) {
                    if let Ok(f) = Font::from_bytes(bytes, FontSettings::default()) {
                        return Some(f);
                    }
                }
            }
        }
        None
    }

    pub fn get(&mut self, spec: &FontSpec) -> Option<&Font> {
        let b = spec.bold as usize;
        if self.cache.get(spec.family.as_str()).is_none_or(|e| e[b].is_none()) {
            let (f, fit) = self.find(spec);
            if let Some(fit) = fit {
                self.fit.entry(spec.family.clone()).or_insert([1.0; 2])[b] = fit;
            }
            self.cache.entry(spec.family.clone()).or_default()[b] = Some(f);
        }
        self.loaded(&spec.family, spec.bold)
    }

    /// `spec`'s font, or its stand-in with the size factor it needs.
    fn find(&self, spec: &FontSpec) -> (Option<Font>, Option<f32>) {
        // Dreist's UI family: Liberation Sans (open licence, shipped in
        // the UI folder) sized to the widths the layouts were drawn for.
        if spec.family == ui_family() {
            if let Some(f) = self.load("Liberation Sans", spec.bold) {
                return (Some(f), Some(TAHOMA_FIT));
            }
        }
        // Liberation Sans (Arial's metrics, open licence): the stand-in
        // when the system has none of these fonts.
        let f = self.load(&spec.family, spec.bold).or_else(|| self.load("Tahoma", spec.bold)).or_else(|| self.load("Arial", spec.bold));
        if f.is_some() {
            return (f, None);
        }
        let f = self.load("Liberation Sans", spec.bold);
        let fit = (f.is_some() && spec.family == "Tahoma").then_some(TAHOMA_FIT);
        (f, fit)
    }

    /// An already loaded font.
    fn loaded(&self, family: &str, bold: bool) -> Option<&Font> {
        self.cache.get(family).and_then(|e| e[bold as usize].as_ref()).and_then(Option::as_ref)
    }

    /// Load `spec` and the fallback fonts (CoreText substitutes glyphs the
    /// font lacks, e.g. "▼" in Tahoma).
    pub fn prepare(&mut self, spec: &FontSpec) {
        self.get(spec);
        for fb in FALLBACKS {
            if self.cache.get(fb).is_none_or(|e| e[0].is_none()) {
                self.get(&FontSpec { family: fb.to_string(), size: spec.size, bold: false });
            }
        }
    }

    /// Font to draw `ch` with (after `prepare`).
    pub fn font_for(&self, spec: &FontSpec, ch: char) -> Option<&Font> {
        self.font_set(spec).pick(ch)
    }

    /// `spec`'s font and the fallbacks (after `prepare`), for picking the
    /// font of each character of a string without looking them up again.
    pub fn font_set(&self, spec: &FontSpec) -> FontSet<'_> {
        FontSet { primary: self.loaded(&spec.family, spec.bold), fallbacks: FALLBACKS.map(|fb| self.loaded(fb, false)) }
    }

    /// `ch` of `font` rasterised at `size` pixels, from the cache.
    pub fn glyph(&self, font: &Font, ch: char, size: f32) -> Arc<(Metrics, Vec<u8>)> {
        let key = (font.file_hash(), ch, size.to_bits());
        if let Some(g) = self.glyphs.borrow().get(&key) {
            return g.clone();
        }
        let g = Arc::new(font.rasterize(ch, size));
        let mut glyphs = self.glyphs.borrow_mut();
        // Bounded: sizes change with the UI scale.
        if glyphs.len() >= 8192 {
            glyphs.clear();
        }
        glyphs.insert(key, g.clone());
        g
    }

    /// The size to draw `spec` at (after `prepare`): its own, or adjusted
    /// for a substituted family.
    pub fn size(&self, spec: &FontSpec) -> f32 {
        spec.size * self.fit.get(spec.family.as_str()).map_or(1.0, |f| f[spec.bold as usize])
    }

    pub fn measure(&mut self, spec: &FontSpec, s: &str) -> (f32, f32) {
        self.prepare(spec);
        let size = self.size(spec);
        let set = self.font_set(spec);
        let w = s.chars().map(|c| set.pick(c).map(|f| f.metrics(c, size).advance_width).unwrap_or(size * 0.55)).sum();
        let h = self.get(spec).and_then(|f| f.horizontal_line_metrics(size)).map(|m| m.ascent - m.descent).unwrap_or(size);
        (w, h)
    }

    /// Fit `s` into `width` with "...".
    pub fn shorten(&mut self, spec: &FontSpec, s: &str, width: f32, mode: Shorten) -> String {
        if mode == Shorten::None || width <= 0.0 || self.measure(spec, s).0 <= width {
            return s.to_string();
        }
        let chars: Vec<char> = s.chars().collect();
        for keep in (0..chars.len()).rev() {
            let cand: String = match mode {
                Shorten::Middle => {
                    let head = keep.div_ceil(2);
                    let tail = keep / 2;
                    chars[..head].iter().chain("...".chars().collect::<Vec<_>>().iter()).chain(chars[chars.len() - tail..].iter()).collect()
                }
                _ => chars[..keep].iter().collect::<String>() + "...",
            };
            if self.measure(spec, &cand).0 <= width {
                return cand;
            }
        }
        String::new()
    }
}
