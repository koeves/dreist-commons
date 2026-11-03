//! Pad inspector ([re:8a338a]): pad header ([re:a94707]),
//! layer list ([re:ecfa09] rows in `LayerTable`) and the layer
//! panel ([re:c9e31a] with the wave display and `LayerParams.skin`).
//!
//! - [re:f38cd2]: vertical gradient
//!   colFill → colAltFill (at the middle) → colFill.
//! - [re:d2ec05]: centre line at α0.5, then per
//!   pixel column the min/max envelope in `colLine`.
//! - [re:335a01]: start/end markers as 2 px lines
//!   in colStartMarker/colEndMarker with a round handle (black centre).
//! - [re:e0ce2b]: body in colFill,
//!   header strip in colAltFill with the display name in colText.
//!
//! Knobs/sliders in `LayerParams` are named after the layer parameters
//! (`Cutoff`, `Resonance` and `PreDelayTime` alias `FilterCutoff`,
//! `FilterResonance`, `PreDelay`) and post `e_setLayerParameter` 0x66.

use std::cell::RefCell;
use std::path::PathBuf;

use dreist_engine::binding::id;
use dreist_formats::LayerParam;
use dreist_gui::app::{App, UiEvent};
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::render::Renderer;
use dreist_gui::{Colour, Ctrl, Rect, Skin};

use crate::command::{Command, msg};
use crate::model::{LAYERS, Model};
use crate::{EditorError, err};

const WAVE_STYLE: &str = "WaveDisplayStyle";

/// The layer settings behind the LayerParams toggles: Reverse
/// `IsReversed`, Phase `PhasePosition` < 0, Env Mode `EnvTracksPitch`,
/// LayerMode `SynthesisMode` = VintageSampler.
pub fn layer_toggle_state(node: Option<&dreist_formats::Node>, name: &str) -> Option<bool> {
    let truthy = |k: &str| node.is_some_and(|n| n.truthy(k));
    Some(match name {
        "Reverse" => truthy("IsReversed"),
        "Phase" => node.and_then(|n| n.f32("PhasePosition")).is_some_and(|v| v < 0.0),
        "EnvMode" => truthy("EnvTracksPitch"),
        "LayerMode" => node.and_then(|n| n.attr("SynthesisMode")).is_some_and(|v| v.trim() == "VintageSampler"),
        _ => return None,
    })
}

pub fn param_for_control(name: &str) -> Option<LayerParam> {
    let n = match name {
        "Cutoff" => "FilterCutoff",
        "Resonance" => "FilterResonance",
        "PreDelayTime" => "PreDelay",
        n => n,
    };
    LayerParam::from_name(n)
}

/// Min/max per bucket of a sample's left channel.
struct Peaks {
    buckets: Vec<(f32, f32)>,
    frames: usize,
    /// File sample rate (envelope overlay times).
    rate: f32,
    /// Zero crossings of the left channel (`BuildZeroCrossingSearchMap`).
    zeros: Vec<u32>,
}

/// Waveforms kept (most recently shown).
const PEAKS_KEPT: usize = 16;

/// Peaks of the samples shown: reloaded when the file changes on disk (a
/// layer sample saved over it, a missing file appearing), the least
/// recently shown dropped past [`PEAKS_KEPT`].
#[derive(Default)]
struct PeaksCache {
    /// (file, modification time, peaks); most recent last.
    entries: Vec<(PathBuf, Option<std::time::SystemTime>, Option<Peaks>)>,
}

impl PeaksCache {
    fn get(&mut self, path: &std::path::Path) -> Option<&Peaks> {
        let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let entry = match self.entries.iter().position(|e| e.0 == path) {
            Some(i) => {
                let e = self.entries.remove(i);
                if e.1 == stamp { e } else { (e.0, stamp, load_peaks(path)) }
            }
            None => (path.to_path_buf(), stamp, load_peaks(path)),
        };
        self.entries.push(entry);
        if self.entries.len() > PEAKS_KEPT {
            self.entries.remove(0);
        }
        self.entries.last().and_then(|e| e.2.as_ref())
    }
}

/// Marker frames of a layer: (frames, start, end, loop start, loop end,
/// looping).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaveMarkers {
    pub frames: u32,
    pub start: u32,
    pub end: u32,
    pub loop_start: u32,
    pub loop_end: u32,
    pub looping: bool,
}

/// The layer's [re:f79f64] (whole file when absent).
pub fn wave_markers(model: &Model, e: usize, p: usize, l: usize, frames: u32) -> WaveMarkers {
    let n = model.layer_node(e, p, l);
    let mk = |name: &str| n.and_then(|n| n.child("AudioFileMarkers", name)).map(|m| (m.u32("StartSample").unwrap_or(0), m.u32("EndSample").unwrap_or(frames)));
    let (start, end) = mk("StartEndMarkers").unwrap_or((0, frames));
    let (loop_start, loop_end) = mk("LoopMarkers").unwrap_or((start, end));
    let looping = n.and_then(|n| n.attr("PlaybackMode")).map(dreist_dsp::voice::PlaybackMode::canonical) == Some("Looping");
    WaveMarkers { frames, start: start.min(frames), end: end.min(frames), loop_start: loop_start.min(frames), loop_end: loop_end.min(frames), looping }
}

/// [re:808270] snapping: the nearest zero crossing within
/// `range` frames.
pub fn snap_zero(zeros: &[u32], f: u32, range: u32) -> u32 {
    let i = zeros.partition_point(|&z| z < f);
    let best = [i.checked_sub(1), Some(i)].into_iter().flatten().filter_map(|k| zeros.get(k)).min_by_key(|&&z| (z as i64 - f as i64).abs());
    match best {
        Some(&z) if (z as i64 - f as i64).unsigned_abs() < range as u64 => z,
        _ => f,
    }
}

pub struct Inspector {
    /// The [re:d46410] panel (the zoom bar's controls are searched under it).
    host: Option<Vec<usize>>,
    params: Option<Vec<usize>>,
    table: Option<Vec<usize>>,
    pad_header: Option<Vec<usize>>,
    peaks: RefCell<PeaksCache>,
    /// Marker drag: object 1 start, 2 end, 3 loop start, 4 loop end
    /// (`GetDragObject`), and the markers when it started.
    drag: Option<(u8, WaveMarkers)>,
    /// Visible part of the file (start, length) as fractions.
    pub(crate) view: (f32, f32),
    /// Pad options panel ([re:fe83c5]).
    vsplit: crate::velsplit::VelSplit,
    /// Zoom slider drag (mode, last x).
    zoom_drag: Option<(u8, f32)>,
    /// Auto zoom mode (`AutoZoomGroup`, option `WaveDisplayZoomMode`).
    zoom_mode: i64,
    /// The layer and file the auto zoom last ran for.
    zoom_key: Option<(usize, usize, usize, PathBuf)>,
    /// Envelope overlays (options `AmpEnvVisible` / `FreeEnvVisible`).
    env_visible: (bool, bool),
    /// Envelope handle drag: (Amp?, handle 0..7).
    env_drag: Option<(bool, usize)>,
    /// Timeline in Beats mode (option `PadEditWaveDisplayTimelineMode`).
    timeline_beats: bool,
    /// The timeline's Time / Beats menu is open on this control.
    timeline_menu: Option<Vec<usize>>,
}

const PEAK_BUCKETS: usize = 2048;

/// Visit the controls tagged `layerZoom` (the layer wave display's zoom bar).
fn walk_tagged(c: &mut Ctrl, f: &mut dyn FnMut(&mut Ctrl)) {
    for k in &mut c.children {
        if k.attr("layerZoom").is_some() {
            f(k);
        }
        walk_tagged(k, f);
    }
}

fn find_tagged<'a>(c: &'a Ctrl, name: &str) -> Option<&'a Ctrl> {
    c.children.iter().find_map(|k| if k.attr("layerZoom").is_some() && k.name == name { Some(k) } else { find_tagged(k, name) })
}

fn load_peaks(path: &std::path::Path) -> Option<Peaks> {
    let mut cache = dreist_engine::audio::SampleCache::default();
    let data = cache.get(path)?;
    let n = data.left.len();
    if n == 0 {
        return None;
    }
    let per = n.div_ceil(PEAK_BUCKETS).max(1);
    let buckets = data.left.chunks(per).map(|c| c.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)))).collect();
    let zeros = data.left.windows(2).enumerate().filter(|(_, w)| (w[0] <= 0.0) != (w[1] <= 0.0)).map(|(i, _)| i as u32 + 1).collect();
    Some(Peaks { buckets, frames: n, zeros, rate: data.sample_rate.max(1.0) })
}

impl Inspector {
    pub fn build(skin: &mut Skin, root: &mut Ctrl) -> Result<Inspector, EditorError> {
        let Some(host) = root.find_mut("PadInspector") else {
            return Ok(Inspector {
                host: None,
                params: None,
                table: None,
                pad_header: None,
                peaks: RefCell::default(),
                drag: None,
                view: (0.0, 1.0),
                vsplit: Default::default(),
                zoom_drag: None,
                zoom_mode: 0,
                zoom_key: None,
                env_visible: (true, true),
                env_drag: None,
                timeline_beats: crate::timeline::beats_mode("PadEdit"),
                timeline_menu: None,
            });
        };
        skin.mount(host, "PadInspector.skin").map_err(err)?;
        if let Some(lp) = host.find_mut("LayerPanel") {
            if lp.children.is_empty() {
                skin.mount(lp, "LayerPanel.skin").map_err(err)?;
            }
        }
        if let Some(sc) = host.find_mut("ParamScroller") {
            let rel = skin.find("LayerParams.skin").unwrap_or_else(|| "Pad Editor/LayerParams.skin".into());
            let p = skin.load_skin(&rel).map_err(err)?;
            for mut c in p.children {
                let h = sc.rect.h.min(c.rect.h).max(c.rect.h.min(sc.rect.h));
                let w = c.rect.w;
                c.set_rect(Rect::new(0, 0, w, h));
                sc.children.push(c);
            }
        }
        // The layer wave display's zoom bar (tagged so the slicer's
        // identically named controls are left alone).
        for n in ["WaveZoomSlider", "ZoomInButton", "ZoomOutButton", "AutoZoomGroup", "ShowAmpEnvButton", "ShowFreeEnvButton"] {
            if let Some(c) = host.find_mut(n) {
                c.set_attr("layerZoom", "true".into());
            }
        }
        if let Some(ps) = host.find_mut("PadSelButton") {
            skin.mount(ps, "PadSelectionButton.skin").map_err(err)?;
        }
        if let Some(t) = host.find_mut("LayerTable") {
            let rel = skin.find("LayerSelectionButton.skin").unwrap_or_else(|| "Pad Editor/LayerSelectionButton.skin".into());
            if let Some(row) = skin.load_skin(&rel).ok().and_then(|r| r.children.into_iter().next()) {
                let w = t.rect.w;
                for l in 0..LAYERS {
                    let mut r = row.clone();
                    r.overload = "LayerRow".into();
                    r.set_attr("layer", l.to_string());
                    r.set_rect(Rect::new(0, l as i32 * (row.rect.h + 1), w, row.rect.h));
                    if let Some(n) = r.find_mut("SampleName") {
                        n.passive = true;
                    }
                    t.children.push(r);
                }
            }
        }
        let vsplit = crate::velsplit::VelSplit::build(skin, root)?;
        Ok(Inspector {
            host: root.path_of("PadInspector"),
            params: root.path_of("LayerParams"),
            table: root.path_of("LayerTable"),
            pad_header: root.path_of("PadSelButton"),
            peaks: RefCell::default(),
            drag: None,
            view: (0.0, 1.0),
            vsplit,
            zoom_drag: None,
            zoom_mode: crate::options::Options::load().int("WaveDisplayZoomMode", 0),
            zoom_key: None,
            env_visible: {
                let o = crate::options::Options::load();
                (o.bool("AmpEnvVisible", true), o.bool("FreeEnvVisible", true))
            },
            env_drag: None,
            timeline_beats: crate::timeline::beats_mode("PadEdit"),
            timeline_menu: None,
        })
    }

    /// Cursor for the pointer over the layer wave display (a marker:
    /// [re:840c1e] → left / right when
    /// `GetDragObject` finds one) or its zoom slider.
    pub fn hover_cursor(&self, app: &App, model: &Model, path: &[usize], overload: &str, x: f32, y: f32) -> Option<dreist_gui::cursor::Cursor> {
        use dreist_gui::cursor::Cursor;
        let (w, h) = app.ctrl(path).map(|c| (c.rect.w as f32, c.rect.h as f32))?;
        match overload {
            "WaveDisplayZoomSlider" if app.ctrl(path).is_some_and(|c| c.attr("layerZoom").is_some()) => {
                Some(if crate::zoomslider::over_edge(self.view, w, h, x, y) { Cursor::LeftRight } else { Cursor::Arrow })
            }
            "LayerWaveDisplay" => {
                let e = model.engine;
                let (p, l) = (model.pad[e], model.layer[e]);
                let (_, frames) = self.layer_frames(model)?;
                let m = wave_markers(model, e, p, l, frames);
                let (v0, vl) = self.view;
                let to_x = |f: u32| ((f as f32 / frames.max(1) as f32) - v0) / vl * w;
                let mut cands = vec![m.start, m.end];
                if m.looping {
                    cands.extend([m.loop_start, m.loop_end]);
                }
                Some(if cands.into_iter().any(|f| (to_x(f) - x).abs() < 4.0) { Cursor::LeftRight } else { Cursor::Arrow })
            }
            _ => None,
        }
    }

    /// The wave display's tooltip at (x, y) ([re:583792]
    ///  [re:49ff99]): an envelope handle — "Amp Envelope " /
    /// "Free Envelope " and its stage ("Attack: 0.25 sec", "Decay: … sec,
    /// Sustain: 050%" when the sustain handle is hidden, curves "Attack
    /// Curve: +20%") — else the marker under the pointer.
    pub fn wave_tooltip(&self, app: &App, model: &Model, path: &[usize], x: f32, y: f32) -> Option<String> {
        let e = model.engine;
        let (p, l) = (model.pad[e], model.layer[e]);
        let c = app.ctrl(path)?;
        let w = c.rect.w as f32;
        let file = model.layer_sample(e, p, l)?;
        let mut cache = self.peaks.borrow_mut();
        let pk = cache.get(&file)?;
        let frames = pk.frames as u32;
        let (v0, vl) = self.view;
        let m = wave_markers(model, e, p, l, frames);
        let h = c.rect.h as f32 - crate::timeline::WAVE_TOP;
        let geo = EnvGeo { w, y0: crate::timeline::WAVE_TOP, h, v0, vl, frames: frames as f32, rate: pk.rate, start: m.start, end: m.end };
        if let Some((amp, k)) = env_hit(model, e, p, l, self.env_visible, &geo, x, y) {
            let v = |i: usize| model.layer_param(e, p, l, env_param(amp, i));
            let secs = |i: usize, max: f32| v(i).powi(4) * max;
            let sus = (v(2) * 100.0) as i64;
            let mode = model.layer_node(e, p, l).and_then(|n| n.attr("PlaybackMode")).map_or("OneShot", dreist_dsp::voice::PlaybackMode::canonical).to_string();
            let (ay, ah) = geo.area(amp, self.env_visible)?;
            let sustain_shown = env_handles(model, e, p, l, amp, &mode, &geo, ay, ah).iter().any(|(i, _)| *i == 2);
            let curve = |i: usize| (v(i) * 200.0 - 100.0) as i64;
            let stage = match k {
                0 => format!("Attack: {:.2} sec", secs(0, 10.0)),
                1 if !sustain_shown => format!("Decay: {:.2} sec, Sustain: {sus:03}%", secs(1, 10.0)),
                1 => format!("Decay: {:.2} sec", secs(1, 10.0)),
                2 => format!("Sustain: {sus:03}%"),
                3 if !sustain_shown => format!("Hold: {:.2} sec, Sustain: {sus:03}%", secs(3, 60.0)),
                3 => format!("Hold: {:.2} sec", secs(3, 60.0)),
                4 => format!("Release: {:.2} sec", secs(4, 15.0)),
                5 => format!("Attack Curve: {:+3}%", curve(5)),
                6 => format!("Decay Curve: {:+3}%", curve(6)),
                _ => format!("Release Curve: {:+3}%", curve(7)),
            };
            return Some(format!("{}{stage}", if amp { "Amp Envelope " } else { "Free Envelope " }));
        }
        let to_x = |f: u32| ((f as f32 / frames.max(1) as f32) - v0) / vl * w;
        let mut cands = vec![(m.start, "Start marker"), (m.end, "End marker")];
        if m.looping {
            cands.extend([(m.loop_start, "Loop start marker"), (m.loop_end, "Loop end marker")]);
        }
        cands.into_iter().find(|(f, _)| (to_x(*f) - x).abs() < 4.0).map(|(_, n)| n.to_string())
    }

    /// Frames of the selected layer's sample (peaks cache).
    fn layer_frames(&self, model: &Model) -> Option<(PathBuf, u32)> {
        let e = model.engine;
        let file = model.layer_sample(e, model.pad[e], model.layer[e])?;
        let mut cache = self.peaks.borrow_mut();
        let frames = cache.get(&file)?.frames as u32;
        Some((file, frames))
    }

    /// Apply the auto zoom mode to the selected layer's markers.
    fn apply_auto_zoom(&mut self, model: &Model) {
        let e = model.engine;
        let (p, l) = (model.pad[e], model.layer[e]);
        let Some((_, frames)) = self.layer_frames(model) else { return };
        let m = wave_markers(model, e, p, l, frames);
        if let Some(v) = crate::zoomslider::auto_zoom(self.zoom_mode, frames, m.start, m.end) {
            self.view = v;
        }
    }

    /// SP / EP / LS / LE (`LoadToolbarControls` names PS / PE / LS / LE):
    /// hidden, shown only in MIDI learn mode ([re:7fd1e4]
    /// [re:cca449]) as learn targets for the markers.
    pub fn learn_buttons(&self, app: &mut App, learning: bool) {
        let Some(c) = self.params.as_ref().and_then(|p| app.ctrl_mut(p)) else { return };
        fn walk(c: &mut Ctrl, on: bool) {
            for k in &mut c.children {
                if matches!(k.name.as_str(), "PS" | "PE" | "LS" | "LE") && k.class == "CtrlButton" {
                    k.visible = on;
                    k.enabled = on;
                }
                walk(k, on);
            }
        }
        walk(c, learning);
    }

    /// The marker message a learn press on SP / EP / LS / LE binds
    /// (`GetBindingForControl` [re:ad8531]: 0x7a..0x7d "Play Start", "Play End",
    /// "Loop Start", "Loop End").
    pub fn marker_learn(&self, app: &App, path: &[usize]) -> Option<(u32, &'static str)> {
        if !self.params.as_ref().is_some_and(|p| path.starts_with(p)) {
            return None;
        }
        use dreist_engine::binding::id;
        match app.ctrl(path)?.name.as_str() {
            "PS" => Some((id::SET_LAYER_START_SAMPLE01, "Play Start")),
            "PE" => Some((id::SET_LAYER_END_SAMPLE01, "Play End")),
            "LS" => Some((id::SET_LAYER_LOOP_START_SAMPLE01, "Loop Start")),
            "LE" => Some((id::SET_LAYER_LOOP_END_SAMPLE01, "Loop End")),
            _ => None,
        }
    }

    pub fn sync(&mut self, app: &mut App, model: &Model) {
        self.vsplit.sync(app, model);
        // [re:87b7a3]: a new layer / file gets the auto zoom
        // (or the whole file when the mode is off).
        {
            let e = model.engine;
            let key = self.layer_frames(model).map(|(f, _)| (e, model.pad[e], model.layer[e], f));
            if key != self.zoom_key {
                self.zoom_key = key;
                self.view = (0.0, 1.0);
                self.apply_auto_zoom(model);
            }
        }
        // Zoom bar states: the mode group, and Zoom in only while the
        // thumb is wider than 20 px.
        let mode = self.zoom_mode;
        let view = self.view;
        let env = self.env_visible;
        if let Some(host) = self.host.as_ref().and_then(|h| app.ctrl_mut(h)) {
            let w = find_tagged(host, "WaveZoomSlider").map(|c| c.rect.w as f32);
            walk_tagged(host, &mut |c: &mut Ctrl| match c.name.as_str() {
                "ShowAmpEnvButton" => c.on = env.0,
                "ShowFreeEnvButton" => c.on = env.1,
                "AutoZoomGroup" => {
                    for (i, b) in c.children.iter_mut().enumerate() {
                        b.on = i as i64 == mode;
                    }
                }
                "ZoomInButton" => {
                    if let Some(w) = w {
                        c.enabled = view.1 * w > 20.0;
                    }
                }
                _ => {}
            });
        }
        let e = model.engine;
        let p = model.pad[e];
        let l = model.layer[e];
        if let Some(pp) = &self.params {
            if let Some(c) = app.ctrl_mut(pp) {
                let source = model.source;
                let src = dreist_formats::TransmodSource::ALL.get(source).copied().unwrap_or(dreist_formats::TransmodSource::ActualParameter);
                let node = model.layer_node(e, p, l);
                fn walk(c: &mut Ctrl, f: &dyn Fn(&mut Ctrl)) {
                    for k in &mut c.children {
                        f(k);
                        walk(k, f);
                    }
                }
                walk(c, &|k: &mut Ctrl| {
                    // [re:ca7824] toggles
                    // (states from `DoUpdateLayerSetting`).
                    if k.class == "CtrlButton" {
                        if let Some(on) = layer_toggle_state(node, &k.name) {
                            k.on = on;
                            return;
                        }
                    }
                    let Some(par) = param_for_control(&k.name) else { return };
                    let base = model.layer_param(e, p, l, par);
                    match k.class.as_str() {
                        "CtrlTransmodKnob" | "CtrlTransmodSlider" => {
                            let depth = src.attr(par.name()).and_then(|a| node.and_then(|n| n.f32(&a))).unwrap_or(0.0);
                            if k.class == "CtrlTransmodKnob" {
                                crate::model::bind_knob(k, source, base, depth);
                            } else {
                                k.value = base;
                            }
                        }
                        "CtrlKnob" | "CtrlSlider" => k.value = base,
                        "CtrlButton" => k.on = base >= 0.5,
                        _ => {}
                    }
                });
            }
        }
        if let Some(hp) = &self.pad_header {
            let name = model.pad_name(e, p).unwrap_or_else(|| format!("Pad {}", p + 1));
            let (m, s) = (model.pad_mute(e, p), model.pad_solo(e, p));
            if let Some(h) = app.ctrl_mut(hp) {
                for c in h.children.iter_mut().flat_map(|c| c.children.iter_mut()) {
                    match c.name.as_str() {
                        "PadName" => c.text = name.clone(),
                        "Mute" => c.on = m,
                        "Solo" => c.on = s,
                        "AllPads" => c.on = model.all_pads,
                        "AllLayers" => c.on = model.all_layers,
                        _ => {}
                    }
                }
            }
        }
        if let Some(tp) = &self.table {
            let (_, lmask) = crate::multipad::layer_selection(model, e, p);
            let _ = l;
            let rows: Vec<(String, bool, bool)> = (0..LAYERS)
                .map(|li| {
                    let f = model.layer(e, p, li);
                    (if f.valid { model.layer_name(e, p, li).unwrap_or_default() } else { String::new() }, f.mute, f.solo)
                })
                .collect();
            if let Some(t) = app.ctrl_mut(tp) {
                for r in &mut t.children {
                    let Some(li) = r.attr("layer").and_then(|v| v.parse::<usize>().ok()) else { continue };
                    let (name, m, s) = &rows[li.min(LAYERS - 1)];
                    r.set_attr("selected", (lmask & (1u8 << li.min(7)) != 0).to_string());
                    for c in &mut r.children {
                        match c.name.as_str() {
                            "SampleName" => c.text = if name.is_empty() { c.attr("placeholder").unwrap_or("").to_string() } else { name.clone() },
                            "Mute" => c.on = *m,
                            "Solo" => c.on = *s,
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        if self.vsplit.handle(app, model, ev, out) {
            return true;
        }
        let e = model.engine;
        let p = model.pad[e];
        let l = model.layer[e];
        let idx = [e as i32, p as i32, l as i32];
        match ev {
            UiEvent::Mouse { path, overload, kind, x, .. } if overload == "LayerWaveDisplay" => {
                let Some(w) = app.ctrl(path).map(|c| c.rect.w as f32) else { return false };
                let Some(file) = model.layer_sample(e, p, l) else { return false };
                let mut cache = self.peaks.borrow_mut();
                let Some(pk) = cache.get(&file) else { return false };
                let frames = pk.frames as u32;
                let (v0, vl) = self.view;
                let to_frame = |x: f32| (((x / w.max(1.0)) * vl + v0).clamp(0.0, 1.0) * frames as f32) as u32;
                let to_x = |f: u32| ((f as f32 / frames.max(1) as f32) - v0) / vl * w;
                // Envelope handles (`GetEnvelopeHandlesForMousePosition`
                // [re:b4faea], `HandleEnvelopeDrag` [re:86bd9d]) come first.
                let h = app.ctrl(path).map(|c| c.rect.h as f32).unwrap_or(0.0) - crate::timeline::WAVE_TOP;
                let geo = EnvGeo {
                    w,
                    y0: crate::timeline::WAVE_TOP,
                    h,
                    v0,
                    vl,
                    frames: frames as f32,
                    rate: pk.rate,
                    start: wave_markers(model, e, p, l, frames).start,
                    end: wave_markers(model, e, p, l, frames).end,
                };
                let y = match ev {
                    UiEvent::Mouse { y, .. } => *y,
                    _ => 0.0,
                };
                // [re:f61cab]: on the timeline
                // strip, the Time / Beats check items.
                if *kind == dreist_gui::app::MouseKind::RightDown && y < crate::timeline::STRIP {
                    let items = ["Time", "Beats"].iter().enumerate().map(|(i, t)| if (i == 1) == self.timeline_beats { format!("✓ {t}") } else { t.to_string() }).collect();
                    let path = path.clone();
                    drop(cache);
                    app.open_menu(&path, items, None);
                    self.timeline_menu = Some(path);
                    return true;
                }
                match kind {
                    dreist_gui::app::MouseKind::Down => {
                        if let Some(hit) = env_hit(model, e, p, l, self.env_visible, &geo, *x, y) {
                            self.env_drag = Some(hit);
                            return true;
                        }
                    }
                    dreist_gui::app::MouseKind::Drag if self.env_drag.is_some() => {
                        let (amp, handle) = self.env_drag.unwrap();
                        for (param, v) in env_drag_values(model, e, p, l, amp, handle, self.env_visible, &geo, *x, y, app.mods.shift) {
                            out.push(msg(id::SET_LAYER_PARAMETER, &idx, &[param as f32, v, 0.0]));
                        }
                        return true;
                    }
                    dreist_gui::app::MouseKind::Up if self.env_drag.is_some() => {
                        self.env_drag = None;
                        return true;
                    }
                    _ => {}
                }
                match kind {
                    dreist_gui::app::MouseKind::Down => {
                        // `GetDragObject`: a marker within 4 px.
                        let m = wave_markers(model, e, p, l, frames);
                        let mut cands = vec![(1u8, m.start), (2, m.end)];
                        if m.looping {
                            cands.push((3, m.loop_start));
                            cands.push((4, m.loop_end));
                        }
                        let hit = cands.into_iter().map(|(o, f)| (o, (to_x(f) - x).abs())).filter(|(_, d)| *d < 4.0).min_by(|a, b| a.1.total_cmp(&b.1));
                        self.drag = hit.map(|(o, _)| (o, m));
                        self.drag.is_some()
                    }
                    dreist_gui::app::MouseKind::Drag => {
                        let Some((obj, m0)) = self.drag else { return false };
                        let mut f = to_frame(*x);
                        // Snap to a zero crossing within 20 px unless shift.
                        if !app.mods.shift {
                            let range = to_frame(20.0).saturating_sub(to_frame(0.0)).max(1);
                            f = snap_zero(&pk.zeros, f, range);
                        }
                        let kinds = [id::SET_LAYER_START_SAMPLE, id::SET_LAYER_END_SAMPLE, id::SET_LAYER_LOOP_START_SAMPLE, id::SET_LAYER_LOOP_END_SAMPLE];
                        let orig = [m0.start, m0.end, m0.loop_start, m0.loop_end];
                        let k = (obj - 1) as usize;
                        out.push(msg(kinds[k], &idx, &[f as f32]));
                        // Alt moves the pair together (start/end, loop start/end).
                        if app.mods.alt {
                            let other = k ^ 1;
                            let moved = (orig[other] as i64 + f as i64 - orig[k] as i64).clamp(0, frames as i64 - 1) as u32;
                            out.push(msg(kinds[other], &idx, &[moved as f32]));
                        }
                        true
                    }
                    // `OnMouseLButtonUp` [re:9718db]: the auto zoom follows the markers.
                    dreist_gui::app::MouseKind::Up => {
                        let was = self.drag.take().is_some();
                        if was {
                            let m = wave_markers(model, e, p, l, frames);
                            if let Some(v) = crate::zoomslider::auto_zoom(self.zoom_mode, frames, m.start, m.end) {
                                self.view = v;
                            }
                        }
                        was
                    }
                    dreist_gui::app::MouseKind::RightDown => false,
                }
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.timeline_menu.as_ref() == Some(anchor) => {
                self.timeline_menu = None;
                self.timeline_beats = *index == 1;
                crate::timeline::save_mode("PadEdit", self.timeline_beats);
                true
            }
            // [re:7197ba]: ZoomIn ×2 / ×0.5 about the
            // sample under the mouse (its fraction of the file).
            UiEvent::Wheel { path, overload, dy } if overload == "LayerWaveDisplay" => {
                let Some(w) = app.ctrl(path).map(|c| c.rect.w as f32) else { return false };
                let Some((_, frames)) = self.layer_frames(model) else { return false };
                let mx = (app.mouse_pos().0 - app.origin(path).0 as f32) / w.max(1.0);
                let at = (self.view.0 + mx * self.view.1).clamp(0.0, 1.0);
                let tw = find_tagged(&app.root, "WaveZoomSlider").map(|c| c.rect.w as f32).unwrap_or(w);
                self.view = crate::zoomslider::zoom_in(self.view, if *dy > 0.0 { 2.0 } else { 0.5 }, at, tw, frames);
                true
            }
            // Zoom bar (`SetZoomButtons` $_3 / $_4: ZoomIn(2, 0.5) / (0.5, 0.5)).
            UiEvent::Clicked { path, name, .. } if (name == "ZoomInButton" || name == "ZoomOutButton") && app.ctrl(path).is_some_and(|c| c.attr("layerZoom").is_some()) => {
                let Some((_, frames)) = self.layer_frames(model) else { return true };
                let tw = find_tagged(&app.root, "WaveZoomSlider").map(|c| c.rect.w as f32).unwrap_or(1.0);
                self.view = crate::zoomslider::zoom_in(self.view, if name == "ZoomInButton" { 2.0 } else { 0.5 }, 0.5, tw, frames);
                true
            }
            // A / F: `UpdateDisplayOptions` [re:682bc8] reads `AmpEnvVisible` /
            // `FreeEnvVisible` (default on).
            UiEvent::Clicked { path, name, on, .. } if (name == "ShowAmpEnvButton" || name == "ShowFreeEnvButton") && app.ctrl(path).is_some_and(|c| c.attr("layerZoom").is_some()) => {
                let key = if name == "ShowAmpEnvButton" { "AmpEnvVisible" } else { "FreeEnvVisible" };
                if name == "ShowAmpEnvButton" {
                    self.env_visible.0 = *on
                } else {
                    self.env_visible.1 = *on
                }
                let mut o = crate::options::Options::load();
                o.set(key, *on);
                o.save();
                true
            }
            // `SetZoomModeButtons` $_9: store the mode and apply it.
            UiEvent::Clicked { path, .. } if path.len() > 1 && app.ctrl(&path[..path.len() - 1]).is_some_and(|c| c.name == "AutoZoomGroup" && c.attr("layerZoom").is_some()) => {
                self.zoom_mode = path[path.len() - 1] as i64;
                let mut o = crate::options::Options::load();
                o.set("WaveDisplayZoomMode", self.zoom_mode);
                o.save();
                self.apply_auto_zoom(model);
                true
            }
            UiEvent::Mouse { path, overload, kind, x, y } if overload == "WaveDisplayZoomSlider" && app.ctrl(path).is_some_and(|c| c.attr("layerZoom").is_some()) => {
                let Some((tw, h)) = app.ctrl(path).map(|c| (c.rect.w as f32, c.rect.h as f32)) else { return false };
                let min = self.layer_frames(model).map(|(_, n)| (64.0 / n.max(1) as f32).min(1.0)).unwrap_or(1.0);
                match kind {
                    dreist_gui::app::MouseKind::Down => {
                        self.zoom_drag = crate::zoomslider::hit(self.view, tw, h, *x, *y).map(|m| (m, *x));
                        self.zoom_drag.is_some()
                    }
                    dreist_gui::app::MouseKind::Drag => {
                        let Some((mode, last)) = self.zoom_drag else { return false };
                        self.view = crate::zoomslider::drag(self.view, tw, mode, last, *x, min);
                        self.zoom_drag = Some((mode, *x));
                        true
                    }
                    dreist_gui::app::MouseKind::Up => self.zoom_drag.take().is_some(),
                    dreist_gui::app::MouseKind::RightDown => false,
                }
            }
            // `OnMouseLButtonDblClick` [re:009295]: the whole file.
            UiEvent::DoubleClicked { path, .. } if app.ctrl(path).is_some_and(|c| c.overload == "WaveDisplayZoomSlider" && c.attr("layerZoom").is_some()) => {
                self.view = (0.0, 1.0);
                true
            }
            // `OnMouseWheel` [re:207f1c]: the thumb moves 5 px.
            UiEvent::Wheel { path, overload, dy } if overload == "WaveDisplayZoomSlider" && app.ctrl(path).is_some_and(|c| c.attr("layerZoom").is_some()) => {
                let Some(tw) = app.ctrl(path).map(|c| c.rect.w as f32) else { return false };
                self.view = crate::zoomslider::clamp((self.view.0 + if *dy > 0.0 { 5.0 } else { -5.0 } / tw.max(1.0), self.view.1));
                true
            }
            UiEvent::ValueChanged { path, name, value, .. } => {
                let Some(pp) = &self.params else { return false };
                if !path.starts_with(pp) {
                    return false;
                }
                let Some(par) = param_for_control(name) else { return false };
                let is_tm = app.ctrl(path).is_some_and(|c| c.class == "CtrlTransmodKnob");
                let (src, v) = crate::model::knob_edit(app.ctrl(path), if is_tm { model.source } else { 0 }, *value);
                out.push(msg(id::SET_LAYER_PARAMETER, &idx, &[par as u8 as f32, v, src as f32]));
                true
            }
            UiEvent::Mouse { path, overload, kind: dreist_gui::app::MouseKind::Down, .. } if overload == "LayerRow" => {
                let Some(li) = app.ctrl(path).and_then(|r| r.attr("layer")).and_then(|v| v.parse::<usize>().ok()) else { return false };
                let (cmd, shift) = (app.mods.cmd, app.mods.shift);
                out.push(crate::multipad::select_layer(model, e, li, cmd, shift));
                true
            }
            UiEvent::Clicked { path, name, on, .. } => {
                if let Some(tp) = &self.table {
                    if path.starts_with(tp) && path.len() > tp.len() + 1 {
                        let Some(li) = app.ctrl(&path[..tp.len() + 1]).and_then(|r| r.attr("layer")).and_then(|v| v.parse::<i32>().ok()) else { return false };
                        let kind = match name.as_str() {
                            "Mute" => id::SET_LAYER_MUTE,
                            "Solo" => id::SET_LAYER_SOLO,
                            _ => return false,
                        };
                        out.push(msg(kind, &[e as i32, p as i32, li], &[*on as i32 as f32]));
                        return true;
                    }
                }
                if let Some(hp) = &self.pad_header {
                    if path.starts_with(hp) && matches!(name.as_str(), "AllPads" | "AllLayers") {
                        // [re:281f54] $_0 / $_1: `e_setModifiers`
                        // 0x26 [all layers, all pads, false].
                        if name == "AllPads" {
                            model.all_pads = *on;
                        } else {
                            model.all_layers = *on;
                        }
                        out.push(msg(id::SET_MODIFIERS, &[], &[model.all_layers as i32 as f32, model.all_pads as i32 as f32, 0.0]));
                        return true;
                    }
                    if path.starts_with(hp) {
                        let kind = match name.as_str() {
                            "Mute" => id::MUTE_PAD,
                            "Solo" => id::SOLO_PAD,
                            _ => return false,
                        };
                        out.push(msg(kind, &[e as i32, p as i32], &[*on as i32 as f32]));
                        return true;
                    }
                }
                if let Some(pp) = &self.params {
                    if path.starts_with(pp) {
                        // Reverse 0x6e, Phase 0x6f, Env Mode 0x70 (bool) and
                        // Vintage / LayerMode 0x6b (the state as an int).
                        let kind = match name.as_str() {
                            "Reverse" => Some(id::SET_LAYER_REVERSE_STATUS),
                            "Phase" => Some(id::SET_LAYER_PHASE_POSITION),
                            "EnvMode" => Some(id::SET_LAYER_ENVELOPE_TRACKING),
                            "LayerMode" => Some(id::SET_LAYER_SYNTHESIS_MODE),
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            out.push(msg(kind, &idx, &[*on as i32 as f32]));
                            return true;
                        }
                        if let Some(par) = param_for_control(name) {
                            out.push(msg(id::SET_LAYER_PARAMETER, &idx, &[par as u8 as f32, if *on { 1.0 } else { 0.0 }, 0.0]));
                            return true;
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    /// The transmod sources (bit per source 1..=28) modulating the layer
    /// parameter of the knob at `path` (`Layer::GetLayerParameter
    /// ModulationBitField`), None off the layer knobs.
    pub fn modulation_mask(&self, app: &App, model: &Model, path: &[usize]) -> Option<u32> {
        if !self.params.as_ref().is_some_and(|pp| path.starts_with(pp)) {
            // [re:fe83c5] $_6: v2a ([re:fadd5a]).
            let e = model.engine;
            return self
                .vsplit
                .reset_value(path, app.ctrl(path).map(|c| c.name.as_str()).unwrap_or(""))
                .map(|_| crate::model::modulation_mask(model.pad_node(e, model.pad[e]), dreist_formats::PadParam::V2A.name()));
        }
        let c = app.ctrl(path)?;
        if c.class != "CtrlTransmodKnob" {
            return None;
        }
        let par = param_for_control(&c.name)?;
        let e = model.engine;
        Some(crate::model::modulation_mask(model.layer_node(e, model.pad[e], model.layer[e]), par.name()))
    }

    /// The layer parameter of the layer panel control at `path`.
    pub fn layer_param_at(&self, path: &[usize], name: &str) -> Option<LayerParam> {
        self.params.as_ref().filter(|pp| path.starts_with(pp)).and_then(|_| param_for_control(name))
    }

    /// The velocity split editor's tooltip.
    pub fn vsplit_tooltip(&self, model: &Model, c: &Ctrl) -> Option<String> {
        self.vsplit.tooltip(model, c)
    }

    /// Whether `path` is the velocity split's v2a knob.
    pub fn is_v2a(&self, path: &[usize], name: &str) -> bool {
        self.vsplit.reset_value(path, name).is_some()
    }

    /// The conversion of a layer knob's typed edit.
    pub fn edit_curve(&self, path: &[usize], name: &str) -> Option<dreist_formats::params::Curve> {
        if self.params.as_ref().is_some_and(|pp| path.starts_with(pp)) {
            return param_for_control(name).map(|p| p.curve());
        }
        self.vsplit.reset_value(path, name).map(|_| dreist_formats::PadParam::V2A.curve())
    }

    /// [re:efb170] $_26 "Remove all modulation from this control"
    /// ([re:bed354]): 0x68 per source; port: the parameter's depth 0 for
    /// sources 1..=28.
    pub fn remove_modulation(&self, app: &App, model: &Model, path: &[usize]) -> Vec<Command> {
        if !self.params.as_ref().is_some_and(|pp| path.starts_with(pp)) || !app.ctrl(path).is_some_and(|c| c.class == "CtrlTransmodKnob") {
            return Vec::new();
        }
        let Some(par) = app.ctrl(path).and_then(|c| param_for_control(&c.name)) else { return Vec::new() };
        let e = model.engine;
        let idx = [e as i32, model.pad[e] as i32, model.layer[e] as i32];
        (1..=28).map(|s| msg(id::SET_LAYER_PARAMETER, &idx, &[par as u8 as f32, 0.0, s as f32])).collect()
    }

    /// A knob double click's reset value ([re:ed3c05]
    /// $_27 [re:f0ac00]: [re:6b28ff], the base
    /// value whatever transmod source is shown; the velocity split's v2a).
    pub fn reset_value(&self, path: &[usize], name: &str) -> Option<f32> {
        if self.params.as_ref().is_some_and(|pp| path.starts_with(pp)) {
            // Hold's −1 ("off") is 0 on the knob.
            return param_for_control(name).map(|p| p.default_value().max(0.0));
        }
        self.vsplit.reset_value(path, name)
    }

    pub fn draw(&self, model: &Model, r: &Renderer, p: &mut Painter, c: &Ctrl) -> bool {
        if self.vsplit.draw(model, r, p, c) {
            return true;
        }
        match c.overload.as_str() {
            "LayerWaveDisplay" => {
                self.draw_wave(model, r, p, c);
                true
            }
            "WaveDisplayZoomSlider" if c.attr("layerZoom").is_some() => {
                crate::zoomslider::draw(p, r.styles, "WaveDisplayZoomSliderStyle", c.rect.w as f32, c.rect.h as f32, self.view);
                true
            }
            "LayerRow" => {
                if c.attr("selected") == Some("true") {
                    let col = r.styles.colour("InspectorTableStyle", "colRowSelection", Colour::WHITE);
                    p.fill_rect(RectF::new(0.0, 0.0, c.rect.w as f32, c.rect.h as f32), col);
                }
                true
            }
            _ => false,
        }
    }

    fn draw_wave(&self, model: &Model, r: &Renderer, p: &mut Painter, c: &Ctrl) {
        let s = r.styles;
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let purple = Colour::rgba(128, 0, 128, 255);
        let top = s.colour(WAVE_STYLE, "colFill", purple);
        let mid = s.colour(WAVE_STYLE, "colAltFill", purple);
        // Vertical gradient top → middle → bottom, drawn as bands.
        let bands = (h as usize).max(1);
        for i in 0..bands {
            let t = i as f32 / (bands - 1).max(1) as f32;
            let k = 1.0 - (t - 0.5).abs() * 2.0;
            let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * k).round() as u8;
            p.fill_rect(RectF::new(0.0, i as f32, w, 1.0), Colour::rgba(mix(top.r, mid.r), mix(top.g, mid.g), mix(top.b, mid.b), 255));
        }
        let e = model.engine;
        let (pad, l) = (model.pad[e], model.layer[e]);
        if let Some(sm) = &model.sampling {
            draw_sampling(sm, r, p, w, h);
            return;
        }
        // `ComputeComponentAreas` [re:d9085e]: timeline 0..15, marker strip
        // 15..30, the waveform below.
        let (wy, full_h) = (crate::timeline::WAVE_TOP, h);
        let h = (full_h - wy).max(1.0);
        let centre = s.colour(WAVE_STYLE, "colCenterLine", purple);
        p.line(0.0, wy + (h / 2.0).floor() + 0.5, w, wy + (h / 2.0).floor() + 0.5, centre.with_alpha(0.5), 1.0);
        let Some(path) = model.layer_sample(e, pad, l) else {
            p.fill_rect(RectF::new(0.0, 0.0, w, wy), s.colour(WAVE_STYLE, "colTimelineBackground", purple));
            return;
        };
        let mut cache = self.peaks.borrow_mut();
        let Some(pk) = cache.get(&path) else { return };
        let line = s.colour(WAVE_STYLE, "colLine", purple);
        let disabled = s.colour(WAVE_STYLE, "colLineDisabled", line);
        let looping_col = s.colour(WAVE_STYLE, "colLineLooping", line);
        let m = wave_markers(model, e, pad, l, pk.frames as u32);
        let n = pk.buckets.len();
        let cols = w as usize;
        let (v0, vl) = self.view;
        let frac_of = |x: f32| v0 + x / w.max(1.0) * vl;
        for x in 0..cols {
            let fa = frac_of(x as f32);
            let fb = frac_of(x as f32 + 1.0);
            let a = ((fa * n as f32) as usize).min(n - 1);
            let b = ((fb * n as f32).ceil() as usize).max(a + 1).min(n);
            let frame = (fa * pk.frames as f32) as u32;
            let line = if frame < m.start || frame >= m.end {
                disabled
            } else if m.looping && frame >= m.loop_start && frame < m.loop_end {
                looping_col
            } else {
                line
            };
            let (lo, hi) = pk.buckets[a..b].iter().fold((f32::MAX, f32::MIN), |(l0, h0), (l1, h1)| (l0.min(*l1), h0.max(*h1)));
            let y0 = wy + h / 2.0 - hi.clamp(-1.0, 1.0) * h / 2.0;
            let y1 = wy + h / 2.0 - lo.clamp(-1.0, 1.0) * h / 2.0;
            p.fill_rect(RectF::new(x as f32, y0, 1.0, (y1 - y0).max(1.0)), line);
        }
        // Timeline (`DrawTimeline` [re:ad103f]) and the marker strip.
        let frames = pk.frames.max(1) as f32;
        {
            let rate = pk.rate.max(1.0) as f64;
            let x_of = |t: f64| ((t * rate) as f32 / frames - v0) / vl * w;
            let (start_s, len_s) = ((v0 * frames) as f64 / rate, (vl * frames) as f64 / rate);
            crate::timeline::draw(p, s, WAVE_STYLE, w, h, start_s, len_s, m.start as f64 / rate, self.timeline_beats, model.tempo(), &x_of);
        }
        // Markers (`DrawStartAndEndMarkers` / `DrawLoopMarkers` →
        // `DrawMarkers` [re:25235f]): a 2 px line from the marker strip down and
        // a 12 px disc (15·0.75 rounded up to even) with a black inset at
        // the top of the strip.
        let marker = |p: &mut Painter, f: u32, col: Colour| {
            let x = ((f as f32 / frames - v0) / vl * w).round();
            if x < -6.0 || x > w + 6.0 {
                return;
            }
            let top = crate::timeline::STRIP;
            p.fill_rect(RectF::new(x - 1.0, top, 2.0, full_h - 1.0 - top), col);
            let d = 12.0;
            p.fill_ellipse(RectF::new(x - d / 2.0, top, d, d), col);
            p.fill_ellipse(RectF::new(x - d / 2.0 + 2.0, top + 2.0, d - 4.0, d - 4.0), Colour::BLACK);
        };
        marker(p, m.start, s.colour(WAVE_STYLE, "colStartMarker", purple));
        marker(p, m.end.min(pk.frames as u32 - 1), s.colour(WAVE_STYLE, "colEndMarker", purple));
        if m.looping {
            marker(p, m.loop_start, s.colour(WAVE_STYLE, "colLoopStartMarker", purple));
            marker(p, m.loop_end.min(pk.frames as u32 - 1), s.colour(WAVE_STYLE, "colLoopEndMarker", purple));
        }
        // [re:c29608]: Amp (top half when both
        // show) and Free envelopes over the waveform.
        let (amp, free) = self.env_visible;
        let mode = model.layer_node(e, pad, l).and_then(|n| n.attr("PlaybackMode")).map_or("OneShot", dreist_dsp::voice::PlaybackMode::canonical).to_string();
        {
            let x_of = |t: f32| ((t * pk.rate) / frames - v0) / vl * w;
            let start_t = m.start as f32 / pk.rate;
            let end_t = (m.end.max(1) - 1) as f32 / pk.rate;
            let half = h / 2.0;
            let areas: Vec<(bool, f32, f32)> = match (amp, free) {
                (true, true) => vec![(true, wy, half), (false, wy + half, half)],
                (true, false) => vec![(true, wy, h)],
                (false, true) => vec![(false, wy, h)],
                _ => Vec::new(),
            };
            for (is_amp, y, ah) in areas {
                let pts = envelope_points(model, e, pad, l, is_amp, &mode, start_t, end_t);
                let mut poly: Vec<(f32, f32)> = pts.iter().map(|&(t, v)| (x_of(t), y + ah * (1.0 - v.clamp(0.0, 1.0)))).collect();
                if poly.len() < 2 {
                    continue;
                }
                let line_pts = poly.clone();
                poly.push((poly.last().unwrap().0, y + ah));
                poly.insert(0, (poly[0].0, y + ah));
                p.fill_polygon(&poly, s.colour(WAVE_STYLE, "colEnvelopeBack", Colour::rgba(255, 192, 203, 255)).with_alpha(0.2));
                p.polyline(&line_pts, s.colour(WAVE_STYLE, "colEnvelopeLine", Colour::rgba(255, 192, 203, 255)), 1.0);
                let geo = EnvGeo { w, y0: wy, h, v0, vl, frames, rate: pk.rate, start: m.start, end: m.end };
                let grabbed = self.env_drag.filter(|d| d.0 == is_amp).map(|d| d.1);
                let handles = if mode == "OneShotLengthBound" { Vec::new() } else { env_handles(model, e, pad, l, is_amp, &mode, &geo, y, ah) };
                for (k, r) in handles {
                    p.fill_rect(r, Colour::WHITE);
                    let inner = RectF::new(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0);
                    let col = if grabbed == Some(k) { s.colour(WAVE_STYLE, "colEnvelopeGrab", Colour::BLACK) } else { Colour::BLACK };
                    p.fill_rect(inner, col);
                }
                // Title box: "AMP ENVELOPE" / "FREE ENVELOPE", centred 10 px down.
                let label = if is_amp { "AMP ENVELOPE" } else { "FREE ENVELOPE" };
                let font = s.font(WAVE_STYLE);
                let (mw, mh) = p.fonts.measure(&font, label);
                let (tw, th) = (mw + 10.0, mh + 10.0);
                let bx = ((w - tw) / 2.0).floor();
                p.fill_rounded(RectF::new(bx, y + 10.0, tw, th), 3.0, [true; 4], Colour::rgba(64, 64, 64, 255).with_alpha(0.5));
                p.text(RectF::new(bx, y + 10.0, tw, th), label, s.colour(WAVE_STYLE, "colText", Colour::WHITE), &font, dreist_gui::style::HAlign::Center, dreist_gui::style::VAlign::Center);
            }
        }
    }
}

/// The wave display's mapping for the envelope handles.
struct EnvGeo {
    w: f32,
    /// Top and height of the waveform area.
    y0: f32,
    h: f32,
    v0: f32,
    vl: f32,
    frames: f32,
    rate: f32,
    start: u32,
    end: u32,
}

impl EnvGeo {
    fn x_of(&self, t: f32) -> f32 {
        ((t * self.rate) / self.frames - self.v0) / self.vl * self.w
    }
    fn t_of(&self, x: f32) -> f32 {
        (self.v0 + x / self.w.max(1.0) * self.vl) * self.frames / self.rate
    }
    fn start_t(&self) -> f32 {
        self.start as f32 / self.rate
    }
    fn end_t(&self) -> f32 {
        (self.end.max(1) - 1) as f32 / self.rate
    }
    /// (y, height) of the Amp / Free area.
    fn area(&self, amp: bool, vis: (bool, bool)) -> Option<(f32, f32)> {
        match (vis, amp) {
            ((true, true), true) => Some((self.y0, self.h / 2.0)),
            ((true, true), false) => Some((self.y0 + self.h / 2.0, self.h / 2.0)),
            ((true, false), true) | ((false, true), false) => Some((self.y0, self.h)),
            _ => None,
        }
    }
}

fn env_param(amp: bool, k: usize) -> dreist_formats::LayerParam {
    dreist_formats::LayerParam::ALL[14 + if amp { 0 } else { 8 } + k]
}

/// `GetEnvelopeXYPositions` [re:b1431d]: x of start, A, D, H / note-off, R end
/// and the sustain level.
fn env_xs(model: &Model, e: usize, p: usize, l: usize, amp: bool, gated: bool, g: &EnvGeo) -> ([f32; 5], f32) {
    let v = |k: usize| model.layer_param(e, p, l, env_param(amp, k));
    let p4 = |x: f32| x * x * x * x;
    let st = g.start_t();
    let ta = st + p4(v(0)) * 10.0;
    let td = ta + p4(v(1)) * 10.0;
    let th = if gated { td.max(g.end_t()) } else { td + p4(v(3)) * 60.0 };
    let tr = th + p4(v(4)) * 15.0;
    ([g.x_of(st), g.x_of(ta), g.x_of(td), g.x_of(th), g.x_of(tr)], v(2))
}

/// `DetermineEnvelopeHandles` [re:be46a1] + `CanEnvelopeCurvesBeSeenAndEdited`
/// [re:0f85ea]: the visible 10 × 10 handles — 0 attack peak, 1 decay end, 2
/// sustain (mid of the sustain line), 3 hold end (one-shot modes), 4
/// release end, 5 / 6 / 7 the attack / decay / release curves.
fn env_handles(model: &Model, e: usize, p: usize, l: usize, amp: bool, mode: &str, g: &EnvGeo, ay: f32, ah: f32) -> Vec<(usize, RectF)> {
    let gated = matches!(mode, "Gated" | "Looping");
    let (x, s) = env_xs(model, e, p, l, amp, gated, g);
    let cv = |k: usize| model.layer_param(e, p, l, env_param(amp, k));
    let sy = ay + ah * (1.0 - s);
    let clampy = |y: f32| y.min(ay + ah - 10.0).max(ay);
    let r = |x: f32, y: f32| RectF::new(x, y, 10.0, 10.0);
    let hs = [
        r(x[1] - 5.0, ay),
        r(x[2] - 5.0, (sy - 5.0).max(ay)),
        r(x[2] + (x[3] - x[2] - 10.0) / 2.0, (sy - 5.0).max(ay)),
        r(x[3] - 5.0, (sy - 5.0).max(ay)),
        r(x[4] - 5.0, ay + ah - 10.0),
        r(x[0] - 5.0 + (x[1] - x[0]) / 2.0, clampy(ay - 5.0 + ah * (1.0 - cv(5)))),
        r(x[1] - 5.0 + (x[2] - x[1]) / 2.0, clampy(ay - 5.0 + (sy - ay) * (1.0 - cv(6)))),
        r(x[3] - 5.0 + (x[4] - x[3]).abs() / 2.0, clampy(sy - 5.0 + (ay + ah - sy) * (1.0 - cv(7)))),
    ];
    let mut vis = [true; 8];
    vis[3] = !gated;
    if hs[0].x < hs[5].x + 5.0 {
        vis[5] = false;
    }
    if hs[7].x <= hs[3].x + 5.0 {
        vis[7] = false;
    }
    if hs[3].x - hs[1].x - 10.0 < 10.0 {
        vis[2] = false;
    }
    if hs[1].x - hs[0].x - 10.0 < 10.0 {
        vis[6] = false;
    }
    (0..8).filter(|&k| vis[k]).map(|k| (k, hs[k])).collect()
}

/// The handle under the mouse: (Amp?, handle), Amp first.
fn env_hit(model: &Model, e: usize, p: usize, l: usize, vis: (bool, bool), g: &EnvGeo, x: f32, y: f32) -> Option<(bool, usize)> {
    let mode = model.layer_node(e, p, l).and_then(|n| n.attr("PlaybackMode")).map_or("OneShot", dreist_dsp::voice::PlaybackMode::canonical).to_string();
    if mode == "OneShotLengthBound" {
        return None;
    }
    for amp in [true, false] {
        let Some((ay, ah)) = g.area(amp, vis) else { continue };
        if let Some((k, _)) = env_handles(model, e, p, l, amp, &mode, g, ay, ah).into_iter().find(|(_, r)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h) {
            return Some((amp, k));
        }
    }
    None
}

/// `HandleEnvelopeDrag` [re:86bd9d]: the layer parameter values for a handle
/// dragged to (x, y) — times from x (`((t − base)/range)^¼`, A / D 10 s, H
/// 60 s, R 15 s), sustain / curves from y; dragging the decay or hold
/// handle also sets the sustain from y unless shift is held
/// (`SendSustainChangeMessage`).
fn env_drag_values(model: &Model, e: usize, p: usize, l: usize, amp: bool, handle: usize, vis: (bool, bool), g: &EnvGeo, x: f32, y: f32, shift: bool) -> Vec<(u8, f32)> {
    let Some((ay, ah)) = g.area(amp, vis) else { return Vec::new() };
    let mode = model.layer_node(e, p, l).and_then(|n| n.attr("PlaybackMode")).map_or("OneShot", dreist_dsp::voice::PlaybackMode::canonical).to_string();
    let gated = matches!(mode.as_str(), "Gated" | "Looping");
    let v = |k: usize| model.layer_param(e, p, l, env_param(amp, k));
    let p4 = |x: f32| x * x * x * x;
    let t = g.t_of(x);
    let st = g.start_t();
    let time = |base: f32, range: f32| ((t.clamp(base, base + range) - base) / range).powf(0.25);
    let sustain = |y: f32| (1.0 - (y - ay) / ah.max(1.0)).clamp(0.0, 1.0);
    let mut out = Vec::new();
    let value = match handle {
        0 => time(st, 10.0),
        1 => {
            if !shift {
                out.push((env_param(amp, 2) as u8, sustain(y)));
            }
            time(st + p4(v(0)) * 10.0, 10.0)
        }
        2 | 5 => sustain(y),
        3 => {
            if !shift {
                out.push((env_param(amp, 2) as u8, sustain(y)));
            }
            time(st + p4(v(0)) * 10.0 + p4(v(1)) * 10.0, 60.0)
        }
        4 => {
            let td = st + p4(v(0)) * 10.0 + p4(v(1)) * 10.0;
            let base = if gated { td.max(g.end_t()) } else { td + p4(v(3)) * 60.0 };
            time(base, 15.0)
        }
        6 => {
            let sy = ah * (1.0 - v(2));
            1.0 - (y - ay) / sy.max(1.0)
        }
        _ => {
            let sy = ay + ah * (1.0 - v(2));
            1.0 - (y - sy) / (ay + ah - sy).max(1.0)
        }
    };
    out.push((env_param(amp, handle) as u8, value.clamp(0.0, 1.0)));
    out
}

/// Envelope curve points (time in seconds, level) for the layer's Amp or
/// Free envelope, run through the engine's `Envelope` (`GetEnvelopeParameters`
/// [re:a4c7b1] / `GetEnvelopeXYPositions` [re:b1431d]: A, D to sustain, then hold
/// in one-shot modes or sustain until the end marker in gated / looping
/// modes, then the release), starting at the start marker.
fn envelope_points(model: &Model, e: usize, p: usize, l: usize, amp: bool, mode: &str, start_t: f32, end_t: f32) -> Vec<(f32, f32)> {
    use dreist_formats::LayerParam as LP;
    let lp = |a: LP, f: LP| model.layer_param(e, p, l, if amp { a } else { f });
    let params = dreist_dsp::envelope::EnvelopeParams {
        attack: lp(LP::AmpEnvAttack, LP::FreeEnvAttack),
        decay: lp(LP::AmpEnvDecay, LP::FreeEnvDecay),
        sustain: lp(LP::AmpEnvSustain, LP::FreeEnvSustain),
        hold: lp(LP::AmpEnvHold, LP::FreeEnvHold),
        release: lp(LP::AmpEnvRelease, LP::FreeEnvRelease),
        attack_curve: lp(LP::AmpEnvAttackCurve, LP::FreeEnvAttackCurve),
        decay_curve: lp(LP::AmpEnvDecayCurve, LP::FreeEnvDecayCurve),
        release_curve: lp(LP::AmpEnvReleaseCurve, LP::FreeEnvReleaseCurve),
    };
    // `DrawAnEnvelopeLengthBound`: the AHR envelope laid over the played
    // part (start..end marker).
    if mode == "OneShotLengthBound" {
        let n = 600u32;
        let mut env = dreist_dsp::envelope::AhrEnvelope::default();
        env.initialize(&params, n, 1, 1.0);
        env.note_on();
        let mut buf = vec![0.0f32; n as usize + 1];
        env.process(&mut buf);
        let span = (end_t - start_t).max(1e-6);
        return buf.iter().enumerate().map(|(i, v)| (start_t + span * i as f32 / n as f32, *v)).collect();
    }
    let p4 = |x: f32| x * x * x * x;
    let (a, d, hold, r) = (p4(params.attack) * 10.0, p4(params.decay) * 10.0, p4(params.hold) * 60.0, p4(params.release) * 15.0);
    let gated = matches!(mode, "Gated" | "Looping");
    let sustain_end = if gated { (start_t + a + d).max(end_t) - start_t } else { a + d + hold };
    let total = (sustain_end + r).max(1e-3);
    // Enough steps for a smooth curve.
    let steps = 600usize;
    let sr = steps as f32 / total;
    let mut env = dreist_dsp::envelope::Envelope::new(sr);
    env.initialize(&params, !gated, 1.0);
    env.note_on();
    let mut out = Vec::with_capacity(steps + 2);
    let mut buf = [0.0f32; 1];
    for i in 0..=steps {
        let t = i as f32 / sr;
        if gated && t >= sustain_end && i > 0 && (t - 1.0 / sr) < sustain_end {
            env.note_off();
        }
        env.process(&mut buf);
        out.push((start_t + t, buf[0]));
    }
    out
}

/// Sampling mode: the whole take fills the display (the take's start and
/// end markers sit at its edges, `UpdateWaveDisplay` [re:05551e]).
fn draw_sampling(sm: &crate::model::Sampling, r: &Renderer, p: &mut Painter, w: f32, h: f32) {
    let s = r.styles;
    let purple = Colour::rgba(128, 0, 128, 255);
    let line = s.colour(WAVE_STYLE, "colLine", purple);
    let n = sm.buckets.len();
    if n == 0 {
        return;
    }
    let cols = w.max(1.0) as usize;
    for x in 0..cols {
        let a = (x * n / cols).min(n - 1);
        let b = ((x + 1) * n / cols).max(a + 1).min(n);
        let (lo, hi) = sm.buckets[a..b].iter().fold((f32::MAX, f32::MIN), |(l0, h0), (l1, h1)| (l0.min(*l1), h0.max(*h1)));
        let y0 = h / 2.0 - hi.clamp(-1.0, 1.0) * h / 2.0;
        let y1 = h / 2.0 - lo.clamp(-1.0, 1.0) * h / 2.0;
        p.fill_rect(RectF::new(x as f32, y0, 1.0, (y1 - y0).max(1.0)), line);
    }
    for (x, col) in [(0.0, "colStartMarker"), (w - 1.0, "colEndMarker")] {
        p.fill_rect(RectF::new((x - 1.0f32).max(0.0), 0.0, 2.0, h), s.colour(WAVE_STYLE, col, purple));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn envelope_handles_and_drag() {
        use dreist_formats::Node;
        let mut doc = Node::new("root", None);
        let mut eng = Node::new("Engine", Some("Engine0"));
        let mut pad = Node::new("Pad", Some("Pad0"));
        let mut l = Node::new("Layer", Some("Layer0"));
        l.set("AmpEnvAttack", 0.5).set("AmpEnvDecay", 0.5).set("AmpEnvSustain", 0.5).set("AmpEnvHold", 0.3).set("AmpEnvRelease", 0.5);
        pad.children.push(l);
        eng.children.push(pad);
        doc.children.push(eng);
        let model = Model::new(doc);
        let g = EnvGeo { w: 1000.0, y0: 0.0, h: 200.0, v0: 0.0, vl: 1.0, frames: 44100.0 * 20.0, rate: 44100.0, start: 0, end: 44100 * 20 };
        let hs = env_handles(&model, 0, 0, 0, true, "OneShot", &g, 0.0, 100.0);
        let (_, a) = hs.iter().find(|(k, _)| *k == 0).copied().unwrap();
        // Attack 0.5 → 0.625 s → x = 31.25 px.
        assert!((a.x + 5.0 - 31.25).abs() < 0.5, "{a:?}");
        assert_eq!(env_hit(&model, 0, 0, 0, (true, true), &g, a.x + 5.0, a.y + 5.0), Some((true, 0)));
        // Dragging it to 2.5 s gives (2.5/10)^¼.
        let vals = env_drag_values(&model, 0, 0, 0, true, 0, (true, true), &g, 125.0, 0.0, false);
        let (pid, v) = *vals.last().unwrap();
        assert_eq!(pid, dreist_formats::LayerParam::AmpEnvAttack as u8);
        assert!((v - 0.25f32.powf(0.25)).abs() < 1e-3, "{v}");
        // The decay handle also sets the sustain from y (not with shift).
        assert_eq!(env_drag_values(&model, 0, 0, 0, true, 1, (true, true), &g, 125.0, 25.0, false).len(), 2);
        assert_eq!(env_drag_values(&model, 0, 0, 0, true, 1, (true, true), &g, 125.0, 25.0, true).len(), 1);
    }

    use super::*;

    #[test]
    fn zero_crossing_snap() {
        let z = [10, 50, 200];
        assert_eq!(snap_zero(&z, 45, 20), 50);
        assert_eq!(snap_zero(&z, 120, 20), 120);
        assert_eq!(snap_zero(&z, 14, 20), 10);
    }
}
