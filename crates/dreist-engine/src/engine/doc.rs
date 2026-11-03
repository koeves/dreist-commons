//! The project document: the parsed `.geist` tree, kept in sync with edits
//! (messages, recording) and written back on save. Writing the tree again
//! reproduces installed files byte-for-byte (apart from line endings), so
//! untouched data survives exactly. See the dreist-re formats notes ("Saving").

use std::path::Path;

use super::Dreist;
use crate::binding::{Message, id};
use dreist_formats::params::{aux_send_name, effect_param_names, effect_type_index};
use dreist_formats::{EngineParam, LayerParam, Node, PadParam, TransmodSource};

/// Floats are stored as the f32 value printed at f64 precision (as v2 does).
fn fmt(v: f32) -> String {
    dreist_formats::node::format_g16(v as f64)
}

pub fn attr_name(param: &str, source: usize) -> Option<String> {
    let s = TransmodSource::ALL.get(source)?;
    Some(s.attr(param).unwrap_or_else(|| param.to_string()))
}

impl Dreist {
    pub fn document(&self) -> &Node {
        &self.doc
    }

    /// Write the project (`.geist`).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        dreist_formats::file::write_atomic(path, self.doc.to_xml())
    }

    /// Stop (or resume) mirroring messages into `document()`; a host
    /// whose editor keeps the document turns it off, which drops the
    /// engine's copy (`document()` is empty from then on).
    pub fn set_document_mirroring(&mut self, on: bool) {
        self.mirror_doc = on;
        if !on {
            // Whoever owns the document now keeps it; this copy would only
            // go stale (and take its share of memory and drop time).
            self.doc = Node::new("root", None);
        }
    }

    /// Mirror a handled message into the document.
    pub(super) fn doc_apply(&mut self, m: &Message) {
        if !self.mirror_doc {
            return;
        }
        mirror_message(&mut self.doc, m);
    }
}

fn doc_engine(doc: &mut Node, e: usize) -> &mut Node {
    doc.child_mut_or_insert("Engine", &format!("Engine{e}"))
}

fn doc_pad(doc: &mut Node, e: usize, p: usize) -> &mut Node {
    doc_engine(doc, e).child_mut_or_insert("Pad", &format!("Pad{p}"))
}

fn doc_layer(doc: &mut Node, e: usize, p: usize, l: usize) -> &mut Node {
    doc_pad(doc, e, p).child_mut_or_insert("Layer", &format!("Layer{l}"))
}

/// Effect slot node under a layer/pad/engine (−1 = the enclosing level).
fn doc_chain(doc: &mut Node, e: i32, p: i32, l: i32) -> Option<&mut Node> {
    let owner = if e == -4 && p >= 0 {
        // Effect chain lookup: −4 = global aux `p`, −3 = master.
        doc.child_mut_or_insert("GlobalAux", &format!("GlobalAux{p}"))
    } else if e == -3 {
        doc.child_mut_or_insert("MasterChannel", "MasterChannel")
    } else if e < 0 {
        return None;
    } else if p < 0 {
        doc_engine(doc, e as usize)
    } else if l < 0 {
        doc_pad(doc, e as usize, p as usize)
    } else {
        doc_layer(doc, e as usize, p as usize, l as usize)
    };
    Some(owner.child_mut_or_insert("EffectChain", "EffectChain"))
}

fn doc_effect(doc: &mut Node, e: i32, p: i32, l: i32, slot: usize) -> Option<&mut Node> {
    Some((doc_chain(doc, e, p, l)?).child_mut_or_insert("Effect", &format!("Effect{slot}")))
}

/// Apply a message's persistent effect to a `.geist` document tree (the
/// engine's own document, or an editor's copy of it).
pub fn mirror_message(doc: &mut Node, m: &Message) {
    {
        let i = |k: usize| m.index[k];
        let v = |k: usize| m.value[k];
        let vi = |k: usize| m.value[k] as i32;
        let bool_str = |b: bool| if b { "true" } else { "false" };
        let (e, p, l) = (i(0), i(1), i(2));
        let ok = |x: i32| x >= 0;
        match m.kind {
            id::SET_MACRO_VALUE if (0..4).contains(&e) => {
                doc.set(&format!("Macro{e}"), fmt(v(0)));
            }
            // [re:eba363] attributes.
            id::SAMPLER_INPUT_GAIN
            | id::SAMPLER_OUTPUT_GAIN
            | id::SAMPLER_START_THRESHOLD
            | id::SAMPLER_STOP_THRESHOLD
            | id::SAMPLER_RECORD_SECS
            | id::SAMPLER_BARS_BEATS
            | id::SAMPLER_BEATS_MODE
            | id::SAMPLER_RECORD_SOURCE
            | id::SAMPLER_THRU => {
                let sm = doc.child_mut_or_insert("Sampler", "Sampler");
                if sm.version.is_none() {
                    sm.version = Some(2);
                }
                match m.kind {
                    id::SAMPLER_INPUT_GAIN => sm.set("inputgain", fmt(v(0))),
                    id::SAMPLER_OUTPUT_GAIN => sm.set("outputgain", fmt(v(0))),
                    id::SAMPLER_START_THRESHOLD => sm.set("startthreshold", fmt(v(0))),
                    id::SAMPLER_STOP_THRESHOLD => sm.set("stopthreshold", fmt(v(0))),
                    id::SAMPLER_RECORD_SECS => sm.set("recsecs", fmt(v(0).clamp(0.0, 3600.0))),
                    id::SAMPLER_BARS_BEATS => {
                        sm.set("recbars", (v(0).max(0.0) as u32).min(16));
                        sm.set("recbeats", v(1).max(0.0) as u32)
                    }
                    id::SAMPLER_BEATS_MODE => sm.set("beatsmode", bool_str(v(0) >= 0.5)),
                    id::SAMPLER_RECORD_SOURCE => sm.set("recordsource", crate::sampler::source_name(vi(0))),
                    _ => sm.set("inputthru", bool_str(v(0) >= 0.5)),
                };
            }
            id::SET_TEMPO => {
                let t = doc.child_mut_or_insert("TimeInfo", "TimeInfo");
                t.set("Tempo", (v(0) as f64).clamp(20.0, 999.0));
            }
            id::SET_LAYER_PARAMETER if ok(e) && ok(p) && ok(l) => {
                if let Some(par) = LayerParam::ALL.get(vi(0).max(0) as usize) {
                    if let Some(a) = attr_name(par.name(), vi(2).max(0) as usize) {
                        doc_layer(doc, e as usize, p as usize, l as usize).set(&a, fmt(v(1)));
                    }
                }
            }
            id::SET_PAD_PARAMETER if ok(e) && ok(p) => {
                if let Some(par) = PadParam::ALL.get(vi(1).max(0) as usize) {
                    if let Some(a) = attr_name(par.name(), vi(2).max(0) as usize) {
                        doc_pad(doc, e as usize, p as usize).set(&a, fmt(v(0)));
                    }
                }
            }
            id::SET_ENGINE_PARAMETER if ok(e) => {
                if let Some(par) = EngineParam::ALL.get(vi(1).max(0) as usize) {
                    if let Some(a) = attr_name(par.name(), vi(2).max(0) as usize) {
                        doc_engine(doc, e as usize).set(&a, fmt(v(0)));
                    }
                }
            }
            id::SET_LAYER_SEND_DEPTH | id::SET_PAD_SEND_LEVEL | id::SET_ENGINE_SEND_LEVEL if ok(e) => {
                let Some(a) = attr_name(&aux_send_name(vi(0).max(0) as usize), vi(2).max(0) as usize) else { return };
                let node = match m.kind {
                    id::SET_LAYER_SEND_DEPTH if ok(p) && ok(l) => doc_layer(doc, e as usize, p as usize, l as usize),
                    id::SET_PAD_SEND_LEVEL if ok(p) => doc_pad(doc, e as usize, p as usize),
                    id::SET_ENGINE_SEND_LEVEL => doc_engine(doc, e as usize),
                    _ => return,
                };
                node.set(&a, fmt(v(1)));
            }
            id::SET_PAD_OUTPUT | id::SET_LAYER_OUTPUT | id::SET_ENGINE_OUTPUT | id::SET_GLOBAL_AUX_OUTPUT if ok(e) => {
                let name = super::Route::from_index(vi(0).max(0) as usize).name();
                let node = match m.kind {
                    id::SET_PAD_OUTPUT if ok(p) => doc_pad(doc, e as usize, p as usize),
                    id::SET_LAYER_OUTPUT if ok(p) && ok(l) => doc_layer(doc, e as usize, p as usize, l as usize),
                    id::SET_ENGINE_OUTPUT => doc_engine(doc, e as usize),
                    id::SET_GLOBAL_AUX_OUTPUT => doc.child_mut_or_insert("GlobalAux", &format!("GlobalAux{e}")),
                    _ => return,
                };
                node.set("OutputLocation", name);
            }
            id::SET_GLOBAL_AUX_PRE_POST if ok(e) => {
                doc.child_mut_or_insert("GlobalAux", &format!("GlobalAux{e}")).set("IsPreFade", bool_str(v(0) >= 0.5));
            }
            id::SET_GLOBAL_AUX_GAIN if ok(e) => {
                doc.child_mut_or_insert("GlobalAux", &format!("GlobalAux{e}")).set("Gain", fmt(v(0)));
            }
            id::SET_MASTER_CHANNEL_GAIN => {
                doc.child_mut_or_insert("MasterChannel", "MasterChannel").set("Gain", fmt(v(0)));
            }
            id::SET_MASTER_CHANNEL_TUNE => {
                doc.child_mut_or_insert("MasterChannel", "MasterChannel").set("Pitch", fmt(v(0)));
            }
            id::MUTE_ENGINE | id::SOLO_ENGINE if ok(e) => {
                let k = if m.kind == id::MUTE_ENGINE { "Mute" } else { "Solo" };
                doc_engine(doc, e as usize).set(k, bool_str(v(0) >= 0.5));
            }
            id::MUTE_PAD | id::SOLO_PAD if ok(e) && ok(p) => {
                let k = if m.kind == id::MUTE_PAD { "Mute" } else { "Solo" };
                doc_pad(doc, e as usize, p as usize).set(k, bool_str(v(0) >= 0.5));
            }
            id::SET_LAYER_MUTE | id::SET_LAYER_SOLO if ok(e) && ok(p) && ok(l) => {
                let k = if m.kind == id::SET_LAYER_MUTE { "Mute" } else { "Solo" };
                doc_layer(doc, e as usize, p as usize, l as usize).set(k, bool_str(v(0) >= 0.5));
            }
            // `e_setFXChainActive` 0x60 / `e_setFXChainMix` 0x62 [e, p, l] [v]:
            // `EffectChain` `IsActive` / `ChainMix`.
            id::SET_FX_CHAIN_ACTIVE | id::SET_FX_CHAIN_MIX => {
                let Some(c) = doc_chain(doc, e, i(1), i(2)) else { return };
                if m.kind == id::SET_FX_CHAIN_ACTIVE {
                    c.set("IsActive", bool_str(v(0) >= 0.5));
                } else {
                    c.set("ChainMix", fmt(v(0).clamp(0.0, 1.0)));
                }
            }
            id::SET_FX_PARAMETER | id::SET_FX_MIX | id::SET_FX_LEVEL | id::SET_FX_TRIM | id::SET_FX_ACTIVE => {
                let slot = if i(3) >= 0 { i(3) } else { vi(0) };
                if slot < 0 {
                    return;
                }
                let Some(fx) = doc_effect(doc, e, p, l, slot as usize) else { return };
                let (name, src, val) = match m.kind {
                    id::SET_FX_PARAMETER => {
                        let kind = fx.attr("EffectType").and_then(effect_type_index).unwrap_or(0);
                        let Some(n) = effect_param_names(kind).get(vi(0).max(0) as usize) else { return };
                        (n.chars().filter(|c| !c.is_whitespace()).collect::<String>(), vi(1).max(0) as usize, v(2))
                    }
                    id::SET_FX_MIX => ("Mix".into(), vi(2).max(0) as usize, v(1)),
                    id::SET_FX_LEVEL => ("Level".into(), vi(2).max(0) as usize, v(1)),
                    id::SET_FX_TRIM => ("Trim".into(), vi(2).max(0) as usize, v(1)),
                    _ => {
                        fx.set("IsActive", bool_str(v(1) >= 0.5));
                        return;
                    }
                };
                if let Some(a) = attr_name(&name, src) {
                    fx.set(&a, fmt(val));
                }
            }
            id::SET_MODULATION_SOURCE_TRIGGER | id::SET_MODULATION_SOURCE_TRANSPORT_START | id::SET_MODULATION_SOURCE_PATTERN_CHANGE if ok(vi(0)) => {
                let key = match m.kind {
                    id::SET_MODULATION_SOURCE_TRIGGER if ok(e) && ok(i(1)) => format!("PadTrig{e}-{}", i(1)),
                    id::SET_MODULATION_SOURCE_TRANSPORT_START => "ResetAtStart".into(),
                    id::SET_MODULATION_SOURCE_PATTERN_CHANGE if ok(e) => format!("EngTrig{e}"),
                    _ => return,
                };
                doc.child_mut_or_insert("Modulator", &format!("Modulator{}", vi(0))).set(&key, bool_str(v(1) >= 0.5));
            }
            id::SET_MODULATION_SOURCE_PARAMETER if ok(e) => {
                let node = doc.child_mut_or_insert("Modulator", &format!("Modulator{e}"));
                let ty = node.attr("Type").and_then(dreist_formats::ModulatorType::from_name).unwrap_or(dreist_formats::ModulatorType::LFO);
                let Some(name) = ty.params().get(vi(1).max(0) as usize) else { return };
                let src = TransmodSource::ALL.get(vi(2).max(0) as usize).copied().unwrap_or(TransmodSource::ActualParameter);
                node.set(&format!("{name}_{}", src.name()), fmt(v(3)));
            }
            id::SET_ENGINE_TEMPO_NUM | id::SET_ENGINE_TEMPO_DENOM | id::SET_ENGINE_SWING_TEMPLATE_DEPTH if ok(e) => {
                let key = match m.kind {
                    id::SET_ENGINE_TEMPO_NUM => "TempoNumerator",
                    id::SET_ENGINE_TEMPO_DENOM => "TempoDenominator",
                    _ => "SwingTemplateDepth",
                };
                let eng = doc_engine(doc, e as usize);
                if m.kind == id::SET_ENGINE_SWING_TEMPLATE_DEPTH {
                    eng.set(key, fmt(v(0)));
                } else {
                    eng.set(key, (v(0).round() as i32).clamp(1, 16));
                }
            }
            id::SONG_SET_INTERNAL_LOOP_ACTIVE | id::SONG_SET_INTERNAL_LOOP_RANGE => {
                let seq = doc.child_mut_or_insert("SongSequencer", "Song");
                if m.kind == id::SONG_SET_INTERNAL_LOOP_ACTIVE {
                    seq.set("SongLoopEnabled", bool_str(v(0) >= 0.5));
                } else {
                    seq.set("SongLoopStart", fmt(crate::song::quantize_round(v(0) as f64) as f32));
                    seq.set("SongLoopEnd", fmt(crate::song::quantize_round(v(1) as f64) as f32));
                }
            }
            id::SONG_SET_ACTIVE | id::SONG_CLEAR_ALL | id::SONG_ADD_PART | id::SONG_DELETE_PART => {
                let seq = doc.child_mut_or_insert("SongSequencer", "Song");
                if m.kind == id::SONG_SET_ACTIVE {
                    seq.set("SongActive", bool_str(v(0) >= 0.5));
                    return;
                }
                let mut song = crate::song::Song::from_node(seq);
                match m.kind {
                    id::SONG_CLEAR_ALL => song.tracks.iter_mut().for_each(Vec::clear),
                    id::SONG_ADD_PART => {
                        song.add_part(e.max(0) as usize, crate::song::SongPart { start: v(0) as f64, length: (v(1) as f64).max(0.25), pattern: vi(2).max(0) as usize, phase: 0.0, mute: false })
                    }
                    _ => song.delete_part_at(e.max(0) as usize, v(0) as f64),
                }
                song.write_tracks(seq);
            }
            id::SCENE_SNAPSHOT | id::SCENE_CLEAR | id::SCENE_SET_START_MODE | id::SCENE_CUT => {
                let sc = vi(0).clamp(0, 95);
                // Engine pattern states (EngineState CurrentPattern / PatternPower).
                let states: Vec<(String, String)> = (0..8)
                    .map(|k| {
                        let st =
                            doc.children.iter().find(|c| c.class == "Engine" && c.name.as_deref() == Some(&format!("Engine{k}"))).and_then(|n| n.children.iter().find(|c| c.class == "EngineState"));
                        let pat = st.and_then(|s| s.attr("CurrentPattern")).unwrap_or("0").to_string();
                        let pow = st.and_then(|s| s.attr("PatternPower")).unwrap_or("true").to_string();
                        (pat, pow)
                    })
                    .collect();
                let mgr = doc.child_mut_or_insert("SceneManager", "Scenes");
                let scene = mgr.child_mut_or_insert("Scene", &format!("Scene{sc}"));
                match m.kind {
                    id::SCENE_SNAPSHOT => {
                        scene.set("used", "true");
                        for (k, (pat, pow)) in states.into_iter().enumerate() {
                            let se = scene.child_mut_or_insert("SceneEngine", &format!("Eng{k}"));
                            se.set("patt", pat);
                            se.set("pattpower", pow);
                        }
                    }
                    // Cut clears the scene; the copy is the editor's own clipboard.
                    id::SCENE_CLEAR | id::SCENE_CUT => {
                        scene.attrs.clear();
                        scene.children.clear();
                        scene.set("used", "false");
                    }
                    _ => {
                        scene.set("start", ["immediate", "beat", "bar"][vi(1).clamp(0, 2) as usize]);
                    }
                }
            }
            id::SET_LAYER_START_SAMPLE
            | id::SET_LAYER_END_SAMPLE
            | id::SET_LAYER_LOOP_START_SAMPLE
            | id::SET_LAYER_LOOP_END_SAMPLE
            | id::SET_LAYER_START_SAMPLE01
            | id::SET_LAYER_END_SAMPLE01
            | id::SET_LAYER_LOOP_START_SAMPLE01
            | id::SET_LAYER_LOOP_END_SAMPLE01
                if ok(e) && ok(p) && i(2) >= 0 =>
            {
                let layer = doc_pad(doc, e as usize, p as usize).child_mut_or_insert("Layer", &format!("Layer{}", i(2)));
                let unit = m.kind >= id::SET_LAYER_START_SAMPLE01;
                let kind = if unit { m.kind - 4 } else { m.kind };
                let (name, key) = match kind {
                    id::SET_LAYER_START_SAMPLE => ("StartEndMarkers", "StartSample"),
                    id::SET_LAYER_END_SAMPLE => ("StartEndMarkers", "EndSample"),
                    id::SET_LAYER_LOOP_START_SAMPLE => ("LoopMarkers", "StartSample"),
                    _ => ("LoopMarkers", "EndSample"),
                };
                let mk = layer.child_mut_or_insert("AudioFileMarkers", name);
                if mk.version.is_none() {
                    mk.version = Some(2);
                }
                // 0..1 variants: round(v·(SampleLength − 1)).
                let f = if unit {
                    let len = mk.attr("SampleLength").and_then(|s| s.trim().parse::<f32>().ok()).unwrap_or(0.0);
                    let x = v(0) * (len - 1.0);
                    (x + if x < 0.0 { -0.5 } else { 0.5 }).max(0.0) as u32
                } else {
                    v(0).max(0.0) as u32
                };
                mk.set(key, f);
            }
            // [re:577e2e] → `EngineState`
            // `padSelAnchor` / `padSelItems` (the mask's halves travel as
            // raw float bits).
            id::SET_PAD_SELECTIONS if ok(e) => {
                let mask = m.value[1].to_bits() as u64 | (m.value[2].to_bits() as u64) << 32;
                let st = doc_engine(doc, e as usize).child_mut_or_insert("EngineState", "EngineState");
                st.set("padSelAnchor", vi(0).max(0));
                st.set("padSelItems", mask);
            }
            // [re:96a0ce] → [re:01b7f4]
            // `layerSelAnchor` / `layerSelItems` under `EngineState`.
            id::SET_LAYER_SELECTIONS if ok(e) && ok(p) => {
                let st = doc_engine(doc, e as usize).child_mut_or_insert("EngineState", "EngineState");
                let ps = st.child_mut_or_insert("PadState", &format!("PadState{p}"));
                ps.set("layerSelAnchor", vi(0).max(0));
                ps.set("layerSelItems", m.value[1].to_bits());
            }
            id::SET_PAD_LAYER_MODE | id::SET_PAD_LAYER_MODE01 if ok(e) && ok(p) => {
                let n = if m.kind == id::SET_PAD_LAYER_MODE { vi(0) } else { (v(0) * 4.0).round() as i32 };
                let name = dreist_dsp::voices::LAYER_MODE_NAMES[n.clamp(0, 4) as usize];
                doc_pad(doc, e as usize, p as usize).set("PadMode", name);
            }
            id::SET_PAD_LINKAGE if ok(e) && ok(p) && (0..64).contains(&vi(0)) => {
                doc_pad(doc, e as usize, p as usize).set(&format!("Link{}", vi(0)), bool_str(v(1) >= 0.5));
            }
            id::SET_LAYER_VELOCITY_SPLIT_MIN | id::SET_LAYER_VELOCITY_SPLIT_MIN_TURNOVER | id::SET_LAYER_VELOCITY_SPLIT_MAX_TURNOVER | id::SET_LAYER_VELOCITY_SPLIT_MAX if ok(e) && ok(p) && ok(l) => {
                let k = (m.kind - id::SET_LAYER_VELOCITY_SPLIT_MIN) as usize;
                // Message order min, min turnover, max, max turnover.
                let k = [0, 1, 3, 2][k];
                let mut s = layer_split(doc_layer(doc, e as usize, p as usize, l as usize));
                s[k] = (vi(0).max(0) as u32).min(127);
                set_layer_split(doc_layer(doc, e as usize, p as usize, l as usize), s);
            }
            id::AUTO_VELOCITY_SPLIT_PAD | id::AUTO_VELOCITY_CROSS if ok(e) && ok(p) => {
                let pad = doc_pad(doc, e as usize, p as usize);
                let layer_of = |pad: &Node, li: usize| pad.children.iter().position(|c| c.class == "Layer" && c.name.as_deref() == Some(&format!("Layer{li}")));
                let idx: Vec<Option<usize>> = (0..8).map(|li| layer_of(pad, li)).collect();
                let valid: Vec<bool> = idx.iter().map(|i| i.is_some_and(|i| pad.children[i].attr("AudioFileName").is_some_and(|a| !a.is_empty()))).collect();
                let mut splits: Vec<crate::velsplit::Split> = idx.iter().map(|i| i.map(|i| layer_split(&pad.children[i])).unwrap_or(crate::velsplit::DEFAULT)).collect();
                if m.kind == id::AUTO_VELOCITY_SPLIT_PAD {
                    crate::velsplit::auto_split(&valid, &mut splits);
                } else {
                    crate::velsplit::auto_cross(&valid, &mut splits, v(0));
                }
                for (li, i) in idx.iter().enumerate() {
                    if let (Some(i), true) = (i, valid[li]) {
                        set_layer_split(&mut pad.children[*i], splits[li]);
                    }
                }
            }
            id::SET_LAYER_CUT_MODE | id::SET_LAYER_PLAY_MODE if ok(e) && ok(p) => {
                let n = (vi(0).max(0)) as usize;
                let (key, name) =
                    if m.kind == id::SET_LAYER_CUT_MODE { ("CutMode", dreist_dsp::voices::CUT_MODE_LABELS[n.min(15)]) } else { ("PlaybackMode", dreist_dsp::voice::PLAYBACK_MODE_LABELS[n.min(3)]) };
                let pad = doc_pad(doc, e as usize, p as usize);
                for c in pad.children.iter_mut().filter(|c| c.class == "Layer") {
                    if ok(l) && c.name.as_deref() != Some(&format!("Layer{l}")) {
                        continue;
                    }
                    c.set(key, name);
                }
            }
            id::SET_LAYER_SYNTHESIS_MODE | id::SET_LAYER_REVERSE_STATUS | id::SET_LAYER_PHASE_POSITION | id::SET_LAYER_ENVELOPE_TRACKING if ok(e) && ok(p) => {
                let on = v(0) >= 0.5;
                if ok(l) {
                    doc_layer(doc, e as usize, p as usize, l as usize);
                }
                let pad = doc_pad(doc, e as usize, p as usize);
                for c in pad.children.iter_mut().filter(|c| c.class == "Layer") {
                    if ok(l) && c.name.as_deref() != Some(&format!("Layer{l}")) {
                        continue;
                    }
                    match m.kind {
                        id::SET_LAYER_SYNTHESIS_MODE => c.set("SynthesisMode", if v(0).round() as i32 == 1 { "VintageSampler" } else { "Sampler" }),
                        id::SET_LAYER_REVERSE_STATUS => c.set("IsReversed", if on { "true" } else { "false" }),
                        id::SET_LAYER_PHASE_POSITION => c.set("PhasePosition", if on { "-1" } else { "1" }),
                        _ => c.set("EnvTracksPitch", if on { "true" } else { "false" }),
                    };
                }
            }
            id::CLEAR_ALL_PATTERNS if ok(e) => {
                for sp in doc_engine(doc, e as usize).children.iter_mut().filter(|c| c.class == "StepPattern") {
                    for tr in sp.children.iter_mut().filter(|c| c.class == "Track") {
                        tr.children.retain(|c| c.class != "Step");
                    }
                }
            }
            id::CLEAR_PATTERN
            | id::CLEAR_TRACK
            | id::NUDGE_TRACK_FORWARD
            | id::NUDGE_TRACK_BACKWARDS
            | id::NUDGE_PATTERN_FORWARD
            | id::NUDGE_PATTERN_BACKWARDS
            | id::SET_STEP_GRAPH_STATE
            | id::SET_GRAPH_RANDOMISATION
            | id::CLEAR_TRACK_AUTOMATION
            | id::CLEAR_ALL_TRACK_AUTOMATION
            | id::CLEAR_ALL_PATTERN_AUTOMATION
            | id::QUANTIZE_PATTERN_TIMING
            | id::QUANTIZE_PATTERN_VELOCITY
            | id::SET_PATTERN_MONO_MODE
                if ok(e) =>
            {
                let eng = doc_engine(doc, e as usize);
                let pi = if i(3) >= 0 {
                    i(3) as usize
                } else {
                    eng.children.iter().find(|c| c.class == "EngineState").and_then(|s| s.attr("CurrentPattern")).and_then(|v| v.trim().parse().ok()).unwrap_or(0)
                };
                let pat = eng.child_mut_or_insert("StepPattern", &format!("StepPattern{pi}"));
                let n = pat.attr("PatternSteps").and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(16).max(1);
                let track = |t: i32, c: &Node| t < 0 || c.name.as_deref() == Some(&format!("Track{t}"));
                if m.kind == id::SET_PATTERN_MONO_MODE {
                    pat.set("MonoModeActive", bool_str(v(0) >= 0.5));
                    return;
                }
                let whole = matches!(
                    m.kind,
                    id::CLEAR_PATTERN | id::NUDGE_PATTERN_FORWARD | id::NUDGE_PATTERN_BACKWARDS | id::CLEAR_ALL_PATTERN_AUTOMATION | id::QUANTIZE_PATTERN_TIMING | id::QUANTIZE_PATTERN_VELOCITY
                );
                let t = if whole { -1 } else { i(4) };
                if !whole && t < 0 {
                    return;
                }
                if m.kind == id::SET_GRAPH_RANDOMISATION {
                    if let Some(name) = dreist_formats::STEP_LANES.get(vi(1).max(0) as usize) {
                        pat.child_mut_or_insert("Track", &format!("Track{t}")).set(&format!("{name}Rand"), fmt(v(0).clamp(0.0, 1.0)));
                    }
                    return;
                }
                if m.kind == id::SET_STEP_GRAPH_STATE {
                    let lane = vi(0).max(0) as usize;
                    let Some(name) = dreist_formats::STEP_LANES.get(lane) else { return };
                    if i(5) < 0 {
                        return;
                    }
                    let tr = pat.child_mut_or_insert("Track", &format!("Track{t}"));
                    let st = tr.child_mut_or_insert("Step", &format!("S{}", i(5)));
                    let mut x = v(1).clamp(0.0, 1.0);
                    if crate::pattern::lane_style(lane) == 2 {
                        x = if x < 0.5 { 0.0 } else { 1.0 };
                    }
                    st.set(name, fmt(x));
                    if *name == "Repeats" && x != 0.0 {
                        st.attrs.retain(|(k, _)| k != "LinkToPrevious");
                    }
                    return;
                }
                for tr in pat.children.iter_mut().filter(|c| c.class == "Track" && track(t, c)) {
                    if matches!(m.kind, id::QUANTIZE_PATTERN_TIMING | id::QUANTIZE_PATTERN_VELOCITY) {
                        let key = if m.kind == id::QUANTIZE_PATTERN_TIMING { "Shift" } else { "Velocity" };
                        for st in tr.children.iter_mut().filter(|c| c.class == "Step") {
                            st.attrs.retain(|(k, _)| k != key);
                        }
                        continue;
                    }
                    if matches!(m.kind, id::CLEAR_TRACK_AUTOMATION | id::CLEAR_ALL_TRACK_AUTOMATION | id::CLEAR_ALL_PATTERN_AUTOMATION) {
                        let only = (m.kind == id::CLEAR_TRACK_AUTOMATION).then(|| dreist_formats::STEP_LANES.get(vi(0).max(0) as usize).copied()).flatten();
                        for st in tr.children.iter_mut().filter(|c| c.class == "Step") {
                            st.attrs.retain(|(k, _)| k == "State" || only.is_some_and(|o| o != k.as_str()));
                        }
                        continue;
                    }
                    match m.kind {
                        id::CLEAR_PATTERN | id::CLEAR_TRACK => tr.children.retain(|c| c.class != "Step"),
                        _ => {
                            let fwd = matches!(m.kind, id::NUDGE_TRACK_FORWARD | id::NUDGE_PATTERN_FORWARD);
                            for c in tr.children.iter_mut().filter(|c| c.class == "Step") {
                                let Some(k) = c.name.as_deref().and_then(|s| s.strip_prefix('S')).and_then(|s| s.parse::<usize>().ok()) else { continue };
                                if k < n {
                                    let k2 = if fwd { (k + 1) % n } else { (k + n - 1) % n };
                                    c.name = Some(format!("S{k2}"));
                                }
                            }
                        }
                    }
                }
            }
            id::SET_PATTERN_STEP_LENGTH
            | id::SET_PATTERN_SYNC_MODE
            | id::SET_PATTERN_STEP_COUNT
            | id::SET_TRACK_MUTE_STATUS
            | id::SET_TRACK_SOLO_STATUS
            | id::SET_TRACK_LENGTH
            | id::SET_STEP_ON
            | id::SET_STEP_OFF
            | id::SET_STEP_VELOCITY
                if ok(e) =>
            {
                let eng = doc_engine(doc, e as usize);
                // [re:d2541e] from the document: no layer with a
                // sample plays Gated / Looping.
                let one_shot = i(4) < 0
                    || !eng
                        .children
                        .iter()
                        .filter(|c| c.class == "Pad" && c.name.as_deref() == Some(&format!("Pad{}", i(4))))
                        .flat_map(|p| p.children.iter().filter(|l| l.class == "Layer"))
                        .any(|l| l.attr("AudioFileName").is_some_and(|f| !f.is_empty()) && matches!(l.attr("PlaybackMode"), Some("Gated" | "Looping")));
                let pi = if i(3) >= 0 {
                    i(3) as usize
                } else {
                    eng.children.iter().find(|c| c.class == "EngineState").and_then(|s| s.attr("CurrentPattern")).and_then(|v| v.trim().parse().ok()).unwrap_or(0)
                };
                let pat = eng.child_mut_or_insert("StepPattern", &format!("StepPattern{pi}"));
                match m.kind {
                    id::SET_PATTERN_STEP_COUNT => {
                        pat.set("PatternSteps", (v(0) as i32).clamp(1, 1024));
                        return;
                    }
                    id::SET_PATTERN_STEP_LENGTH => {
                        pat.set("Resolution", crate::pattern::RESOLUTION_NAMES[(v(0) as i32).clamp(0, 13) as usize]);
                        return;
                    }
                    id::SET_PATTERN_SYNC_MODE => {
                        pat.set("SyncMode", crate::pattern::SYNC_MODE_NAMES[(v(0).max(0.0) as usize).min(3)]);
                        return;
                    }
                    _ => {}
                }
                if i(4) < 0 {
                    return;
                }
                let tr = pat.child_mut_or_insert("Track", &format!("Track{}", i(4)));
                match m.kind {
                    id::SET_TRACK_MUTE_STATUS => {
                        tr.set("Mute", bool_str(v(0) >= 0.5));
                    }
                    id::SET_TRACK_SOLO_STATUS => {
                        tr.set("Solo", bool_str(v(0) >= 0.5));
                    }
                    id::SET_TRACK_LENGTH => {
                        tr.set("TrackLength", (v(0) as i32).max(1));
                    }
                    _ if i(5) >= 0 => {
                        let step = tr.child_mut_or_insert("Step", &format!("S{}", i(5)));
                        match m.kind {
                            id::SET_STEP_ON => {
                                step.set("Velocity", fmt(v(0)));
                                step.set("State", "true");
                                // Gated / looping pads also take duration
                                // and the tie (as the engine's 0xd2).
                                if !one_shot {
                                    step.set("Duration", fmt(v(1)));
                                }
                                if !one_shot && v(2) >= 0.5 {
                                    step.set("LinkToPrevious", "1");
                                } else {
                                    step.attrs.retain(|(k, _)| k != "LinkToPrevious");
                                }
                            }
                            id::SET_STEP_OFF => {
                                step.attrs.retain(|(k, _)| k != "LinkToPrevious");
                                step.set("State", "false");
                            }
                            _ => {
                                step.set("Velocity", fmt(v(0)));
                            }
                        }
                    }
                    _ => {}
                }
            }
            // [re:9d583d] attributes for the pads strip.
            id::SET_NOTE_ERASE_STATE | id::SET_NOTE_REPEAT_STATE | id::SET_FULL_LEVEL_STATE | id::SET_FULL_LEVEL_LEVEL if ok(e) => {
                let es = doc_engine(doc, e as usize).child_mut_or_insert("EngineState", "EngineState");
                match m.kind {
                    id::SET_NOTE_ERASE_STATE => es.set("NoteEraseEnabled", bool_str(v(0) >= 0.5)),
                    id::SET_NOTE_REPEAT_STATE => es.set("NoteRepeatEnabled", bool_str(v(0) >= 0.5)),
                    id::SET_FULL_LEVEL_STATE => es.set("FullLevelEnabled", bool_str(v(0) >= 0.5)),
                    _ => es.set("FullLevels", (v(0) * 127.0) as i32),
                };
            }
            id::SET_PATTERN_POWER_STATE if ok(e) => {
                let es = doc_engine(doc, e as usize).child_mut_or_insert("EngineState", "EngineState");
                es.set("PatternPower", bool_str(v(0) >= 0.5));
            }
            _ => {}
        }
    }
}

impl Dreist {
    /// Mirror a recorded step (recording message).
    pub(super) fn doc_step(&mut self, e: usize, pattern: usize, track: usize, step: usize) {
        let Some(s) = self.engines.get(e).and_then(|x| x.patterns.get(pattern)).and_then(|p| p.tracks.get(track)).and_then(|t| t.steps.get(step)).cloned() else {
            return;
        };
        if self.step_writes.len() < 4096 {
            self.step_writes.push(super::StepWrite { engine: e, pattern, track, step, data: s.clone() });
        }
        if self.mirror_doc {
            write_step(&mut self.doc, e, pattern, track, step, &s);
        }
    }
}

/// Write a step's lanes into the document (`Step name="S{n}"`, lanes that
/// differ from the defaults, `State`).
pub fn write_step(doc: &mut Node, e: usize, pattern: usize, track: usize, step: usize, s: &crate::pattern::StepData) {
    let defaults = crate::pattern::StepData::off();
    let eng = doc_engine(doc, e);
    let pat = eng.child_mut_or_insert("StepPattern", &format!("StepPattern{pattern}"));
    let tr = pat.child_mut_or_insert("Track", &format!("Track{track}"));
    let node = tr.child_mut_or_insert("Step", &format!("S{step}"));
    node.attrs.clear();
    for (k, name) in dreist_formats::STEP_LANES.iter().enumerate() {
        if (s.lanes[k] - defaults.lanes[k]).abs() > 1e-9 {
            node.set(name, fmt(s.lanes[k]));
        }
    }
    node.set("State", if s.on { "true" } else { "false" });
}

/// Mirror `e_songReplaceTrack` into the document.
pub fn mirror_song_tracks(doc: &mut Node, tracks: &[(usize, Vec<crate::song::SongPart>)]) {
    let seq = doc.child_mut_or_insert("SongSequencer", "Song");
    let mut song = crate::song::Song::from_node(seq);
    for (t, parts) in tracks {
        song.replace_track(*t, parts.clone());
    }
    song.write_tracks(seq);
}

/// Mirror `e_setEngineSwingTemplate`: the engine's [re:9f001e] child.
pub fn mirror_swing_template(doc: &mut Node, e: usize, template: &Node) {
    let eng = doc_engine(doc, e);
    eng.children.retain(|c| c.class != "SwingTemplate");
    let mut t = template.clone();
    t.name = Some("SwingTemplate".into());
    eng.children.push(t);
}

/// A layer node's `VelocitySplit` (default 0/0/127/127).
pub fn layer_split(layer: &Node) -> crate::velsplit::Split {
    use crate::velsplit::{ATTRS, DEFAULT, NODE_NAME};
    let node = layer.children.iter().find(|c| c.name.as_deref() == Some(NODE_NAME));
    std::array::from_fn(|k| node.and_then(|n| n.attr(ATTRS[k])).and_then(|v| v.parse().ok()).unwrap_or(DEFAULT[k]).min(127))
}

pub fn set_layer_split(layer: &mut Node, s: crate::velsplit::Split) {
    use crate::velsplit::{ATTRS, NODE_CLASS, NODE_NAME};
    let node = match layer.children.iter().position(|c| c.name.as_deref() == Some(NODE_NAME)) {
        Some(i) => &mut layer.children[i],
        None => layer.child_mut_or_insert(NODE_CLASS, NODE_NAME),
    };
    for (k, a) in ATTRS.iter().enumerate() {
        node.set(a, s[k]);
    }
}
