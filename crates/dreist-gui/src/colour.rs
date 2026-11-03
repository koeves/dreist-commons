//! [re:52ce43] / [re:f60e99]: integer RGBA colours, the standard named
//! colours, string parsing and the stylesheet modifiers.

/// 8-bit RGBA, straight (non-premultiplied) alpha, as [re:96fe05] stores it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Colour {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Colour {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Colour {
        Colour { r, g, b, a }
    }

    pub const TRANSPARENT: Colour = Colour::rgba(0, 0, 0, 0);
    pub const BLACK: Colour = Colour::rgba(0, 0, 0, 255);
    pub const WHITE: Colour = Colour::rgba(255, 255, 255, 255);

    pub fn f_alpha(self) -> f32 {
        self.a as f32 / 255.0
    }

    /// [re:bc403f]: truncating, clamped to 0..255.
    pub fn with_alpha(self, a: f32) -> Colour {
        Colour { a: ((a * 255.0) as i32).clamp(0, 255) as u8, ..self }
    }

    /// [re:b4a7a7]: `255 − (255 − c) / (1 + f)`.
    pub fn lighten(self, f: f32) -> Colour {
        let k = 1.0 / (f + 1.0);
        let l = |c: u8| ((255.0 - (255 - c as i32) as f32 * k) as i32).clamp(0, 255) as u8;
        Colour { r: l(self.r), g: l(self.g), b: l(self.b), a: self.a }
    }

    /// [re:d90ff6]: `c / (1 + f)`.
    pub fn darken(self, f: f32) -> Colour {
        let k = 1.0 / (f + 1.0);
        let d = |c: u8| ((c as f32 * k) as i32).clamp(0, 255) as u8;
        Colour { r: d(self.r), g: d(self.g), b: d(self.b), a: self.a }
    }

    /// [re:8de5de]: rotate the hue by `deg`.
    pub fn hue_shift(self, deg: f32) -> Colour {
        let (r, g, b) = (self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        if max == min {
            return self;
        }
        let d = max - min;
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let mut h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        } * 60.0;
        h = (h + deg).rem_euclid(360.0) / 360.0;
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        let f = |mut t: f32| {
            t = t.rem_euclid(1.0);
            let v = if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            };
            (v * 255.0) as u8
        };
        Colour { r: f(h + 1.0 / 3.0), g: f(h), b: f(h - 1.0 / 3.0), a: self.a }
    }

    /// Literal part of [re:58c6b6] (names are resolved
    /// by the caller first): `#RRGGBB` / `0xRRGGBB` (opaque) or up to four
    /// decimal components separated by commas, with `[](){}rgba:` ignored.
    pub fn parse_literal(s: &str) -> Option<Colour> {
        let t = s.trim();
        if let Some(hex) = t.strip_prefix('#').or_else(|| t.strip_prefix("0x")) {
            let v = u32::from_str_radix(hex.trim(), 16).ok()?;
            return Some(Colour::rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, 255));
        }
        let mut t = t.to_string();
        if t.contains(" g:") {
            t = t.replace(' ', ",");
        }
        let cleaned: String = t.chars().map(|c| if "[]{}()rgba:".contains(c) { ' ' } else { c }).collect();
        let mut v = [0i32, 0, 0, 255];
        let mut any = false;
        for (i, tok) in cleaned.split(',').take(4).enumerate() {
            let tok = tok.trim();
            if tok.is_empty() && i == 0 {
                return None;
            }
            v[i] = atoi(tok).max(0);
            any = true;
        }
        any.then(|| Colour::rgba(v[0].min(255) as u8, v[1].min(255) as u8, v[2].min(255) as u8, v[3].min(255) as u8))
    }
}

/// C `atoi`: leading whitespace, optional sign, digits; 0 otherwise.
fn atoi(s: &str) -> i32 {
    let s = s.trim_start();
    let (neg, rest) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let n = rest.bytes().take_while(u8::is_ascii_digit).fold(0i64, |a, d| (a * 10 + (d - b'0') as i64).min(i32::MAX as i64));
    (if neg { -n } else { n }) as i32
}

/// [re:32c45e]: table [re:c14dfa] (47 × RGBA int),
/// names from `CreateStandardColoursMap` [re:01614a].
pub const STANDARD: [(&str, Colour); 47] = {
    const fn c(r: u8, g: u8, b: u8, a: u8) -> Colour {
        Colour::rgba(r, g, b, a)
    }
    [
        ("Aqua", c(0, 255, 255, 255)),
        ("Azure", c(240, 255, 255, 255)),
        ("Black", c(0, 0, 0, 255)),
        ("Beige", c(245, 245, 220, 255)),
        ("Blue", c(22, 101, 220, 255)),
        ("Chartreuse", c(127, 255, 0, 255)),
        ("Cyan", c(0, 255, 255, 255)),
        ("DarkGrey", c(64, 64, 64, 255)),
        ("Fuchsia", c(255, 0, 255, 255)),
        ("Gold", c(255, 215, 0, 255)),
        ("Green", c(82, 202, 82, 255)),
        ("Grey", c(128, 128, 128, 255)),
        ("Lavender", c(200, 200, 250, 255)),
        ("LightBlue", c(128, 128, 255, 255)),
        ("LightGrey", c(192, 192, 192, 255)),
        ("Lime", c(0, 255, 0, 255)),
        ("Magenta", c(255, 0, 255, 255)),
        ("Maroon", c(128, 0, 0, 255)),
        ("Mauve", c(200, 192, 200, 255)),
        ("Navy", c(0, 0, 128, 255)),
        ("Olive", c(128, 128, 0, 255)),
        ("Orange", c(255, 128, 0, 255)),
        ("PaleGreen", c(200, 255, 200, 255)),
        ("Purple", c(128, 0, 128, 255)),
        ("Pink", c(255, 200, 200, 255)),
        ("Red", c(220, 22, 22, 255)),
        ("Silver", c(192, 192, 192, 255)),
        ("Slate", c(112, 128, 144, 255)),
        ("Turquoise", c(72, 209, 204, 255)),
        ("Teal", c(0, 128, 128, 255)),
        ("White", c(255, 255, 255, 255)),
        ("Yellow", c(255, 255, 0, 255)),
        ("Transparent", c(0, 0, 0, 0)),
        ("AccentBlue", c(37, 112, 226, 255)),
        ("VellocetViolet", c(109, 1, 175, 255)),
        ("StandardControlBackground", c(255, 255, 255, 255)),
        ("StandardControlBackgroundDark", c(195, 195, 195, 255)),
        ("StandardControlBorder", c(100, 100, 100, 255)),
        ("StandardControlBorderLighter", c(200, 200, 200, 255)),
        ("StandardControlBorderDarker", c(80, 80, 80, 255)),
        ("StandardControlText", c(0, 0, 0, 255)),
        ("StandardControlWebLinkText", c(57, 62, 219, 255)),
        ("StandardControlDisabledText", c(180, 180, 180, 255)),
        ("StandardControlHighlight", c(0, 228, 161, 255)),
        ("StandardControlHighlightDarker", c(18, 180, 132, 255)),
        ("StandardControlHighlightDarkest", c(21, 135, 102, 255)),
        ("StandardControlMenuFill", c(252, 252, 252, 255)),
    ]
};

pub fn standard(name: &str) -> Colour {
    STANDARD.iter().find(|(n, _)| *n == name).map(|(_, c)| *c).unwrap_or(Colour::BLACK)
}

/// [re:09e718] modifiers: `lighter=`, `darker=`, `alpha=`
/// (each clamped 0..1) and `hue=` (0..360).
pub fn apply_modifier(c: Colour, modifier: &str) -> Colour {
    let arg = |p: &str| modifier.strip_prefix(p).map(|v| v.trim().parse::<f64>().unwrap_or(0.0) as f32);
    if let Some(f) = arg("lighter=") {
        c.lighten(f.clamp(0.0, 1.0))
    } else if let Some(f) = arg("darker=") {
        c.darken(f.clamp(0.0, 1.0))
    } else if let Some(f) = arg("alpha=") {
        c.with_alpha(f.clamp(0.0, 1.0))
    } else if let Some(f) = arg("hue=") {
        c.hue_shift(f.clamp(0.0, 360.0))
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_literals() {
        assert_eq!(Colour::parse_literal("#282E2D"), Some(Colour::rgba(0x28, 0x2e, 0x2d, 255)));
        assert_eq!(Colour::parse_literal("218, 224, 228"), Some(Colour::rgba(218, 224, 228, 255)));
        assert_eq!(Colour::parse_literal("1, 2, 3, 4"), Some(Colour::rgba(1, 2, 3, 4)));
        // Unresolved names fall through to atoi (black), as in the original.
        assert_eq!(Colour::parse_literal("PanelDark"), Some(Colour::BLACK));
    }

    #[test]
    fn modifiers() {
        let c = Colour::rgba(100, 110, 114, 255);
        assert_eq!(apply_modifier(c, "alpha=0.7").a, 178);
        assert_eq!(c.darken(0.4), Colour::rgba(71, 78, 81, 255));
        assert_eq!(c.lighten(0.7).r, 163);
    }
}
