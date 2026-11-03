//! Generic tree for the original's [re:77e2c1] XML.
//!
//! Every v2 file (`.geist`, `.geistkit`, `.gsa`, `.fxpreset`, `.fxchain`,
//! `.patterntemplate`, `.swingtemplate`) is XML in the same shape:
//!
//! * element tag = serialized class (`Engine`, `Pad`, `Layer`, ...)
//! * `name` attribute = instance key within the parent (`Pad3`, `Layer0`, ...)
//! * optional `version` attribute = per-class serialization version
//! * remaining attributes = parameters, stored sparsely (defaults omitted)
//! * leaf children [re:05b513] / [re:f7fa23] / `VFixedString_N` carry a
//!   `string` attribute; [re:c9743d], [re:f60e99] carry typed scalars.
//!
//! Parameter attributes can be suffixed with `_SlotN` (modulator slot depth)
//! or `_MacroN` (macro depth); see [`Node::modulated`].

use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub class: String,
    pub name: Option<String>,
    pub version: Option<u32>,
    /// Attributes other than `name` and `version`, in document order.
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

/// A parameter value with its modulation routings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Modulated {
    /// Base value; `None` means the class default applies.
    pub base: Option<f32>,
    /// `(slot index, bipolar depth)` from `<param>_SlotN`.
    pub slots: Vec<(u32, f32)>,
    /// `(macro index, bipolar depth)` from `<param>_MacroN`.
    pub macros: Vec<(u32, f32)>,
}

impl Node {
    pub fn parse(text: &str) -> Result<Node, Error> {
        let doc = roxmltree::Document::parse(text)?;
        Ok(Self::from_xml(doc.root_element()))
    }

    /// A new element with a class and optional instance name.
    pub fn new(class: &str, name: Option<&str>) -> Node {
        Node { class: class.into(), name: name.map(Into::into), version: None, attrs: Vec::new(), children: Vec::new() }
    }

    /// Set (or replace) an attribute.
    pub fn set(&mut self, key: &str, value: impl ToString) -> &mut Node {
        let v = value.to_string();
        match self.attrs.iter_mut().find(|(k, _)| k == key) {
            Some(a) => a.1 = v,
            None => self.attrs.push((key.into(), v)),
        }
        self
    }

    /// Serialize as v2 writes files: 4-space indentation, `name` and
    /// `version` first, self-closing empty elements.
    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, depth: usize) {
        fn esc(s: &str) -> String {
            // Line breaks and tabs as character references: XML attribute
            // normalisation turns literal ones into spaces on reading.
            s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\n', "&#10;").replace('\r', "&#13;").replace('\t', "&#9;")
        }
        out.push_str(&"    ".repeat(depth));
        out.push('<');
        out.push_str(&self.class);
        if let Some(n) = &self.name {
            out.push_str(&format!(" name=\"{}\"", esc(n)));
        }
        if let Some(v) = self.version {
            out.push_str(&format!(" version=\"{v}\""));
        }
        for (k, v) in &self.attrs {
            out.push_str(&format!(" {k}=\"{}\"", esc(v)));
        }
        if self.children.is_empty() {
            out.push_str(" />\n");
            return;
        }
        out.push_str(">\n");
        for c in &self.children {
            c.write(out, depth + 1);
        }
        out.push_str(&"    ".repeat(depth));
        out.push_str(&format!("</{}>\n", self.class));
    }

    fn from_xml(e: roxmltree::Node) -> Node {
        let mut node = Node { class: e.tag_name().name().to_owned(), name: None, version: None, attrs: Vec::new(), children: Vec::new() };
        for a in e.attributes() {
            match a.name() {
                "name" => node.name = Some(a.value().to_owned()),
                // Version is numeric on every class seen so far; keep it raw if not.
                "version" => match a.value().parse() {
                    Ok(v) => node.version = Some(v),
                    Err(_) => node.attrs.push(("version".into(), a.value().into())),
                },
                k => node.attrs.push((k.to_owned(), a.value().to_owned())),
            }
        }
        node.children = e.children().filter(|c| c.is_element()).map(Self::from_xml).collect();
        node
    }

    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// A finite number (a damaged file's "NaN" / "inf" reads as missing,
    /// so callers fall back to their defaults).
    pub fn f32(&self, key: &str) -> Option<f32> {
        self.attr(key)?.parse::<f32>().ok().filter(|v| v.is_finite())
    }

    pub fn u32(&self, key: &str) -> Option<u32> {
        self.attr(key)?.parse().ok()
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.attr(key)? {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        }
    }

    pub fn modulated(&self, key: &str) -> Modulated {
        let mut m = Modulated { base: self.f32(key), ..Default::default() };
        for (k, v) in &self.attrs {
            let Some(rest) = k.strip_prefix(key).and_then(|r| r.strip_prefix('_')) else {
                continue;
            };
            let Ok(depth) = v.parse::<f32>() else { continue };
            if let Some(i) = rest.strip_prefix("Slot").and_then(|i| i.parse().ok()) {
                m.slots.push((i, depth));
            } else if let Some(i) = rest.strip_prefix("Macro").and_then(|i| i.parse().ok()) {
                m.macros.push((i, depth));
            }
        }
        m
    }

    /// An attribute that is `true` or `1` (surrounding blanks allowed, as
    /// the original's string compare after trimming).
    pub fn truthy(&self, key: &str) -> bool {
        self.attr(key).is_some_and(|v| matches!(v.trim(), "true" | "1"))
    }

    /// `LockState` (pads, layers, patterns and tracks the user locked).
    pub fn locked(&self) -> bool {
        self.truthy("LockState")
    }

    /// The child with this class and instance name, appended when missing.
    pub fn child_mut_or_insert(&mut self, class: &str, name: &str) -> &mut Node {
        let i = match self.children.iter().position(|c| c.class == class && c.name.as_deref() == Some(name)) {
            Some(i) => i,
            None => {
                self.children.push(Node::new(class, Some(name)));
                self.children.len() - 1
            }
        };
        &mut self.children[i]
    }

    /// Children of the given serialized class.
    pub fn children_of<'a, 'c>(&'a self, class: &'c str) -> impl Iterator<Item = &'a Node> + use<'a, 'c> {
        self.children.iter().filter(move |c| c.class == class)
    }

    /// Child with the given class and instance name.
    pub fn child(&self, class: &str, name: &str) -> Option<&Node> {
        self.children_of(class).find(|c| c.name.as_deref() == Some(name))
    }

    /// Value of a named string leaf ([re:05b513], [re:f7fa23], `VFixedString_N`).
    pub fn string(&self, name: &str) -> Option<&str> {
        self.children
            .iter()
            .filter(|c| c.class == "VString" || c.class == "FixedString" || c.class.starts_with("VFixedString_"))
            .find(|c| c.name.as_deref() == Some(name))
            .and_then(|c| c.attr("string"))
    }

    /// Trailing integer of the instance name: `Pad12` -> 12, `S3` -> 3.
    pub fn index(&self) -> Option<u32> {
        let name = self.name.as_deref()?;
        let digits = name.trim_start_matches(|c: char| !c.is_ascii_digit());
        digits.parse().ok()
    }
}

/// A float as v2 writes it: `%.16g` of the value as a double
/// (`0.1f32` → "0.1000000014901161").
pub fn format_g16(x: f64) -> String {
    if x == 0.0 || !x.is_finite() {
        return if x.is_nan() {
            "nan".into()
        } else if x.is_infinite() {
            if x > 0.0 { "inf".into() } else { "-inf".into() }
        } else {
            "0".into()
        };
    }
    let s = format!("{:.15e}", x);
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let trim = |t: String| -> String { if t.contains('.') { t.trim_end_matches('0').trim_end_matches('.').to_string() } else { t } };
    let body = if !(-4..16).contains(&exp) {
        let m = trim(format!("{}.{}", &digits[..1], &digits[1..]));
        format!("{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    } else if exp >= 0 {
        let e = exp as usize + 1;
        trim(format!("{}.{}", &digits[..e], &digits[e..]))
    } else {
        trim(format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits))
    };
    if neg { format!("-{body}") } else { body }
}

#[cfg(test)]
mod g16_tests {
    #[test]
    fn matches_v2_files() {
        use super::format_g16 as g;
        assert_eq!(g(0.1f32 as f64), "0.1000000014901161");
        assert_eq!(g(0.00562f32 as f64), "0.005619999952614307");
        assert_eq!(g(0.7605903f32 as f64), "0.7605903148651123");
        assert_eq!(g(1.0), "1");
        assert_eq!(g(-0.25), "-0.25");
        assert_eq!(g(120.0), "120");
        assert_eq!(g(1e-5), "1e-05");
        assert_eq!(g(0.0001), "0.0001");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_reads_finite_numbers_only() {
        let n = Node::parse(r#"<a x="0.25" y="NaN" z="-inf" w="1e99" v=" 2" />"#).unwrap();
        assert_eq!((n.f32("x"), n.f32("y"), n.f32("z"), n.f32("w"), n.f32("missing")), (Some(0.25), None, None, None, None));
    }

    /// Multi-line text (descriptions, names) survives a save and reload.
    #[test]
    fn line_breaks_and_tabs_round_trip() {
        let mut n = Node::new("root", None);
        n.set("text", "one\ntwo\tthree\r\n<&\"");
        let back = Node::parse(&n.to_xml()).unwrap();
        assert_eq!(back.attr("text"), n.attr("text"));
    }
}
