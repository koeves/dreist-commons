//! Parses every v2 XML file found in the installed content and plugin
//! resources. Skips silently when v2 is not installed; override paths with
//! `DREIST_CONTENT` / `DREIST_RESOURCES`.

use std::path::{Path, PathBuf};

use dreist_formats::{Kit, Node, Project};

fn roots() -> Vec<PathBuf> {
    let content = dreist_formats::legacy::content_dir().to_string_lossy().into_owned();
    let resources = dreist_formats::paths::resources_dir();
    [PathBuf::from(content), resources].into_iter().filter(|p| p.exists()).collect()
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn parse_all_installed_files() {
    let mut all = Vec::new();
    for r in roots() {
        files(&r, &mut all);
    }
    let mut parsed = 0;
    for p in &all {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        match ext {
            "geist" => {
                let proj = Project::load(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                assert!(!proj.engines.is_empty(), "{}", p.display());
                for e in &proj.engines {
                    for pad in &e.pads {
                        for l in &pad.layers {
                            if let Some(f) = &l.audio_file {
                                let s = dreist_formats::model::samples_dir(p).join(f);
                                assert!(s.exists(), "missing sample {}", s.display());
                            }
                        }
                    }
                }
            }
            "geistkit" => {
                Kit::load(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            }
            "gsa" | "fxpreset" | "fxchain" | "patterntemplate" | "swingtemplate" | "skin" | "stylesheet" => {
                let text = std::fs::read_to_string(p).unwrap();
                Node::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            }
            _ => continue,
        }
        parsed += 1;
    }
    eprintln!("parsed {parsed} files");
}

/// Every numeric Layer/Pad attribute in installed projects and kits is a known
/// parameter or `<Param>_<TransmodSource>` depth (`Cutoff`/`Resonance` are
/// the stored names of FilterCutoff/FilterResonance).
#[test]
fn layer_and_pad_attributes_are_known_parameters() {
    use dreist_formats::{LayerParam, PadParam, TransmodSource};
    const NON_PARAM: &[&str] = &["AudioFileName", "EnvTracksPitch", "CutMode", "PlaybackMode", "SynthesisMode", "IsReversed", "PhasePosition"];
    fn known(class: &str, base: &str) -> bool {
        match class {
            "Layer" => LayerParam::from_name(base).is_some(),
            _ => PadParam::from_name(base).is_some(),
        }
    }
    fn walk(n: &Node, bad: &mut Vec<String>) {
        if n.class == "Layer" || n.class == "Pad" {
            for (k, _) in &n.attrs {
                let (base, src) = k.split_once('_').map_or((k.as_str(), None), |(b, s)| (b, Some(s)));
                let base_ok = NON_PARAM.contains(&base) || known(&n.class, base);
                let src_ok = src.is_none_or(|s| TransmodSource::from_name(s).is_some());
                if !(base_ok && src_ok) {
                    bad.push(format!("{}.{k}", n.class));
                }
            }
        }
        n.children.iter().for_each(|c| walk(c, bad));
    }
    let mut all = Vec::new();
    for r in roots() {
        files(&r, &mut all);
    }
    let mut bad = Vec::new();
    for p in all.iter().filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("geist" | "geistkit"))) {
        walk(&Node::parse(&std::fs::read_to_string(p).unwrap()).unwrap(), &mut bad);
    }
    bad.sort();
    bad.dedup();
    assert!(bad.is_empty(), "unknown attributes: {bad:?}");
}

/// Step, Engine and Effect attributes in installed content are all known.
#[test]
fn step_engine_effect_attributes_are_known() {
    use dreist_formats::params::{effect_type_index, step_lane_index};
    use dreist_formats::{EngineParam, TransmodSource};
    fn walk(n: &Node, bad: &mut Vec<String>) {
        for (k, v) in &n.attrs {
            let (base, src) = k.split_once('_').map_or((k.as_str(), None), |(b, s)| (b, Some(s)));
            // Factory .fxpreset files also carry depths for sources the original does not
            // have (EngineGraph1-4, GlobalSlot1-10, Macro5-10); the loader ignores them.
            let src_ok = n.class == "Effect" || src.is_none_or(|s| TransmodSource::from_name(s).is_some());
            let ok = match n.class.as_str() {
                "Step" => base == "State" || step_lane_index(base).is_some(),
                "Engine" => EngineParam::from_name(base).is_some() || base.starts_with("AuxSend") || matches!(base, "Name" | "KitName" | "EnginePadMode"),
                "Effect" if k == "EffectType" => effect_type_index(v).is_some(),
                _ => true,
            };
            if !(ok && src_ok) {
                bad.push(format!("{}.{k}", n.class));
            }
        }
        n.children.iter().for_each(|c| walk(c, bad));
    }
    let mut all = Vec::new();
    for r in roots() {
        files(&r, &mut all);
    }
    let mut bad = Vec::new();
    for p in all.iter().filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("geist" | "geistkit" | "fxpreset" | "fxchain"))) {
        walk(&Node::parse(&std::fs::read_to_string(p).unwrap()).unwrap(), &mut bad);
    }
    bad.sort();
    bad.dedup();
    assert!(bad.is_empty(), "unknown attributes: {bad:?}");
}

/// `Layer::depths` (one pass over the attributes) reads every modulation
/// depth exactly as `Layer::depth` does, for every layer of every project
/// and kit installed.
#[test]
fn layer_depths_match_depth() {
    use dreist_formats::params::{LayerParam, TransmodSource};
    let mut all = Vec::new();
    for r in roots() {
        files(&r, &mut all);
    }
    let mut layers = Vec::new();
    for p in &all {
        match p.extension().and_then(|e| e.to_str()) {
            Some("geist") => layers.extend(Project::load(p).into_iter().flat_map(|pr| pr.engines).flat_map(|e| e.pads).flat_map(|p| p.layers)),
            Some("geistkit") => layers.extend(Kit::load(p).into_iter().flat_map(|k| k.pads).flat_map(|p| p.layers)),
            _ => {}
        }
    }
    let mut probe = layers.first().cloned();
    if let Some(l) = probe.as_mut() {
        // Repeated, unparsable and out-of-range depths.
        for (k, v) in [("Gain_Velocity", "x"), ("Gain_Velocity", "0.5"), ("Pan_Random", "-3"), ("Pan_ActualParameter", "0.2"), ("Bogus_Velocity", "1")] {
            l.raw.attrs.push((k.into(), v.into()));
        }
    }
    for l in layers.iter().chain(&probe) {
        let mut m = vec![[0.0f32; 29]; LayerParam::ALL.len()];
        l.depths(|p, s, v| m[p as usize][s as usize] = v);
        for p in LayerParam::ALL {
            for s in &TransmodSource::ALL[1..] {
                assert_eq!(m[*p as usize][*s as usize].to_bits(), l.depth(*p, *s).to_bits(), "{p:?} {s:?}");
            }
        }
    }
}
