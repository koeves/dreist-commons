//! Sampler panel (`SimpleSamplerPanel.skin`, [re:9fbf48]): content
//! 0 of the pads `Splitter`, opened to 80 px when the engine's pad mode is
//! Sampler ([re:f5831b], which also gives the pad
//! matrix a top spacing of 2).
//!
//! - [re:e1d697]: TriggerMode items Immediate /
//!   MIDI / Host / Threshold / Timed MIDI (posts `e_setSamplerStartMode`
//!   0xf2, lambda [re:2a7fed]); MonoButton menu Stereo / Mono (Left) / Mono
//!   (Right) → 0xfe (lambdas [re:7e2b16]…); Follow lists the auto modes
//!   (`CreateAutoSamplerModeMap` [re:91218b]).
//! - `HandleControlEvent` [re:97cc15]: Arm → 0xec, Rec → 0xef, Thru → 0xff,
//!   InputGain → 0xf4 (slider × 4), PreviewVolume → 0xe8, thresholds →
//!   0xf6 / 0xf7 via [re:766df4], sync group →
//!   0xfa, seconds → 0xf8 (0..3600), bars/beats → 0xf9, source → 0xfd.
//! - `UpdateState` [re:c51166]: button states and panel visibility.

use dreist_engine::binding::id;
use dreist_gui::app::{App, UiEvent};
use dreist_gui::{Ctrl, Rect, Skin};

use crate::command::{Command, msg};
use crate::model::Model;
use crate::{EditorError, err};

pub const TRIGGER_MODES: [&str; 5] = ["Immediate", "MIDI", "Host", "Threshold", "Timed MIDI"];
pub const CHANNEL_MODES: [&str; 3] = ["Stereo", "Mono (Left)", "Mono (Right)"];
/// [re:55c82e] names, camel case split.
pub const AUTO_MODES: [&str; 6] = ["Slice", "Single", "Next Layer", "Next Pad", "Next Layer And Pad", "No Auto Allocation"];
/// `e_setPreviewVolume`.
const SET_PREVIEW_VOLUME: u32 = 0xe8;
/// [re:dab344]: "External" (the host
/// input, 38), then output locations 1–33 by name (Master, Sub output
/// 1–32); MITM producers would follow (out of scope).
pub fn source_items() -> Vec<String> {
    std::iter::once("External".to_string()).chain((1..34).map(|i| dreist_engine::engine::Route::from_index(i).name())).collect()
}

/// List index ↔ `recordsource`: item 0 = 38, item i = location i.
fn source_of_item(i: usize) -> i32 {
    if i == 0 { dreist_engine::sampler::SOURCE_EXTERNAL } else { i.min(33) as i32 }
}

fn item_of_source(s: i32) -> usize {
    if (1..34).contains(&s) { s as usize } else { 0 }
}
/// Splitter position when open (`UpdatePadMode`).
pub const OPEN_HEIGHT: i32 = 80;

/// [re:c42363].
pub fn level_to_value(l: f32) -> f32 {
    if l <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * l.log10();
    if db > -1000.0 { (db / 26.888344).exp() / 1.25 } else { 0.0 }
}

/// [re:f41d07].
pub fn value_to_level(v: f32) -> f32 {
    if (v as f64) <= 5.64045724672641e-17 {
        return 0.0;
    }
    10f32.powf((v * 1.25).ln() * 6.0 / 4.4629)
}

pub struct Sampler {
    /// Path of the `SplitterPanel0` host.
    host: Option<Vec<usize>>,
    /// Whether the splitter is open.
    open: bool,
    /// Auto-allocation target (engine, pad, layer) ([re:b41333]
    /// ), set when arming.
    pub target: Option<(usize, usize, usize)>,
    /// Takes saved this session (" Take %d ").
    taken: usize,
}

/// What the editor does with a finished take.
#[derive(Debug, PartialEq)]
pub enum TakeAction {
    /// Follow = Slice: open the slicer on the file.
    Slice(std::path::PathBuf),
    /// Loaded into a layer (structural rebuild needed).
    Loaded,
    /// Saved only (No Auto Allocation).
    Saved,
}

/// [re:1a0972] $_10 [re:324b22]: takes
/// go to `<record path>/<date>/` ([re:9c6245] +
/// "Sampler Captures", unless `SamplerRecordPath` is set).
pub fn record_dir(opts: &crate::options::Options) -> std::path::PathBuf {
    let custom = opts.string("SamplerRecordPath", "");
    let base = if custom.is_empty() { crate::options::documents_dir().join("Sampler Captures") } else { std::path::PathBuf::from(custom) };
    let (y, mo, d, _, _, _) = now_utc();
    base.join(format!("{y:04}-{mo:02}-{d:02}"))
}

/// UTC civil time (year, month, day, hour, minute, second).
fn now_utc() -> (i64, u32, u32, u32, u32, u32) {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + (m <= 2) as i64;
    (y, m, d, (rem / 3600) as u32, (rem / 60 % 60) as u32, (rem % 60) as u32)
}

/// `FindFreeSampleFilename` [re:7d058d]: `name.wav`, then `name (2).wav` …
/// up to 9999.
pub fn free_filename(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    let p = dir.join(format!("{name}.wav"));
    if !p.exists() {
        return Some(p);
    }
    (2..=9999).map(|n| dir.join(format!("{name} ({n}).wav"))).find(|p| !p.exists())
}

/// Auto-allocation step after a take (`DoSendTakeToPadAutoExport` $_11
/// [re:78dd34]): the layer the take goes to and the next target, or None
/// when the sampler disarms.
pub fn allocate(mode: usize, target: (usize, usize, usize), selected_layer: usize, layer_used: &dyn Fn(usize) -> bool) -> ((usize, usize, usize), Option<(usize, usize, usize)>) {
    let (e, mut pad, mut layer) = target;
    match mode {
        // Single: the selected layer of the target pad.
        1 => layer = selected_layer,
        // Next Pad: the first free layer of the target pad (else layer 0).
        3 => layer = (0..8).find(|&l| !layer_used(l)).unwrap_or(0),
        _ => {}
    }
    let at = (e, pad, layer);
    let next = match mode {
        2 => (layer + 1 < 8).then(|| (e, pad, layer + 1)),
        3 => {
            pad += 1;
            (pad < 64).then_some((e, pad, layer))
        }
        4 => {
            layer += 1;
            if layer > 7 {
                layer = 0;
                pad += 1;
            }
            (pad < 64).then_some((e, pad, layer))
        }
        _ => None,
    };
    (at, next)
}

impl Sampler {
    pub fn build(skin: &mut Skin, root: &mut Ctrl) -> Result<Sampler, EditorError> {
        let Some(sp_path) = root.path_of("Splitter") else { return Ok(Sampler { host: None, open: false, target: None, taken: 0 }) };
        let mut sp = &mut *root;
        for &i in &sp_path {
            sp = &mut sp.children[i];
        }
        let mut host = Ctrl {
            class: "CtrlPanel".into(),
            name: "SamplerPanel".into(),
            display_name: "SplitterPanel0".into(),
            style: "InvisibleStyle".into(),
            rect: Rect::new(0, 0, sp.rect.w, OPEN_HEIGHT),
            visible: false,
            enabled: true,
            ..Ctrl::default()
        };
        skin.mount(&mut host, "SimpleSamplerPanel.skin").map_err(err)?;
        // [re:eec7d9] / [re:b2a4bd] behave as stock controls here.
        fn stock(c: &mut Ctrl) {
            for k in &mut c.children {
                if matches!(k.overload.as_str(), "FlashingButton" | "SamplerSlider") {
                    k.overload.clear();
                }
                stock(k);
            }
        }
        stock(&mut host);
        sp.children.push(host);
        let mut p = sp_path;
        p.push(sp.children.len() - 1);
        Ok(Sampler { host: Some(p), open: false, target: None, taken: 0 })
    }

    /// Open or close the splitter for the pad mode; true when it changed
    /// (the pad matrix must be laid out again).
    pub fn sync(&mut self, app: &mut App, model: &Model) -> bool {
        let Some(hp) = self.host.clone() else { return false };
        let open = model.pad_mode(model.engine) == dreist_engine::live::PadMode::Sampler;
        let changed = open != self.open;
        if changed {
            self.open = open;
            if let Some(sp) = app.ctrl_mut(&hp[..hp.len() - 1]) {
                sp.set_attr("intSplitPosition", if open { OPEN_HEIGHT } else { 0 }.to_string());
                sp.layout_splitter();
                for c in &mut sp.children {
                    if c.name == "PadsMatrix" {
                        c.set_attr("intTopSpacing", if open { "2" } else { "0" }.into());
                    }
                }
            }
            if let Some(h) = app.ctrl_mut(&hp) {
                h.visible = open;
            }
        }
        if !open {
            return changed;
        }
        let editing = app.is_editing();
        let s = model.sampler.clone();
        let recording = s.state == 2;
        let Some(h) = app.ctrl_mut(&hp) else { return changed };
        h.walk_mut(&mut |c: &mut Ctrl| match c.name.as_str() {
            "ArmButton" => {
                c.on = s.state >= 1;
                c.enabled = !recording;
                c.set_attr("flashing", (s.state == 1).to_string());
            }
            "RecButton" => {
                c.on = recording;
                c.text = if recording { "Stop" } else { "Rec" }.into();
            }
            "SourceList" => {
                c.enabled = s.state == 0;
                c.text = source_items()[item_of_source(s.source)].clone();
            }
            "TriggerMode" => {
                c.enabled = !recording;
                c.text = TRIGGER_MODES.get(s.start_mode as usize).copied().unwrap_or("").into();
            }
            "Follow" => c.text = AUTO_MODES[s.auto_mode.min(5)].into(),
            "ThresholdPanel" => c.visible = s.start_mode == 3,
            "SyncPanel" => c.visible = matches!(s.start_mode, 2 | 4),
            "BeatsPanel" => c.visible = s.beats_mode,
            "SecondsPanel" => c.visible = !s.beats_mode,
            "SyncButtonGroup" => {
                c.enabled = !recording;
                for (i, b) in c.children.iter_mut().enumerate() {
                    b.on = (i == 0) == s.beats_mode;
                    b.enabled = !recording;
                }
            }
            "SecsTextBox" | "BarsTextBox" | "BeatsTextBox" => {
                c.enabled = !recording;
                if !editing {
                    c.text = match c.name.as_str() {
                        "SecsTextBox" => format!("{:.3}", s.secs),
                        "BarsTextBox" => s.bars.to_string(),
                        _ => s.beats.to_string(),
                    };
                }
            }
            "InputGain" => c.value = s.input_gain * 0.25,
            "PreviewVolume" => c.value = s.preview_volume,
            "MonoButton" => c.text = ["St", "L", "R"][(s.channels as usize).min(2)].into(),
            "ThruButton" => c.on = s.thru,
            "StartThresholdSlider" => c.value = level_to_value(s.start_threshold),
            "StopThresholdSlider" => c.value = level_to_value(s.stop_threshold),
            _ => {}
        });
        changed
    }

    /// A finished take ([re:c5233e] →
    /// `DoSendTakeToPadAutoExport` [re:b3b498]): saved as
    /// "<HH-MM-SS> Take <n>.wav" (peaks above 0 dBFS scaled to 0.998),
    /// then allocated by the Follow mode.
    pub fn take_finished(&mut self, model: &mut Model, opts: &crate::options::Options, left: &[f32], right: &[f32], sr: f32, out: &mut Vec<Command>) -> Option<TakeAction> {
        if left.is_empty() {
            return None;
        }
        let peak = left.iter().chain(right).fold(0.0f32, |m, v| m.max(v.abs()));
        let g = if peak > 1.0 { 0.998 / peak } else { 1.0 };
        let (l, r): (Vec<f32>, Vec<f32>) = (left.iter().map(|v| v * g).collect(), right.iter().map(|v| v * g).collect());
        let dir = record_dir(opts);
        std::fs::create_dir_all(&dir).ok()?;
        self.taken += 1;
        let (_, _, _, h, mi, se) = now_utc();
        let file = free_filename(&dir, &format!("{h:02}-{mi:02}-{se:02} Take {}", self.taken))?;
        dreist_engine::write_wav(&file, sr.round() as u32, &l, if r.is_empty() { &l } else { &r }).ok()?;
        let mode = model.sampler.auto_mode;
        match mode {
            0 => {
                out.push(msg(id::SAMPLER_SET_NOT_ARMED, &[], &[0.0]));
                Some(TakeAction::Slice(file))
            }
            5 => Some(TakeAction::Saved),
            _ => {
                let e = model.engine;
                let target = self.target.unwrap_or((e, model.pad[e], model.layer[e]));
                let used = |l: usize| model.layer(target.0, target.1, l).valid;
                let (at, next) = allocate(mode, target, model.layer[target.0], &used);
                crate::browser::load_sample_into_layer(model, at.0, at.1, at.2, &file);
                self.target = next;
                out.push(msg(if next.is_some() { id::SAMPLER_SET_ARMED } else { id::SAMPLER_SET_NOT_ARMED }, &[], &[0.0]));
                out.push(model.rebuild_command());
                Some(TakeAction::Loaded)
            }
        }
    }

    /// [re:0fd0d9] (at messaging start):
    /// the last start mode (default Threshold; 0 also maps to Threshold),
    /// beats sync, seconds, bars/beats, thresholds, channel mode and thru,
    /// re-sent to the sampler.
    pub fn startup_messages(model: &mut Model) -> Vec<Command> {
        let o = crate::options::Options::load();
        let s = &mut model.sampler;
        let start = match o.int("SamplerLastStartMode", 3) {
            0 => 3,
            n => n.clamp(0, 4),
        };
        s.start_mode = start as u8;
        s.beats_mode = o.bool("SamplerLastSyncMode", true);
        s.secs = crate::conv::number(&o.string("SamplerLastSeconds", "1")).unwrap_or(0.0).clamp(0.0, 3600.0);
        s.bars = o.string("SamplerLastBars", "1").trim().parse::<i32>().unwrap_or(0).max(0) as u32;
        s.beats = o.string("SamplerLastBeats", "0").trim().parse::<i32>().unwrap_or(0).max(0) as u32;
        s.start_threshold = o.string("SamplerLastStartThreshold", "0.1").trim().parse().unwrap_or(0.1);
        s.stop_threshold = o.string("SamplerLastStopThreshold", "0.00562").trim().parse().unwrap_or(0.00562);
        s.channels = o.int("SamplerLastMonoMode", 0).clamp(0, 2) as u8;
        s.thru = o.bool("SamplerLastThruMode", false);
        vec![
            msg(id::SAMPLER_START_MODE, &[], &[start as f32]),
            msg(id::SAMPLER_BEATS_MODE, &[], &[s.beats_mode as i32 as f32]),
            msg(id::SAMPLER_RECORD_SECS, &[], &[s.secs]),
            msg(id::SAMPLER_BARS_BEATS, &[], &[s.bars as f32, s.beats as f32]),
            msg(id::SAMPLER_START_THRESHOLD, &[], &[s.start_threshold]),
            msg(id::SAMPLER_STOP_THRESHOLD, &[], &[s.stop_threshold]),
            msg(id::SAMPLER_MONO, &[], &[s.channels as f32]),
            msg(id::SAMPLER_THRU, &[], &[s.thru as i32 as f32]),
        ]
    }

    fn mine(&self, path: &[usize]) -> bool {
        self.open && self.host.as_ref().is_some_and(|h| path.starts_with(h))
    }

    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        let s = &mut model.sampler;
        match ev {
            UiEvent::Clicked { path, name, on, .. } if self.mine(path) => {
                let parent = app.ctrl(&path[..path.len() - 1]).map(|c| c.name.clone()).unwrap_or_default();
                if parent == "SyncButtonGroup" {
                    let beats = *path.last().unwrap() == 0;
                    s.beats_mode = beats;
                    out.push(msg(id::SAMPLER_BEATS_MODE, &[], &[beats as i32 as f32]));
                    save_opt("SamplerLastSyncMode", beats);
                    return true;
                }
                match name.as_str() {
                    "ArmButton" => {
                        if s.state == 0 {
                            let e = model.engine;
                            self.target = Some((e, model.pad[e], model.layer[e]));
                        }
                        out.push(msg(id::SAMPLER_TOGGLE_ARMED, &[], &[0.0]));
                    }
                    "RecButton" => {
                        if s.state == 0 || self.target.is_none() {
                            let e = model.engine;
                            self.target = Some((e, model.pad[e], model.layer[e]));
                        }
                        out.push(msg(id::SAMPLER_TOGGLE_RECORD, &[], &[0.0]));
                    }
                    "ThruButton" => {
                        s.thru = *on;
                        out.push(msg(id::SAMPLER_THRU, &[], &[*on as i32 as f32]));
                        save_opt("SamplerLastThruMode", *on);
                    }
                    "MonoButton" => app.open_menu(path, CHANNEL_MODES.iter().map(|x| x.to_string()).collect(), Some(s.channels as usize)),
                    "TriggerMode" => app.open_menu(path, TRIGGER_MODES.iter().map(|x| x.to_string()).collect(), Some(s.start_mode as usize)),
                    "SourceList" => app.open_menu(path, source_items(), Some(item_of_source(s.source))),
                    "Follow" => app.open_menu(path, AUTO_MODES.iter().map(|x| x.to_string()).collect(), None),
                    _ => return false,
                }
                true
            }
            UiEvent::MenuSelected { anchor, name, index } if self.mine(anchor) => {
                match name.as_str() {
                    "MonoButton" => {
                        s.channels = (*index).min(2) as u8;
                        s.state = 0;
                        out.push(msg(id::SAMPLER_MONO, &[], &[*index as f32]));
                        save_opt("SamplerLastMonoMode", (*index).min(2));
                    }
                    "TriggerMode" => {
                        s.start_mode = (*index).min(4) as u8;
                        out.push(msg(id::SAMPLER_START_MODE, &[], &[*index as f32]));
                        save_opt("SamplerLastStartMode", (*index).min(4));
                    }
                    "SourceList" => {
                        s.source = source_of_item(*index);
                        out.push(msg(id::SAMPLER_RECORD_SOURCE, &[], &[s.source as f32]));
                    }
                    "Follow" => {
                        if let Some(c) = app.ctrl_mut(anchor) {
                            c.text = AUTO_MODES[(*index).min(5)].into();
                        }
                        s.auto_mode = (*index).min(5);
                        out.push(msg(id::SAMPLER_AUTO_MODE, &[], &[*index as f32]));
                    }
                    _ => return false,
                }
                true
            }
            // `HandleControlEvent` [re:97cc15] event 8 (double click): In
            // 1.0, Thru 0.7937, Start 0.1, Stop 0.00562.
            UiEvent::DoubleClicked { path, name } if self.mine(path) => {
                match name.as_str() {
                    "InputGain" => {
                        s.input_gain = 1.0;
                        out.push(msg(id::SAMPLER_INPUT_GAIN, &[], &[1.0]));
                    }
                    "PreviewVolume" => {
                        s.preview_volume = 0.793_700_5;
                        out.push(msg(SET_PREVIEW_VOLUME, &[], &[0.793_700_5]));
                    }
                    "StartThresholdSlider" => {
                        s.start_threshold = 0.1;
                        out.push(msg(id::SAMPLER_START_THRESHOLD, &[], &[0.1]));
                        if 0.1 < s.stop_threshold {
                            s.stop_threshold = 0.1;
                            out.push(msg(id::SAMPLER_STOP_THRESHOLD, &[], &[0.1]));
                        }
                        save_opt("SamplerLastStartThreshold", s.start_threshold);
                        save_opt("SamplerLastStopThreshold", s.stop_threshold);
                    }
                    "StopThresholdSlider" => {
                        s.stop_threshold = 0.00562;
                        out.push(msg(id::SAMPLER_STOP_THRESHOLD, &[], &[0.00562]));
                        if 0.00562 > s.start_threshold {
                            s.start_threshold = 0.00562;
                            out.push(msg(id::SAMPLER_START_THRESHOLD, &[], &[0.00562]));
                        }
                        save_opt("SamplerLastStartThreshold", s.start_threshold);
                        save_opt("SamplerLastStopThreshold", s.stop_threshold);
                    }
                    _ => return false,
                }
                true
            }
            UiEvent::ValueChanged { path, name, value, .. } if self.mine(path) => {
                match name.as_str() {
                    "InputGain" => {
                        s.input_gain = value * 4.0;
                        out.push(msg(id::SAMPLER_INPUT_GAIN, &[], &[s.input_gain]));
                    }
                    "PreviewVolume" => {
                        s.preview_volume = *value;
                        out.push(msg(SET_PREVIEW_VOLUME, &[], &[*value]));
                    }
                    // `SetStartThreshold02` [re:a2dcfe]: a start below the stop
                    // threshold drags the stop threshold down with it.
                    "StartThresholdSlider" => {
                        let l = value_to_level(*value);
                        s.start_threshold = l;
                        out.push(msg(id::SAMPLER_START_THRESHOLD, &[], &[l]));
                        if l < s.stop_threshold {
                            s.stop_threshold = l;
                            out.push(msg(id::SAMPLER_STOP_THRESHOLD, &[], &[l]));
                        }
                        save_opt("SamplerLastStartThreshold", s.start_threshold);
                        save_opt("SamplerLastStopThreshold", s.stop_threshold);
                    }
                    "StopThresholdSlider" => {
                        let l = value_to_level(*value);
                        s.stop_threshold = l;
                        out.push(msg(id::SAMPLER_STOP_THRESHOLD, &[], &[l]));
                        if l > s.start_threshold {
                            s.start_threshold = l;
                            out.push(msg(id::SAMPLER_START_THRESHOLD, &[], &[l]));
                        }
                        save_opt("SamplerLastStartThreshold", s.start_threshold);
                        save_opt("SamplerLastStopThreshold", s.stop_threshold);
                    }
                    _ => return false,
                }
                true
            }
            UiEvent::TextCommitted { path, name, text } if self.mine(path) => {
                match name.as_str() {
                    "SecsTextBox" => {
                        let v = crate::conv::number(text).unwrap_or(s.secs).clamp(0.0, 3600.0);
                        s.secs = v;
                        out.push(msg(id::SAMPLER_RECORD_SECS, &[], &[v]));
                        save_opt("SamplerLastSeconds", format!("{v:.3}"));
                    }
                    "BarsTextBox" | "BeatsTextBox" => {
                        let n = text.trim().parse::<i32>().unwrap_or(0).max(0) as u32;
                        if name == "BarsTextBox" {
                            s.bars = n.min(16)
                        } else {
                            s.beats = n
                        }
                        out.push(msg(id::SAMPLER_BARS_BEATS, &[], &[s.bars as f32, s.beats as f32]));
                        save_opt("SamplerLastBars", s.bars);
                        save_opt("SamplerLastBeats", s.beats);
                    }
                    _ => return false,
                }
                true
            }
            _ => false,
        }
    }
}

/// `Options::Write*` for the sampler's `SamplerLast*` keys.
fn save_opt(key: &str, v: impl ToString) {
    let mut o = crate::options::Options::load();
    o.set(key, v);
    o.save();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_follows_the_mode() {
        let none = |_| false;
        // Next Layer walks the layers, then disarms.
        assert_eq!(allocate(2, (0, 3, 6), 0, &none), ((0, 3, 6), Some((0, 3, 7))));
        assert_eq!(allocate(2, (0, 3, 7), 0, &none), ((0, 3, 7), None));
        // Next Pad takes the first free layer and moves on a pad.
        let used = |l| l < 2;
        assert_eq!(allocate(3, (0, 5, 0), 0, &used), ((0, 5, 2), Some((0, 6, 2))));
        assert_eq!(allocate(3, (0, 63, 0), 0, &none).1, None);
        // Next Layer And Pad wraps layers into pads.
        assert_eq!(allocate(4, (1, 2, 7), 0, &none), ((1, 2, 7), Some((1, 3, 0))));
        // Single uses the selected layer and disarms.
        assert_eq!(allocate(1, (0, 4, 0), 5, &none), ((0, 4, 5), None));
    }

    #[test]
    fn free_filenames_count_up() {
        let dir = std::env::temp_dir().join(format!("dreist-free-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = free_filename(&dir, "x").unwrap();
        assert!(a.ends_with("x.wav"));
        std::fs::write(&a, b"").unwrap();
        assert!(free_filename(&dir, "x").unwrap().ends_with("x (2).wav"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn meter_mapping_round_trips() {
        for l in [0.001f32, 0.0056, 0.1, 0.5, 1.0] {
            assert!((value_to_level(level_to_value(l)) - l).abs() < 1e-4 * l.max(1e-3), "{l}");
        }
        assert_eq!(level_to_value(0.0), 0.0);
        // 0 dBFS sits at 0.8 of the travel.
        assert!((level_to_value(1.0) - 0.8).abs() < 1e-6);
    }
}
