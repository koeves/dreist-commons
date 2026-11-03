//! [re:733d78] stylesheets: named colours ([re:f60e99]), fonts
//! ([re:0a9a16]) and styles with parent inheritance.
//!
//! Attribute lookup follows [re:edaa92]: the
//! style's own value, else the first parent (depth-first) that has it, else
//! the attribute default ([re:3d83c2]). The built-in
//! `ApplicationStyle` holds every base attribute at its default and
//! `InvisibleStyle` clears fill and border ([re:307020], [re:3b7e9c]).

use std::collections::HashMap;

use dreist_formats::Node;

use crate::colour::{self, Colour};

/// [re:0a9a16]: family, size (points = pixels at 1x) and weight.
#[derive(Clone, Debug, PartialEq)]
pub struct FontSpec {
    pub family: String,
    pub size: f32,
    pub bold: bool,
}

impl Default for FontSpec {
    /// Base attribute default: Tahoma 12 ([re:b23392]).
    fn default() -> FontSpec {
        FontSpec { family: "Tahoma".into(), size: 12.0, bold: false }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Style {
    pub name: String,
    /// Element class ([re:1756ae], [re:7065b8], ...).
    pub class: String,
    pub parents: Vec<String>,
    pub attrs: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Center,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shorten {
    None,
    End,
    Middle,
}

/// Defaults of the base attributes ([re:743e63]).
fn base_default(name: &str) -> Option<&'static str> {
    Some(match name {
        "optDrawShadow" => "false",
        "optDrawBorder" | "optDrawText" | "optRoundTopLeft" | "optRoundTopRight" | "optRoundBottomLeft" | "optRoundBottomRight" => "true",
        "colBorder" => "StandardControlBorder",
        "colAltBorder" => "StandardControlBorderDarker",
        "colFill" => "StandardControlBackground",
        "colAltFill" => "StandardControlHighlight",
        "colFillHighlight" => "StandardControlHighlightDarker",
        "colText" | "colAltText" => "StandardControlText",
        "colDisabledOverlay" => "255, 255, 255, 127",
        "realDisabledAlpha" => "0.7",
        "realCornerRounding" | "realTextOffset" => "0",
        "realBorderWidth" | "realShadowOpacity" => "1",
        "realShadowAngle" => "135",
        "realShadowSize" | "realShadowDistance" => "3",
        "realShadowBlur" => "0.25",
        "realMacroValueScalar" => "200",
        "realMicroValueScaler" => "2000",
        "font" => "",
        "enumDisabledDrawMode" | "enumTextHorizontalAlign" | "enumTextVerticalAlign" => "1",
        "enumTextShortenAlgorithm" | "enumShadowStyle" => "0",
        _ => return None,
    })
}

#[derive(Clone, Debug, Default)]
pub struct StyleSheet {
    pub colours: HashMap<String, Colour>,
    pub fonts: HashMap<String, FontSpec>,
    pub styles: HashMap<String, Style>,
}

impl StyleSheet {
    pub fn new() -> StyleSheet {
        let mut s = StyleSheet::default();
        for (n, c) in colour::STANDARD {
            s.colours.insert(n.to_string(), c);
        }
        s.styles.insert("ApplicationStyle".into(), Style { name: "ApplicationStyle".into(), class: "CtrlStyle".into(), ..Style::default() });
        let mut inv = Style { name: "InvisibleStyle".into(), class: "CtrlStyle".into(), parents: vec!["ApplicationStyle".into()], ..Style::default() };
        for (k, v) in [("colFill", "Transparent"), ("colAltFill", "Transparent"), ("optDrawBorder", "false"), ("optDrawText", "false")] {
            inv.attrs.insert(k.into(), v.into());
        }
        s.styles.insert(inv.name.clone(), inv);
        s
    }

    /// [re:e904ea]: a registered name, else a literal.
    pub fn parse_colour(&self, s: &str) -> Colour {
        self.colours.get(s.trim()).copied().or_else(|| Colour::parse_literal(s)).unwrap_or(Colour::BLACK)
    }

    /// Load the elements of a `.stylesheet` or `.skin` root: [re:f60e99],
    /// [re:0a9a16] and style elements (any class ending in `Style`).
    pub fn load(&mut self, root: &Node) {
        for n in &root.children {
            let Some(name) = n.name.clone() else { continue };
            match n.class.as_str() {
                "VColour" => {
                    let c = match n.attr("colour") {
                        Some(v) => {
                            let c = self.parse_colour(v);
                            n.attr("modifier").map(|m| colour::apply_modifier(c, m)).unwrap_or(c)
                        }
                        None => {
                            let ch = |k: &str, d: i32| n.attr(k).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(d).clamp(0, 255) as u8;
                            Colour::rgba(ch("red", 0), ch("green", 0), ch("blue", 0), ch("alpha", 255))
                        }
                    };
                    self.colours.insert(name, c);
                }
                "VFont" => {
                    let f = FontSpec {
                        family: n.attr("fontName").unwrap_or("Tahoma").to_string(),
                        size: n.f32("fontSize").unwrap_or(12.0),
                        bold: n.attr("weight").is_some_and(|w| w.eq_ignore_ascii_case("bold")),
                    };
                    self.fonts.insert(name, f);
                }
                c if c.ends_with("Style") => {
                    let mut st = Style { name: name.clone(), class: c.to_string(), ..Style::default() };
                    let mut parents: Vec<(usize, String)> = Vec::new();
                    for (k, v) in &n.attrs {
                        if k == "name" {
                            continue;
                        }
                        if let Some(i) = k.strip_prefix("parentStyle") {
                            parents.push((i.parse().unwrap_or(0), v.clone()));
                        } else {
                            st.attrs.insert(k.clone(), v.clone());
                        }
                    }
                    parents.sort();
                    st.parents = parents.into_iter().map(|(_, p)| p).collect();
                    self.styles.insert(name, st);
                }
                _ => {}
            }
        }
    }

    /// Raw attribute value through the parent chain.
    pub fn raw(&self, style: &str, attr: &str) -> Option<&str> {
        self.raw_depth(style, attr, 0)
    }

    fn raw_depth(&self, style: &str, attr: &str, depth: usize) -> Option<&str> {
        let st = self.styles.get(style)?;
        if let Some(v) = st.attrs.get(attr) {
            return Some(v);
        }
        if depth > 32 {
            return None;
        }
        // ApplicationStyle carries every base attribute at its default.
        if st.name == "ApplicationStyle" {
            return base_default(attr);
        }
        st.parents.iter().find_map(|p| self.raw_depth(p, attr, depth + 1))
    }

    /// [re:aad242].
    pub fn colour(&self, style: &str, attr: &str, default: Colour) -> Colour {
        self.raw(style, attr).map(|v| self.parse_colour(v)).unwrap_or(default)
    }

    pub fn float(&self, style: &str, attr: &str, default: f32) -> f32 {
        self.raw(style, attr).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
    }

    pub fn bool(&self, style: &str, attr: &str, default: bool) -> bool {
        self.raw(style, attr).map(|v| v.trim() == "true" || v.trim() == "1").unwrap_or(default)
    }

    /// [re:63a9ca]: the named [re:0a9a16], else the default Tahoma 12.
    pub fn font(&self, style: &str) -> FontSpec {
        self.raw(style, "font").and_then(|f| self.fonts.get(f)).cloned().unwrap_or_default()
    }

    /// A font attribute other than `font` (e.g. `fontTimeline`), falling
    /// back to the style's `font`.
    pub fn font_attr(&self, style: &str, attr: &str) -> FontSpec {
        self.raw(style, attr).and_then(|f| self.fonts.get(f)).cloned().unwrap_or_else(|| self.font(style))
    }

    pub fn h_align(&self, style: &str) -> HAlign {
        match self.raw(style, "enumTextHorizontalAlign").map(str::trim) {
            Some("left" | "0") => HAlign::Left,
            Some("right" | "2") => HAlign::Right,
            _ => HAlign::Center,
        }
    }

    pub fn v_align(&self, style: &str) -> VAlign {
        match self.raw(style, "enumTextVerticalAlign").map(str::trim) {
            Some("top" | "0") => VAlign::Top,
            Some("bottom" | "2") => VAlign::Bottom,
            _ => VAlign::Center,
        }
    }

    pub fn shorten(&self, style: &str) -> Shorten {
        match self.raw(style, "enumTextShortenAlgorithm").or_else(|| self.raw(style, "enumShortenAlgorithn")).map(str::trim) {
            Some("end" | "1") => Shorten::End,
            Some("middle" | "2") => Shorten::Middle,
            _ => Shorten::None,
        }
    }

    /// Rounding options [re:6bd0c8]: TL, TR, BL, BR (default all true).
    pub fn rounding_options(&self, style: &str) -> [bool; 4] {
        ["optRoundTopLeft", "optRoundTopRight", "optRoundBottomLeft", "optRoundBottomRight"].map(|k| self.bool(style, k, true))
    }
}
