//! Saving and creating projects from the editor's document.
//!
//! A `.geist` file keeps its samples in `<stem>.samples/` with
//! `AudioFileName`s relative to it (backslash separated). Samples loaded
//! from elsewhere (absolute paths) or belonging to another project's
//! folder are copied in on save, as v2's "save with samples" does.

use std::path::{Path, PathBuf};

use dreist_formats::Node;

/// Rewrite every `AudioFileName` under `n` relative to `dest_samples`,
/// copying files that live elsewhere. `src_samples` resolves relative names.
fn collect(n: &mut Node, src_samples: Option<&Path>, dest_samples: &Path, errors: &mut Vec<String>) {
    if let Some((_, v)) = n.attrs.iter_mut().find(|(k, _)| k == "AudioFileName") {
        let rel = v.replace('\\', "/");
        let path = Path::new(&rel);
        let abs = if path.is_absolute() { path.to_path_buf() } else { src_samples.map(|s| dreist_formats::model::resolve_sample(s, &rel)).unwrap_or_else(|| path.to_path_buf()) };
        let new_rel: PathBuf = match abs.strip_prefix(dest_samples) {
            Ok(r) => r.to_path_buf(),
            Err(_) => {
                // Keep the relative layout when it came from a samples folder.
                let target_rel = if path.is_absolute() { PathBuf::from(abs.file_name().unwrap_or_default()) } else { path.to_path_buf() };
                // A REX ".truncated" name copies the file without it.
                let target = dreist_engine::rex::strip_truncated(&dest_samples.join(&target_rel)).0;
                let src = dreist_engine::rex::strip_truncated(&abs).0;
                if !target.exists() {
                    if let Some(parent) = target.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Err(e) = std::fs::copy(&src, &target) {
                        errors.push(format!("{}: {e}", src.display()));
                    }
                }
                target_rel
            }
        };
        *v = new_rel.to_string_lossy().replace('/', "\\");
    }
    for c in &mut n.children {
        collect(c, src_samples, dest_samples, errors);
    }
}

/// Write `doc` to `path`, collecting samples into `path`'s `.samples`
/// folder. Returns the saved document (with the rewritten sample paths) and
/// any copy errors.
pub fn save(doc: &Node, src_samples: Option<&Path>, path: &Path) -> std::io::Result<(Node, Vec<String>)> {
    let dest = dreist_formats::model::samples_dir(path);
    let mut out = doc.clone();
    let mut errors = Vec::new();
    collect(&mut out, src_samples, &dest, &mut errors);
    dreist_formats::file::write_atomic(path, out.to_xml())?;
    Ok((out, errors))
}

/// The parts of a project that have their own files ([re:4196af] File
/// menu; [re:57f674], `SerializeCurrentKit`
/// [re:394e94], `SerializeCurrentPad` [re:25e3af], `SerializeCurrentPatternBank`
/// [re:1b98ba], `SerializeCurrentPattern` [re:b14536], `SerializeBindingMap`
/// [re:65fec7]). Each file is `<root>` holding the object(s) under their class
/// name (`Engine`, `Pad`, `StepPattern`, `BindingMap`; a kit is `Pad0..63`,
/// a bank `StepPattern0..23`), default attributes removed, samples kept in
/// `<stem>.samples/` like a project's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Engine(usize),
    Kit(usize),
    Pad(usize, usize),
    Bank(usize),
    Pattern(usize, usize),
    Mappings,
}

impl Part {
    /// (extension, filter description, noun) as [re:0b6aaa]
    /// 0x16c.. passes them to `Load` / `Save`.
    /// The extension is the app's own (saving); loading takes every family
    /// (`dreist_formats::profile::extensions_of`).
    pub fn file_type(self) -> (&'static str, &'static str, &'static str) {
        let (kind, filter, noun) = self.doc_kind();
        (dreist_formats::profile::get().save.of(kind), filter, noun)
    }

    /// The document kind, filter description and noun.
    pub fn doc_kind(self) -> (dreist_formats::profile::DocKind, &'static str, &'static str) {
        use dreist_formats::profile::DocKind;
        match self {
            Part::Engine(_) => (DocKind::Engine, "Engine files", "engine"),
            Part::Kit(_) => (DocKind::Kit, "Kit files", "kit"),
            Part::Pad(..) => (DocKind::Pad, "Pad files", "pad"),
            Part::Bank(_) => (DocKind::Bank, "Pattern bank files", "pattern bank"),
            Part::Pattern(..) => (DocKind::Pattern, "Pattern files", "pattern"),
            Part::Mappings => (DocKind::Binding, "Mapping files", "mapping"),
        }
    }
}

fn engine_of(doc: &Node, e: usize) -> Option<&Node> {
    doc.children.iter().find(|c| c.class == "Engine" && c.name.as_deref() == Some(&format!("Engine{e}")))
}

fn renamed(n: &Node, name: &str) -> Node {
    let mut n = n.clone();
    n.name = Some(name.to_string());
    n
}

/// The `<root>` written for `part`.
pub fn part_root(doc: &Node, part: Part) -> Node {
    let mut root = Node::new("root", None);
    let child = |e: usize, class: &str, name: &str| engine_of(doc, e).and_then(|n| n.children.iter().find(|c| c.class == class && c.name.as_deref() == Some(name))).cloned();
    match part {
        Part::Engine(e) => root.children.extend(engine_of(doc, e).map(|n| renamed(n, "Engine"))),
        Part::Kit(e) => root.children.extend((0..64).filter_map(|p| child(e, "Pad", &format!("Pad{p}")))),
        Part::Pad(e, p) => root.children.extend(child(e, "Pad", &format!("Pad{p}")).map(|n| renamed(&n, "Pad"))),
        Part::Bank(e) => root.children.extend((0..24).filter_map(|p| child(e, "StepPattern", &format!("StepPattern{p}")))),
        Part::Pattern(e, p) => root.children.extend(child(e, "StepPattern", &format!("StepPattern{p}")).map(|n| renamed(&n, "StepPattern"))),
        Part::Mappings => root.children.extend(doc.children.iter().find(|c| c.class == "BindingMap").map(|n| renamed(n, "BindingMap"))),
    }
    root
}

/// Write `part` of `doc` to `path`, its samples collected into `path`'s
/// `.samples` folder. Returns copy errors.
pub fn save_part(doc: &Node, src_samples: Option<&Path>, part: Part, path: &Path) -> std::io::Result<Vec<String>> {
    let mut root = part_root(doc, part);
    let mut errors = Vec::new();
    collect(&mut root, src_samples, &dreist_formats::model::samples_dir(path), &mut errors);
    dreist_formats::file::write_atomic(path, root.to_xml())?;
    Ok(errors)
}

/// Option `AllocatorRestoreOutputs` (default on): loads take the files'
/// `OutputLocation`s. Off, the objects keep their routing
/// ([re:7decb8] reads the option;
/// [re:d12ebb] skips the attribute).
pub fn restore_outputs() -> bool {
    crate::options::Options::load().bool("AllocatorRestoreOutputs", true)
}

/// Give every node of `new` the `OutputLocation` of the node at the same
/// place in `old` (none: the default, Parent).
pub fn keep_outputs(old: &Node, new: &mut Node) {
    match old.attr("OutputLocation") {
        Some(v) => {
            new.set("OutputLocation", v.to_string());
        }
        None => new.attrs.retain(|(k, _)| k != "OutputLocation"),
    }
    for c in &mut new.children {
        let empty = Node::new(&c.class, c.name.as_deref());
        let o = old.children.iter().find(|x| x.class == c.class && x.name == c.name).unwrap_or(&empty);
        keep_outputs(o, c);
    }
}

/// Load a part file into `doc` (samples made absolute against the file's
/// `.samples` folder). Engine loads keep the old engine's locked pads,
/// layers, patterns and tracks (`DoLoadEngineOnPath` $_16 [re:6463fe]); a pad
/// replaces the pad (`DoLoadPadOnPath` $_18 [re:7acfa5], 0x56); a bank or a
/// pattern replaces the unlocked patterns.
pub fn load_part(doc: &mut Node, samples: Option<&Path>, part: Part, path: &Path) -> bool {
    let before = (!restore_outputs()).then(|| doc.clone());
    let ok = load_part_inner(doc, samples, part, path);
    if let (true, Some(old)) = (ok, before) {
        keep_outputs(&old, doc);
    }
    ok
}

fn load_part_inner(doc: &mut Node, samples: Option<&Path>, part: Part, path: &Path) -> bool {
    let Some(mut root) = (dreist_formats::profile::get().read_document)(path) else { return false };
    crate::browser::absolutise(&mut root, &dreist_formats::model::samples_dir(path));
    let find = |root: &Node, class: &str, name: &str| root.children.iter().find(|c| c.class == class && c.name.as_deref() == Some(name)).cloned();
    match part {
        Part::Engine(e) => {
            let Some(new) = find(&root, "Engine", "Engine") else { return false };
            let old = doc.clone();
            let slot = doc.child_mut_or_insert("Engine", &format!("Engine{e}"));
            *slot = renamed(&new, &format!("Engine{e}"));
            crate::padmenu::carry_locked(&old, samples, doc);
        }
        Part::Kit(_) => return false,
        Part::Pad(e, p) => {
            let Some(new) = find(&root, "Pad", "Pad") else { return false };
            let eng = doc.child_mut_or_insert("Engine", &format!("Engine{e}"));
            *eng.child_mut_or_insert("Pad", &format!("Pad{p}")) = renamed(&new, &format!("Pad{p}"));
        }
        Part::Bank(e) => {
            let eng = doc.child_mut_or_insert("Engine", &format!("Engine{e}"));
            for i in 0..24 {
                let name = format!("StepPattern{i}");
                let Some(new) = find(&root, "StepPattern", &name) else { continue };
                let slot = eng.child_mut_or_insert("StepPattern", &name);
                if !slot.locked() {
                    *slot = new;
                }
            }
        }
        Part::Pattern(e, p) => {
            let Some(new) = find(&root, "StepPattern", "StepPattern") else { return false };
            let eng = doc.child_mut_or_insert("Engine", &format!("Engine{e}"));
            let slot = eng.child_mut_or_insert("StepPattern", &format!("StepPattern{p}"));
            if slot.locked() {
                return false;
            }
            *slot = renamed(&new, &format!("StepPattern{p}"));
        }
        Part::Mappings => {
            let Some(new) = find(&root, "BindingMap", "BindingMap") else { return false };
            doc.children.retain(|c| c.class != "BindingMap");
            doc.children.push(renamed(&new, "Bindings"));
        }
    }
    true
}

/// An empty project document.
pub fn new_document() -> Node {
    Node::parse("<root version=\"3000\" CurrentEngine=\"0\" />").expect("static XML")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_copies_foreign_samples() {
        let dir = std::env::temp_dir().join(format!("dreist-proj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("hit.wav");
        std::fs::write(&wav, b"RIFF").unwrap();
        let mut doc = new_document();
        let mut eng = Node::new("Engine", Some("Engine0"));
        let mut pad = Node::new("Pad", Some("Pad0"));
        let mut layer = Node::new("Layer", Some("Layer0"));
        layer.set("AudioFileName", wav.to_string_lossy());
        pad.children.push(layer);
        eng.children.push(pad);
        doc.children.push(eng);
        let out = dir.join("Song.geist");
        let (saved, errors) = save(&doc, None, &out).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(dir.join("Song.samples/hit.wav").exists());
        let l = &saved.children[0].children[0].children[0];
        assert_eq!(l.attr("AudioFileName"), Some("hit.wav"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
