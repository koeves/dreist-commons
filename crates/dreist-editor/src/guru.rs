//! Guru imports ([re:73eb4f], see the dreist-re Guru notes): the
//! original Guru's XML files converted into the document — `.G01` patterns, `.G24`
//! pattern banks, `.KIT` kits, `.GRU` presets — and Guru 1 text swing
//! templates ([re:0a45b1]).

use std::path::{Path, PathBuf};

use dreist_formats::Node;

/// What a Guru file holds ([re:3c6863]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pattern,
    Bank,
    Kit,
    Preset,
}

pub fn kind(path: &Path) -> Option<Kind> {
    let e = path.extension()?.to_str()?.to_ascii_uppercase();
    Some(match e.as_str() {
        "G01" => Kind::Pattern,
        "G24" => Kind::Bank,
        "KIT" => Kind::Kit,
        "GRU" => Kind::Preset,
        _ => return None,
    })
}

const CORRUPT: &str = "Failed to open Guru file (corrupt xml - Please check its a newer GURU format, not v1 binary format)";

fn load_root(path: &Path) -> Result<Node, String> {
    let text = std::fs::read_to_string(path).map_err(|_| CORRUPT.to_string())?;
    Node::parse(&text).map_err(|_| CORRUPT.to_string())
}

/// A child by instance name ([re:55206e]).
fn child<'a>(n: &'a Node, name: &str) -> Option<&'a Node> {
    n.children.iter().find(|c| c.name.as_deref() == Some(name))
}

fn f(n: Option<&Node>, key: &str, default: f32) -> f32 {
    n.and_then(|n| n.f32(key)).unwrap_or(default)
}

fn int(n: Option<&Node>, key: &str, default: i64) -> i64 {
    n.and_then(|n| n.attr(key)).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

fn is_true(n: Option<&Node>, key: &str) -> bool {
    n.and_then(|n| n.attr(key)).is_some_and(|v| v.trim() == "true")
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Guru levels (layer Gain, pad Level, engine Volume): 2g below 0.5, else
/// (g − 0.5)·6 + 1, then cbrt(·/4).
pub fn level(g: f32) -> f32 {
    let g = if g < 0.5 { g + g } else { (g - 0.5) * 6.0 + 1.0 };
    clamp01((g * 0.25).cbrt())
}

/// [re:7ab03d] (±12 semitones).
fn pitch_norm(st: f64) -> f32 {
    ((st + 12.0) / 24.0) as f32
}

/// [re:e3a2d2] (±4 octaves).
fn octave_norm(o: f64) -> f32 {
    ((o + 4.0) / 8.0) as f32
}

/// [re:f50fdf] → a `StepPattern` node ([re:78d175]).
pub fn convert_pattern(c: Option<&Node>, slot: usize) -> Node {
    let mut sp = Node::new("StepPattern", Some(&format!("StepPattern{slot}")));
    let pages = int(c, "Pages", 1);
    let steps_attr = int(c, "Steps", 1);
    let total = steps_attr * pages;
    let steps = if total == 0 { 16 } else { total.clamp(1, 1024) } as usize;
    let res = (int(c, "Resolution", 0) + 6).clamp(0, 13) as usize;
    let sync = int(c, "Sync", 0).clamp(0, 3) as usize;
    sp.set("PatternSteps", steps);
    sp.set("Resolution", dreist_engine::pattern::RESOLUTION_NAMES[res]);
    sp.set("SyncMode", dreist_engine::pattern::SYNC_MODE_NAMES[sync]);
    let name: String = c.and_then(|c| c.attr("Name")).unwrap_or("").chars().take(63).collect();
    let mut label = Node::new("FixedString", Some("PatternName"));
    label.set("string", name);
    sp.children.push(label);
    for t in 0..16 {
        let ct = c.and_then(|c| child(c, &format!("Track{t}")));
        let mut track = Node::new("Track", Some(&format!("Track{t}")));
        if let Some(ct) = ct {
            let count = match ct.attr("Steps").and_then(|v| v.trim().parse::<i64>().ok()) {
                None => 0,
                Some(n) if n < 0 => 16,
                Some(n) => n,
            };
            for i in 0..count.max(0) as usize {
                let Some(cs) = child(ct, &format!("Step{i}")) else { continue };
                // Several pages: children come in pages of 32.
                let at = if pages == 1 { i } else { (i & 31) + steps_attr.max(0) as usize * (i >> 5) };
                if at >= steps {
                    continue;
                }
                track.children.push(convert_step(cs, at));
            }
        }
        sp.children.push(track);
    }
    sp
}

/// One [re:d7fbce] (defaults when an attribute is missing).
fn convert_step(cs: &Node, at: usize) -> Node {
    let n = Some(cs);
    let mut s = Node::new("Step", Some(&format!("Step{at}")));
    s.set("State", if cs.attr("State").map(str::trim) == Some("1") { "true" } else { "false" });
    let rpt = clamp01(f(n, "Rpt", 0.0));
    let count = if rpt < 0.04 {
        1
    } else if rpt <= 0.11 {
        2
    } else if rpt <= 0.16 {
        3
    } else if rpt <= 0.2 {
        4
    } else if rpt <= 0.24 {
        5
    } else {
        (((rpt - 0.24) * 82.08 + 6.0) as i32).min(8)
    };
    let frc = clamp01(f(n, "Frc", 0.0));
    let layer = (frc * 10.0 + 0.5).floor() as i32 - 1;
    let layer = if (0..8).contains(&layer) { layer as usize } else { 0 };
    let lanes = [
        ("Velocity", clamp01(f(n, "Vol", 1.0))),
        ("Pan", clamp01(f(n, "Pan", 0.5))),
        ("Repeats", clamp01((count as f32 - 1.0) * 0.14285715)),
        ("Shift", clamp01(clamp01(f(n, "Shi", 0.5)) * 0.5 + 0.25)),
        ("FilterCutoff", clamp01(f(n, "FFr", 0.5))),
        ("FilterResonance", clamp01(f(n, "Res", 0.5))),
        ("CoarsePitch", clamp01(f(n, "Pit", 0.5))),
        ("FinePitch", clamp01(f(n, "Fin", 0.5))),
        ("StartPoint", clamp01(f(n, "Scr", 0.5))),
        ("Reverse", clamp01(f(n, "F10", 0.0))),
        ("ForceLayer", dreist_dsp::slice::layer_index_value(layer)),
    ];
    for (k, v) in lanes {
        s.set(k, v);
    }
    s
}

/// [re:7dc2a1] → layer node attributes ([re:fa1c16]). `cut_by`
/// is the pad's Guru `CutBy`; `guru` the imported file (its folder holds
/// the samples).
fn convert_layer(cl: &Node, layer: &mut Node, cut_by: i64, guru: &Path) {
    let n = Some(cl);
    layer.set("LayerGain", level(f(n, "Gain", 0.5)));
    layer.set("LayerPan", clamp01(f(n, "Pan", 0.5)));
    let mut st = (f(n, "Tune", 0.5) as f64 * 48.0 - 24.0).round();
    if st >= 13.0 {
        st -= 12.0;
        layer.set("Octave", octave_norm(1.0));
    } else if st <= -13.0 {
        st += 12.0;
        layer.set("Octave", octave_norm(-1.0));
    }
    layer.set("CoarseTune", pitch_norm(st));
    layer.set("FineTune", clamp01(f(n, "Fine", 0.5)));
    layer.set("Cutoff", clamp01(f(n, "Cutoff", 1.0)));
    layer.set("Resonance", clamp01(f(n, "Resonance", 0.0)));
    layer.set("FilterType", clamp01(f(n, "FilterType", 0.0)));
    layer.set("FilterActive", 1.0);
    layer.set("FilterAlgorithm", 0.0);
    layer.set("Mute", if f(n, "LayerMute", 0.0) != 0.0 { "true" } else { "false" });
    layer.set("Solo", if f(n, "LayerSolo", 0.0) != 0.0 { "true" } else { "false" });
    if is_true(n, "Lock") {
        layer.set("LockState", "true");
    }
    layer.set("PhasePosition", if f(n, "PhaseFlip", 0.0) != 0.0 { -1.0 } else { 1.0 });
    for (i, k) in ["AuxSend1", "AuxSend2", "AuxSend3"].iter().enumerate() {
        layer.set(&dreist_formats::params::aux_send_name(i), clamp01(f(n, k, 0.0)));
    }
    let (a, h, r) = (clamp01(f(n, "AmpEnvAttack", 0.0)), clamp01(f(n, "AmpEnvHold", 1.0)), clamp01(f(n, "AmpEnvRelease", 0.0)));
    let (fa, fh, fr) = (clamp01(f(n, "FxEnvAttack", 0.0)), clamp01(f(n, "FxEnvHold", 1.0)), clamp01(f(n, "FxEnvRelease", 0.0)));
    for (k, v) in
        [("AmpEnvAttack", a), ("AmpEnvDecay", 0.0), ("AmpEnvSustain", 1.0), ("AmpEnvRelease", r), ("FreeEnvAttack", fa), ("FreeEnvDecay", 0.0), ("FreeEnvSustain", 1.0), ("FreeEnvRelease", fr)]
    {
        layer.set(k, v);
    }
    layer.set("Cutoff_FreeEnvelope", clamp01(f(n, "FxEnvCutoff", 0.5)) * 2.0 - 1.0);
    layer.set("CoarseTune_FreeEnvelope", clamp01(f(n, "FxEnvPitch", 0.5)) * 2.0 - 1.0);
    if (0..4).contains(&cut_by) {
        let cut = [0usize, 1, 5, 6][cut_by as usize];
        layer.set("CutMode", dreist_dsp::voices::CUT_MODE_LABELS[cut]);
    }
    layer.set("PlaybackMode", "Length bound");
    // The sample: next to the Guru file, else the stored path.
    let raw = cl.attr("Path").unwrap_or("").replace('\\', "/");
    if raw.is_empty() {
        return;
    }
    let near = guru.parent().map(|d| d.join(&raw)).unwrap_or_else(|| PathBuf::from(&raw));
    let file = if near.exists() { near } else { PathBuf::from(&raw) };
    let (start, end) = (cl.f32("StartSample").unwrap_or(0.0) as i64, cl.f32("EndSample").unwrap_or(0.0) as i64);
    layer.set("AudioFileName", file.to_string_lossy());
    let len = dreist_engine::audio::info(&file);
    let mut markers = Node::new("AudioFileMarkers", Some("StartEndMarkers"));
    markers.set("StartSample", start.max(0));
    markers.set("EndSample", end.max(0));
    markers.set("SampleLength", len.map(|l| l.0).unwrap_or(end.max(0) as u64));
    layer.children.retain(|c| !(c.class == "AudioFileMarkers" && c.name.as_deref() == Some("StartEndMarkers")));
    layer.children.push(markers);
    // A file found: envelope times re-derived from the region length.
    let Some((_, sr)) = len else { return };
    let region = (end - start) as f32;
    let scale = |x: f32, y: f32, z: f32| if x + y + z > 1.0 { 1.0 / (x + y + z) } else { 1.0 };
    let (sa, sf) = (scale(a, h, r) * region, scale(fa, fh, fr) * region);
    let q = |frames: f32, secs: f32| clamp01((frames / (sr * secs)).min(1.0).powf(0.25));
    for (k, v) in [
        ("AmpEnvAttack", q(a * sa, 10.0)),
        ("AmpEnvHold", q(h * sa, 60.0)),
        ("AmpEnvRelease", q(r * sa, 15.0)),
        ("FreeEnvAttack", q(fa * sf, 10.0)),
        ("FreeEnvHold", q(fh * sf, 60.0)),
        ("FreeEnvRelease", q(fr * sf, 15.0)),
    ] {
        layer.set(k, v);
    }
}

/// [re:b5da8f] → pad node ([re:34b283]).
fn convert_pad(cp: Option<&Node>, pad: &mut Node, guru: &Path) {
    pad.set("Gain", level(f(cp, "Level", 0.7937)));
    pad.set("Mute", if f(cp, "Mute", 0.0) != 0.0 { "true" } else { "false" });
    pad.set("Solo", if f(cp, "Solo", 0.0) != 0.0 { "true" } else { "false" });
    if is_true(cp, "Lock") {
        pad.set("LockState", "true");
    }
    let cut_by = int(cp, "CutBy", 0);
    for l in 0..8 {
        let Some(cl) = cp.and_then(|c| child(c, &format!("Layer{l}"))) else { continue };
        if cl.attr("Path").is_none_or(|p| p.is_empty()) {
            continue;
        }
        let name = format!("Layer{l}");
        pad.children.retain(|c| !(c.class == "Layer" && c.name.as_deref() == Some(name.as_str())));
        let mut layer = Node::new("Layer", Some(&name));
        convert_layer(cl, &mut layer, cut_by, guru);
        pad.children.push(layer);
    }
    let split = int(cp, "UsingVelocitySplit", 0) == 1;
    pad.set("PadMode", dreist_dsp::voices::LAYER_MODE_NAMES[split as usize]);
    if split {
        let valid: Vec<bool> =
            (0..8).map(|l| pad.children.iter().any(|c| c.class == "Layer" && c.name.as_deref() == Some(&format!("Layer{l}")) && c.attr("AudioFileName").is_some_and(|f| !f.is_empty()))).collect();
        let mut splits: Vec<dreist_engine::velsplit::Split> = (0..8)
            .map(|l| pad.children.iter().find(|c| c.class == "Layer" && c.name.as_deref() == Some(&format!("Layer{l}"))).map(dreist_engine::engine::doc::layer_split).unwrap_or_default())
            .collect();
        dreist_engine::velsplit::auto_split(&valid, &mut splits);
        for (l, s) in splits.into_iter().enumerate() {
            if valid[l] {
                dreist_engine::engine::doc::set_layer_split(pad.child_mut_or_insert("Layer", &format!("Layer{l}")), s);
            }
        }
    }
}

/// [re:976fb1] → engine node ([re:26a8d5]); patterns only for
/// presets.
fn convert_engine(ce: Option<&Node>, eng: &mut Node, guru: &Path, with_patterns: bool) {
    eng.set("TempoNumerator", f(ce, "Numerator", 1.0) as i64);
    eng.set("TempoDenominator", f(ce, "Denominator", 1.0) as i64);
    eng.set("Solo", if is_true(ce, "Solo") { "true" } else { "false" });
    eng.set("Mute", if is_true(ce, "Mute") { "true" } else { "false" });
    eng.set("SwingTemplateDepth", clamp01(f(ce, "GrooveTime", 0.0)));
    eng.set("Gain", level(f(ce, "Volume", 0.7937)));
    eng.set("Pan", clamp01(f(ce, "Pan", 0.5)));
    let p = f(ce, "Pitch", 0.5) as f64;
    let st = if p > 0.0 {
        p.min(12.0).floor()
    } else if p < 0.0 {
        p.max(-12.0).ceil()
    } else {
        p
    };
    eng.set("Pitch", clamp01(pitch_norm(st)));
    eng.children.retain(|c| c.class != "SwingTemplate");
    eng.children.push(convert_groove(ce.and_then(|c| child(c, "Groove"))));
    for p in 0..16 {
        let cp = ce.and_then(|c| child(c, &format!("Pad{p}")));
        convert_pad(cp, eng.child_mut_or_insert("Pad", &format!("Pad{p}")), guru);
    }
    if with_patterns {
        for s in 0..24 {
            let name = format!("StepPattern{s}");
            eng.children.retain(|c| !(c.class == "StepPattern" && c.name.as_deref() == Some(name.as_str())));
            eng.children.push(convert_pattern(ce.and_then(|c| child(c, &format!("Pattern{s}"))), s));
        }
    }
}

/// [re:190791] (Steps 1..128, Name "Guru ST", `Step%d` `Mov0`) → a swing
/// template ([re:fd40bc]).
fn convert_groove(g: Option<&Node>) -> Node {
    let steps = int(g, "Steps", 1).clamp(1, 128);
    let mut n = Node::new("SwingTemplate", Some("SwingTemplate"));
    n.set("Ver", 2);
    n.set("NumberOfSteps", steps);
    for i in 0..128 {
        let v = f(g.and_then(|g| child(g, &format!("Step{i}"))), "Mov0", 0.0);
        if v != 0.0 {
            n.set(&format!("S{i}"), v);
        }
    }
    let mut label = Node::new("FixedString", Some("SwingTemplateName"));
    label.set("string", g.and_then(|g| g.attr("Name")).unwrap_or("Guru ST"));
    n.children.push(label);
    n
}

/// Guru pattern: the file's `Pattern0` into slot `slot`
/// (locked patterns stay).
pub fn import_pattern(doc: &mut Node, e: usize, slot: usize, path: &Path) -> Result<(), String> {
    let root = load_root(path)?;
    let c = child(&root, "Pattern0").ok_or("Failed to restore guru pattern from XML")?;
    let eng = doc.child_mut_or_insert("Engine", &format!("Engine{e}"));
    let target = eng.child_mut_or_insert("StepPattern", &format!("StepPattern{slot}"));
    if !target.locked() {
        *target = convert_pattern(Some(c), slot);
    }
    Ok(())
}

/// Guru pattern bank: `Pattern0`..`Pattern23` (missing ones
/// skipped, locked slots kept).
pub fn import_bank(doc: &mut Node, e: usize, path: &Path) -> Result<(), String> {
    let root = load_root(path)?;
    let eng = doc.child_mut_or_insert("Engine", &format!("Engine{e}"));
    for s in 0..24 {
        let Some(c) = child(&root, &format!("Pattern{s}")) else { continue };
        let target = eng.child_mut_or_insert("StepPattern", &format!("StepPattern{s}"));
        if !target.locked() {
            *target = convert_pattern(Some(c), s);
        }
    }
    Ok(())
}

/// Guru kit: the current engine with its pads 0..15 converted
/// from the file's `Engine` (locked pads and layers keep the old ones),
/// kit name = file stem.
pub fn import_kit(doc: &mut Node, e: usize, path: &Path) -> Result<(), String> {
    let root = load_root(path)?;
    let ce = child(&root, "Engine").ok_or("Failed to restore guru engine from XML")?;
    let old = doc.child_mut_or_insert("Engine", &format!("Engine{e}")).clone();
    let mut eng = old.clone();
    convert_engine(Some(ce), &mut eng, path, false);
    // Locked pads / layers come back from the old engine.
    for p in 0..64 {
        let pn = format!("Pad{p}");
        let Some(op) = old.children.iter().find(|c| c.class == "Pad" && c.name.as_deref() == Some(pn.as_str())) else { continue };
        if op.locked() {
            *eng.child_mut_or_insert("Pad", &pn) = op.clone();
            continue;
        }
        for l in 0..8 {
            let ln = format!("Layer{l}");
            if let Some(ol) = op.children.iter().find(|c| c.class == "Layer" && c.name.as_deref() == Some(ln.as_str())).filter(|c| c.locked()) {
                *eng.child_mut_or_insert("Pad", &pn).child_mut_or_insert("Layer", &ln) = ol.clone();
            }
        }
    }
    let stem: String = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default().chars().take(63).collect();
    eng.set("KitName", stem);
    *doc.child_mut_or_insert("Engine", &format!("Engine{e}")) = eng;
    Ok(())
}

/// Guru preset: a new document — tempo `BPM` (120),
/// master tune `MstrPtc` (semitones), master level cbrt(`MstrLvl`) (0.5),
/// engines `Engine1`..`Engine8` with their patterns. The scenes end up
/// empty: the scene import resets each scene it restores.
pub fn import_preset(path: &Path) -> Result<Node, String> {
    let root = load_root(path)?;
    let mut doc = crate::project::new_document();
    let bpm = root.f32("BPM").unwrap_or(120.0);
    let tune = pitch_norm(root.f32("MstrPtc").unwrap_or(0.0) as f64);
    let lvl = root.f32("MstrLvl").unwrap_or(0.5).cbrt();
    doc.set("BPM", bpm);
    let master = doc.child_mut_or_insert("MasterChannel", "MasterChannel");
    master.set("Gain", lvl);
    master.set("Pitch", tune);
    for e in 0..8 {
        let Some(ce) = child(&root, &format!("Engine{}", e + 1)) else { continue };
        let eng = doc.child_mut_or_insert("Engine", &format!("Engine{e}"));
        convert_engine(Some(ce), eng, path, true);
    }
    Ok(doc)
}

/// Guru 1 swing template text
/// ([re:5070ca]): a "VERSION" line is required; "MAX=" gives the step count
/// (default 32); each "STEP=" line's value is the 6 characters from
/// column 9 (at most 128 steps). The template is named after the file.
pub fn convert_swing_text(path: &Path) -> Result<Node, String> {
    let text = std::fs::read_to_string(path).map_err(|_| format!("Failed to open the Guru file on path {}", path.display()))?;
    let (mut version, mut max, mut values) = (false, 32usize, Vec::new());
    for line in text.lines() {
        if line.contains("VERSION") {
            version = true;
            continue;
        }
        if line.contains("MAX=") {
            max = line.get(4..).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(0).max(0) as usize;
        }
        if line.contains("STEP=") {
            let v = line.get(9..(9 + 6).min(line.len())).and_then(crate::conv::number64).unwrap_or(0.0);
            values.push(v);
            if values.len() >= 128 {
                break;
            }
        }
    }
    if !version {
        return Err("This doesnt appear to be a valid Guru swing template".into());
    }
    let mut n = Node::new("SwingTemplate", Some("SwingTemplate"));
    n.set("Ver", 2);
    n.set("NumberOfSteps", max.min(128));
    for (i, v) in values.iter().enumerate().take(max.min(128)) {
        if *v != 0.0 {
            n.set(&format!("S{i}"), v);
        }
    }
    let mut label = Node::new("FixedString", Some("SwingTemplateName"));
    label.set("string", path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    n.children.push(label);
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guru_pattern_steps_and_lanes() {
        let xml = r#"<root><CPattern name="Pattern0" Resolution="2" Sync="1" Name="Groove" Steps="16" Pages="1">
            <CTrack name="Track0" Steps="2"><CStep name="Step0" State="1" Vol="0.8" Rpt="0.15" Shi="0.5" Frc="0.3"/><CStep name="Step1" State="0"/></CTrack>
        </CPattern></root>"#;
        let dir = std::env::temp_dir().join(format!("dreist-guru-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.G01");
        std::fs::write(&p, xml).unwrap();
        let mut doc = crate::project::new_document();
        import_pattern(&mut doc, 0, 3, &p).unwrap();
        let eng = doc.children.iter().find(|c| c.name.as_deref() == Some("Engine0")).unwrap();
        let sp = eng.children.iter().find(|c| c.name.as_deref() == Some("StepPattern3")).unwrap();
        let pd = dreist_engine::pattern::PatternData::from_node(sp);
        assert_eq!(pd.steps, 16);
        assert_eq!(dreist_engine::pattern::RESOLUTION_NAMES[pd.resolution], "1/16");
        let s = &pd.tracks[0].steps[0];
        assert!(s.on && (s.lanes[0] - 0.8).abs() < 1e-6);
        // Rpt 0.15 → 3 repeats → 2/7.
        assert!((s.lanes[2] - 2.0 / 7.0).abs() < 1e-5);
        assert!((s.lanes[3] - 0.5).abs() < 1e-6);
        assert!((s.lanes[15] - dreist_dsp::slice::layer_index_value(2)).abs() < 1e-6);
        assert!(!pd.tracks[0].steps[1].on);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn guru_kit_preset_and_swing() {
        let dir = std::env::temp_dir().join(format!("dreist-guru-kit-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Samples")).unwrap();
        let wav = dir.join("Samples/kick.wav");
        dreist_engine::export::write_wav24(&wav, 44100, &vec![0.1; 44100], &vec![0.1; 44100]).unwrap();
        let kit = r#"<root><CEngine name="Engine" Volume="0.5">
            <CPad name="Pad0" Level="0.5" CutBy="2" UsingVelocitySplit="1">
              <CLayer name="Layer0" Path="Samples/kick.wav" Name="kick" Tune="0.75" StartSample="0" EndSample="44100" AmpEnvAttack="0.1" AmpEnvHold="0.5" AmpEnvRelease="0.2" PhaseFlip="1"/>
            </CPad></CEngine></root>"#;
        let kp = dir.join("Test.KIT");
        std::fs::write(&kp, kit).unwrap();
        let mut doc = crate::project::new_document();
        import_kit(&mut doc, 0, &kp).unwrap();
        let m = crate::model::Model::new(doc);
        let l = m.layer_node(0, 0, 0).unwrap();
        assert_eq!(l.attr("AudioFileName").map(std::path::PathBuf::from), Some(wav.clone()));
        assert_eq!(l.attr("CutMode"), Some("Pad type"));
        assert_eq!(l.f32("PhasePosition"), Some(-1.0));
        // Tune 0.75 → +12 st.
        assert_eq!(l.f32("CoarseTune"), Some(1.0));
        // Attack: 0.1 of 44100 frames (a + h + r ≤ 1: unscaled) → (4410 / 441000)^¼.
        let want = (0.1f32 * 44100.0 / 441000.0).powf(0.25);
        assert!((l.f32("AmpEnvAttack").unwrap() - want).abs() < 1e-4);
        assert_eq!(m.pad_node(0, 0).and_then(|p| p.attr("PadMode")), Some("Velocity split"));
        assert_eq!(m.engine_node(0).and_then(|e| e.attr("KitName")), Some("Test"));
        let preset = r#"<root BPM="96" MstrPtc="0"><CEngine name="Engine1"><CPattern name="Pattern2" Steps="8"/></CEngine></root>"#;
        let pp = dir.join("Song.GRU");
        std::fs::write(&pp, preset).unwrap();
        let d = import_preset(&pp).unwrap();
        assert_eq!(d.f32("BPM"), Some(96.0));
        let m = crate::model::Model::new(d);
        assert_eq!(m.pattern(0, 2).steps, 8);
        let st = dir.join("Shuffle.txt");
        std::fs::write(&st, "VERSION 1\nMAX=2\nSTEP=001 0.2500\nSTEP=002 0.0000\n").unwrap();
        let t = convert_swing_text(&st).unwrap();
        assert_eq!(t.f32("NumberOfSteps"), Some(2.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn levels_and_tuning() {
        assert!((level(0.5) - 0.25f32.cbrt()).abs() < 1e-6);
        assert!((level(0.25) - 0.125f32.cbrt()).abs() < 1e-6);
        assert_eq!(pitch_norm(0.0), 0.5);
        assert_eq!(octave_norm(1.0), 0.625);
    }
}
