//! Swing templates ([re:9f001e], `.swingtemplate` files): the toolbar
//! swing menu ([re:86142f]: user
//! folder then factory folder, recursive; [re:c0a2f3]
//!  [re:031034] loads one and posts 0x96) and "Extract
//! groove from pattern" ([re:e2e0a3]
//! [re:b8a8b3]).

use std::path::{Path, PathBuf};

use dreist_engine::pattern::PatternData;
use dreist_formats::Node;

/// The port's user template folder (the original's `Paths::
/// GetSwingTemplatePath(false)`).
pub fn user_dir() -> Option<PathBuf> {
    crate::options::settings_dir().map(|d| d.join("Swing Templates"))
}

/// Factory templates beside the skin folder (`<resources>/Swing
/// Templates`, see `dreist_formats::paths::skin_dir`).
pub fn factory_dir(resources: &Path) -> PathBuf {
    resources.parent().unwrap_or(resources).join("Swing Templates")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("swingtemplate")) {
            out.push(p);
        }
    }
}

/// User templates first, then factory ones.
pub fn template_paths(resources: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(u) = user_dir() {
        walk(&u, &mut v);
    }
    walk(&factory_dir(resources), &mut v);
    v
}

/// The [re:9f001e] node of a template file.
pub fn load(path: &Path) -> Option<Node> {
    let text = std::fs::read_to_string(path).ok()?;
    let root = Node::parse(&text).ok()?;
    if root.class == "SwingTemplate" {
        return Some(root);
    }
    root.children.into_iter().find(|c| c.class == "SwingTemplate")
}

/// "SwingTemplateName", else the file stem.
pub fn name_of(node: Option<&Node>, path: &Path) -> String {
    node.and_then(|n| n.children.iter().find(|c| c.name.as_deref() == Some("SwingTemplateName")))
        .and_then(|c| c.attr("string"))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
}

/// Pattern to swing template: up to 128 steps (longer patterns are
/// sampled every floor(steps / 128)); each value is the mean of
/// `2·Shift − 1` over the tracks whose step is on (0 when none).
pub fn from_pattern(pat: &PatternData, name: &str) -> Node {
    let steps = pat.steps.max(1) as usize;
    let count = steps.min(128);
    let stride = if steps > 128 { (steps as f32 / 128.0).floor() as usize } else { 1 };
    let mut n = Node::new("SwingTemplate", Some("SwingTemplate"));
    n.set("Ver", 2);
    n.set("NumberOfSteps", count);
    for i in 0..count {
        let at = i * stride;
        if at >= steps {
            break;
        }
        let (mut sum, mut k) = (0.0f64, 0u32);
        for t in &pat.tracks {
            if let Some(s) = t.steps.get(at).filter(|s| s.on) {
                sum += 2.0 * s.lanes[dreist_engine::pattern::lane::SHIFT] as f64 - 1.0;
                k += 1;
            }
        }
        let v = if k == 0 { 0.0 } else { sum / k as f64 };
        if v != 0.0 {
            n.set(&format!("S{i}"), v);
        }
    }
    let mut label = Node::new("FixedString", Some("SwingTemplateName"));
    label.set("string", name);
    n.children.push(label);
    n
}

/// Write a template file (`<root>` wrapper as the factory files).
pub fn save(path: &Path, template: &Node) -> std::io::Result<()> {
    let mut root = Node::new("root", None);
    root.children.push(template.clone());
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    dreist_formats::file::write_atomic(path, root.to_xml())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_templates_load() {
        let res = dreist_formats::paths::skin_dir();
        let paths = template_paths(&res);
        let Some(t) = paths.iter().find(|p| p.ends_with("Triplet Feel.swingtemplate")) else { return };
        let n = load(t).unwrap();
        assert_eq!(name_of(Some(&n), t), "Triplet Feel");
        let sw = dreist_engine::pattern::Swing::from_node(&n);
        assert_eq!(sw.steps, 4);
        assert!((sw.value(1) - 0.3).abs() < 1e-9);
    }

    #[test]
    fn groove_from_shift() {
        let mut pat = PatternData::empty();
        pat.steps = 4;
        for t in pat.tracks.iter_mut() {
            t.steps.resize(4, dreist_engine::pattern::StepData::off());
        }
        pat.tracks[0].steps[1].on = true;
        pat.tracks[0].steps[1].lanes[dreist_engine::pattern::lane::SHIFT] = 0.75;
        let n = from_pattern(&pat, "G");
        let sw = dreist_engine::pattern::Swing::from_node(&n);
        assert_eq!(sw.steps, 4);
        assert!((sw.value(1) - 0.5).abs() < 1e-9 && sw.value(0) == 0.0);
    }
}
