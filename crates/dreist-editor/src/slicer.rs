//! Slicer ([re:c241a0], `Slicer/SlicePanel.skin` 870×740): a modal panel
//! over the editor for cutting an audio file into slices and extracting
//! them to pads and a pattern.
//!
//! - Opened by [re:bff97e] (here: the
//!   browser's Slice mode on an audio file); the wave editor resets its zoom
//!   and the analysis (a `.gsa` sidecar, else [re:c5ff4b])
//!   fills the slices.
//! - Slicing [re:77d1da]: Transient mode keeps the slices usable at the
//!   sensitivity, Chop mode cuts
//!   equal divisions; locked slices are carried over.
//! - Controls per slicing mode [re:cc20ff]: Transient shows the
//!   Sensitivity slider, Chop the ChopDivisionBox.
//! - Meta data changes [re:168d6a]: BPM 20..1000, Bars 0..1000, Beats
//!   0..100; with Auto on, beats ≥ 4 carry into bars and tempo ↔ bars/beats
//!   follow the region length.
//! - Allocation [re:90734a] / job [re:f09ec6]: the pattern is extracted from the analysis,
//!   then the allocation manager assigns slices to pads (and the pattern).
//! - Revert [re:db19a5] (Cancel) restores the engine saved when the
//!   panel opened.
//! - [re:b13346] / slice markers [re:be5846]:
//!   a line per slice with a 12×10 head at the top (filled box when locked,
//!   else a triangle, yellow when selected, Slate otherwise).

use std::path::{Path, PathBuf};

use dreist_dsp::slice::{self, SlicePoint, SlicedPattern};
use dreist_engine::gsa::SliceFile;
use dreist_formats::Node;
use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::render::Renderer;
use dreist_gui::style::{HAlign, VAlign};
use dreist_gui::{Colour, Ctrl, Rect, Skin};

use crate::command::Command;
use crate::model::Model;
use crate::{EditorError, err};

pub const MODES: [&str; 2] = ["Transient", "Chop"];
const BUCKET: usize = 64;
const SLATE: Colour = Colour::rgba(112, 128, 144, 255);
const YELLOW: Colour = Colour::rgba(255, 255, 0, 255);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Extract {
    Both,
    Audio,
    Pattern,
}

struct Audio {
    /// The file, shared with the preview voice.
    data: std::sync::Arc<dreist_dsp::voice::SampleData>,
    /// (min, max) per `BUCKET` frames.
    buckets: Vec<(f32, f32)>,
}

/// Slide animation tags.
const SLIDE_IN: u32 = 0x51c1;
const SLIDE_OUT: u32 = 0x51c0;

pub struct Slicer {
    /// Path of the overlay (a root child).
    overlay: Option<Vec<usize>>,
    /// The editor's `MainPanel` (the slicer covers it).
    main: Option<Vec<usize>>,
    /// On screen (sliding in, shown or sliding out) / sliding out.
    shown: bool,
    closing: bool,
    pub open: bool,
    file: Option<PathBuf>,
    audio: Option<Audio>,
    /// All analysed slices (the `.gsa` / detector output).
    analysis: SliceFile,
    /// Slices in use (after mode / sensitivity, plus user edits).
    pub slices: Vec<SlicePoint>,
    pub mode: usize,
    pub sensitivity: f32,
    pub divisions: u32,
    auto_tempo: bool,
    multi_layer: bool,
    tool: usize,
    selected: Option<usize>,
    drag: Option<usize>,
    /// Visible frames.
    view: (u32, u32),
    /// Zoom slider drag ([re:105b41]: 1 centre, 2 left
    /// edge, 3 right edge) and the last x.
    zoom_drag: Option<(u8, f32)>,
    /// The document when the panel opened (Cancel restores it).
    snapshot: Option<Node>,
    extracted: bool,
    pub pattern: SlicedPattern,
    /// PreviewWave / LoopPreview button states.
    previewing: bool,
    loop_preview: bool,
    /// A REX file: its own slices and tempo (Sensitivity and BPM disabled,
    /// [re:4cb63d] / meta data from the file
    /// [re:17c579]).
    rex: bool,
}

/// Preference "Quantize REX" (`AllocatorQuantizeREX`): extracted steps get
/// no timing offset (pattern extraction [re:150941] sets lane 3 to
/// 0.5 instead of the slice's distance from the step) — for every slicing,
/// not only REX files.
fn quantize_rex() -> bool {
    crate::options::Options::load().bool("AllocatorQuantizeREX", false)
}

impl Slicer {
    pub fn build(skin: &mut Skin, root: &mut Ctrl) -> Result<Slicer, EditorError> {
        let (w, h) = (root.rect.w, root.rect.h);
        let mut overlay = Ctrl {
            class: "CtrlPanel".into(),
            name: "SlicerOverlay".into(),
            overload: "SlicerOverlay".into(),
            style: "InvisibleStyle".into(),
            rect: Rect::new(0, 0, w, h),
            zorder: 1000,
            visible: false,
            enabled: true,
            ..Ctrl::default()
        };
        let rel = skin.find("SlicePanel.skin").unwrap_or_else(|| "Slicer/SlicePanel.skin".into());
        let panel = skin.load_skin(&rel).map_err(err)?.children.into_iter().next();
        let mut out = Slicer {
            overlay: None,
            main: root.path_of("MainPanel"),
            shown: false,
            closing: false,
            open: false,
            file: None,
            audio: None,
            analysis: SliceFile::new(0),
            slices: Vec::new(),
            mode: 0,
            sensitivity: 0.5,
            divisions: 16,
            auto_tempo: true,
            multi_layer: false,
            tool: 0,
            selected: None,
            drag: None,
            view: (0, 0),
            zoom_drag: None,
            snapshot: None,
            extracted: false,
            pattern: SlicedPattern { steps: 16, hits: Vec::new() },
            previewing: false,
            loop_preview: false,
            rex: false,
        };
        let Some(mut p) = panel else { return Ok(out) };
        p.name = "SlicePanel".into();
        // [re:695294] 0x13a: the panel is added to MainPanel at
        // its full size; the overlay stands in for MainPanel here.
        if let Some(m) = out.main.as_ref() {
            let mut r = Rect::new(0, 0, 0, 0);
            let mut c = &*root;
            for &i in m {
                c = &c.children[i];
                r.x += c.rect.x;
                r.y += c.rect.y;
            }
            r.w = c.rect.w;
            r.h = c.rect.h;
            overlay.rect = r;
            p.set_rect(Rect::new(0, -r.h, r.w, r.h));
        } else {
            let (pw, ph) = (p.rect.w.min(w), p.rect.h.min(h));
            p.set_rect(Rect::new((w - pw) / 2, (h - ph) / 2, pw, ph));
        }
        if let Some(s) = p.find_mut("ChopDivisionBox") {
            s.overload = "SliceChopStepper".into();
        }
        overlay.children.push(p);
        root.children.push(overlay);
        out.overlay = Some(vec![root.children.len() - 1]);
        Ok(out)
    }

    /// Slicing started: load `file` and its analysis, remember the
    /// document for Cancel.
    pub fn start(&mut self, model: &Model, file: &Path) -> bool {
        // Loop load [re:660edf]: a REX ".truncated" path does not
        // slice; a REX file loads its slices end to end with the file's
        // own slice points.
        let (_, truncated) = dreist_engine::rex::strip_truncated(file);
        let rex = dreist_engine::rex::is_rex(file);
        if rex && truncated {
            return false;
        }
        let rex_c = if rex { dreist_engine::rex::load(file) } else { None };
        if rex && rex_c.is_none() {
            return false;
        }
        let Ok(data) = dreist_engine::audio::load(file) else { return false };
        let n = data.left.len();
        if n == 0 {
            return false;
        }
        let buckets = (0..n.div_ceil(BUCKET))
            .map(|b| {
                let s = &data.left[b * BUCKET..((b + 1) * BUCKET).min(n)];
                let r = data.right.as_ref().map(|r| &r[b * BUCKET..((b + 1) * BUCKET).min(n)]);
                let mut lo = s.iter().fold(f32::MAX, |a, &x| a.min(x));
                let mut hi = s.iter().fold(f32::MIN, |a, &x| a.max(x));
                if let Some(r) = r {
                    lo = r.iter().fold(lo, |a, &x| a.min(x));
                    hi = r.iter().fold(hi, |a, &x| a.max(x));
                }
                (lo, hi)
            })
            .collect();
        let gsa = file.with_extension("gsa");
        let mut analysis = match &rex_c {
            Some(c) => {
                let mut a = SliceFile::new(n as u32);
                a.slices = c.slices.iter().map(|&p| SlicePoint::manual(p as u32, false)).collect();
                a.bpm = c.tempo;
                a.bars = c.bars;
                a
            }
            None => SliceFile::load(&gsa).unwrap_or_else(|| SliceFile::analyze(&data.left, data.right.as_deref())),
        };
        self.rex = rex;
        if analysis.end == 0 || analysis.end as usize > n {
            analysis.end = n as u32;
        }
        self.audio = Some(Audio { data: std::sync::Arc::new(data), buckets });
        self.analysis = analysis;
        self.file = Some(file.to_path_buf());
        self.view = (0, n as u32);
        self.selected = None;
        self.snapshot = Some(model.doc.clone());
        self.extracted = false;
        self.open = true;
        if self.auto_tempo {
            self.tempo_from_bars();
        }
        self.reslice();
        true
    }

    fn region(&self) -> (u32, u32) {
        (self.analysis.start, self.analysis.end)
    }

    /// Zoom slider thumb (x, width) in a track `tw` wide: start and size as
    /// fractions of the file ([re:214f77]).
    fn zoom_thumb(&self, tw: f32) -> (f32, f32) {
        let n = self.frames().max(1) as f32;
        let (a, b) = self.view;
        (a as f32 / n * tw, (b.saturating_sub(a)) as f32 / n * tw)
    }

    /// The smallest view the zoom buttons allow (64 frames) as a fraction.
    fn min_zoom_proportion(&self) -> f32 {
        (64.0 / self.frames().max(1) as f32).min(1.0)
    }

    /// The slider's notification: the view follows start = x / w and
    /// size = max(width / w, minimum).
    fn set_zoom_from_thumb(&mut self, x: f32, w: f32, tw: f32) {
        self.view = zoom_view(x, w, tw, self.frames(), self.min_zoom_proportion());
    }

    fn frames(&self) -> u32 {
        self.audio.as_ref().map(|a| a.data.left.len() as u32).unwrap_or(0)
    }

    /// Slice the file again.
    pub fn reslice(&mut self) {
        let locked: Vec<SlicePoint> = self.slices.iter().filter(|s| s.locked).copied().collect();
        let (start, end) = self.region();
        let mut v = if self.mode == 0 { slice::for_sensitivity(&self.analysis.slices, self.sensitivity) } else { slice::chop(start, end - start, self.divisions as f32) };
        for l in locked {
            if let Some(s) = v.iter_mut().find(|s| s.start == l.start) {
                s.locked = true;
            } else {
                v.push(l);
            }
        }
        v.sort_by_key(|s| s.start);
        self.slices = v;
        self.update_pattern();
    }

    fn update_pattern(&mut self) {
        let (start, end) = self.region();
        self.pattern = slice::extract_pattern(&self.slices, start, end, self.analysis.bars, self.analysis.beats, self.multi_layer, quantize_rex());
    }

    /// Tempo from bars and beats.
    fn tempo_from_bars(&mut self) {
        let (start, end) = self.region();
        let Some(sr) = self.audio.as_ref().map(|a| a.data.sample_rate) else { return };
        let secs = (end - start) as f32 / sr;
        let beats = self.analysis.bars * 4.0 + self.analysis.beats;
        if secs > 0.0 && beats > 0.0 {
            self.analysis.bpm = (beats * 60.0 / secs).clamp(20.0, 1000.0);
        }
    }

    /// Bars and beats from the tempo.
    fn bars_from_tempo(&mut self) {
        let (start, end) = self.region();
        let Some(sr) = self.audio.as_ref().map(|a| a.data.sample_rate) else { return };
        let beats = ((end - start) as f32 / sr * self.analysis.bpm / 60.0).round();
        self.analysis.bars = (beats / 4.0).floor();
        self.analysis.beats = beats - self.analysis.bars * 4.0;
    }

    pub fn sync(&mut self, app: &mut App) {
        let Some(op) = self.overlay.clone() else { return };
        let editing = app.is_editing();
        // [re:695294] 0x13a / 0x145: slides down over MainPanel
        // (350 ms exponential), back up on close (350 ms logarithmic,
        // then [re:5e9662]).
        if let Some(m) = self.main.clone() {
            let panel = [op.clone(), vec![0]].concat();
            // Follow MainPanel (window resizes).
            let (mx, my) = app.origin(&m);
            if let Some((mw, mh)) = app.ctrl(&m).map(|c| (c.rect.w, c.rect.h)) {
                let want = Rect::new(mx, my, mw, mh);
                if app.ctrl(&op).is_some_and(|o| o.rect != want) {
                    if let Some(o) = app.ctrl_mut(&op) {
                        o.rect = want;
                    }
                    let y = if self.shown && !self.closing { 0 } else { -mh };
                    if let Some(p) = app.ctrl_mut(&panel) {
                        p.set_rect(Rect::new(0, y, mw, mh));
                    }
                }
            }
            if let Some(r) = app.ctrl(&op).map(|c| c.rect) {
                let up = Rect::new(0, -r.h, r.w, r.h);
                let down = Rect::new(0, 0, r.w, r.h);
                if self.open && !self.shown {
                    app.animate_rect(&panel, up, down, 350.0, dreist_gui::collapse::exponential, SLIDE_IN);
                    self.shown = true;
                    self.closing = false;
                } else if !self.open && self.shown && !self.closing {
                    let from = app.ctrl(&panel).map(|c| c.rect).unwrap_or(down);
                    app.animate_rect(&panel, from, up, 350.0, dreist_gui::collapse::curve, SLIDE_OUT);
                    self.closing = true;
                }
            }
        } else {
            self.shown = self.open;
        }
        let open = self.shown;
        let (mode, tool, auto, multi) = (self.mode, self.tool, self.auto_tempo, self.multi_layer);
        let (sens, bpm, bars, beats, rex) = (self.sensitivity, self.analysis.bpm, self.analysis.bars, self.analysis.beats, self.rex);
        let Some(o) = app.ctrl_mut(&op) else { return };
        o.visible = open;
        if !open {
            return;
        }
        o.walk_mut(&mut |c: &mut Ctrl| match c.name.as_str() {
            "ToolsButton" => {
                for (i, b) in c.children.iter_mut().enumerate() {
                    b.on = i == tool;
                }
            }
            "AutoDetect" => c.on = auto,
            "SliceMode" => c.text = MODES[mode].into(),
            "Sensitivity" => {
                c.visible = mode == 0;
                c.enabled = !rex;
                c.value = sens;
            }
            "BPMBox" if rex => {
                c.enabled = false;
                c.text = format!("{bpm:.2}");
            }
            "ChopDivisionBox" => c.visible = mode != 0,
            "MultiLayersButton" => c.on = multi,
            "BPMBox" if !editing => {
                c.enabled = true;
                c.text = format!("{bpm:.2}");
            }
            "BarsBox" if !editing => c.text = format!("{bars}"),
            "BeatsBox" if !editing => c.text = format!("{beats}"),
            _ => {}
        });
    }

    /// Cursor for the pointer over the slicer's zoom slider.
    pub fn hover_cursor(&self, app: &App, path: &[usize], overload: &str, x: f32, y: f32) -> Option<dreist_gui::cursor::Cursor> {
        if !self.mine(path) || overload != "WaveDisplayZoomSlider" {
            return None;
        }
        let (w, h) = app.ctrl(path).map(|c| (c.rect.w as f32, c.rect.h as f32))?;
        let (tx, tw) = self.zoom_thumb(w);
        let view = (tx / w.max(1.0), tw / w.max(1.0));
        Some(if crate::zoomslider::over_edge(view, w, h, x, y) { dreist_gui::cursor::Cursor::LeftRight } else { dreist_gui::cursor::Cursor::Arrow })
    }

    fn mine(&self, path: &[usize]) -> bool {
        self.open && self.overlay.as_ref().is_some_and(|o| path.starts_with(o))
    }

    fn rebuild(model: &Model, out: &mut Vec<Command>) {
        out.push(model.rebuild_command());
    }

    /// Preview frames `start..end` ([re:e80b43] with the
    /// region markers).
    fn preview(&self, start: u32, end: u32, looping: bool, out: &mut Vec<Command>) {
        if let Some(a) = &self.audio {
            out.push(Command::PreviewRegion { data: a.data.clone(), start, end, looping });
        }
    }

    /// Slice preview at the click [re:e50ea1]: the last slice starting
    /// at or before `frame`, up to the next slice, clamped to the region;
    /// the whole region when there are no slices.
    pub fn slice_span(&self, frame: u32) -> Option<(u32, u32)> {
        let (start, end) = self.region();
        let live: Vec<u32> = self.slices.iter().filter(|s| !s.deleted).map(|s| s.start).collect();
        if live.is_empty() {
            return Some((start, end));
        }
        let i = live.iter().rposition(|&s| s <= frame)?;
        let next = live.get(i + 1).copied().unwrap_or(end);
        Some((live[i].max(start), next.min(end)))
    }

    fn close(&mut self) {
        self.open = false;
        self.previewing = false;
        self.snapshot = None;
        self.audio = None;
    }

    /// Frame under a wave-editor x.
    fn frame_at(&self, x: f32, w: f32) -> u32 {
        let (a, b) = self.view;
        (a as f64 + (x.max(0.0) / w.max(1.0)) as f64 * (b - a) as f64) as u32
    }

    fn x_of(&self, frame: u32, w: f32) -> f32 {
        let (a, b) = self.view;
        (frame as f64 - a as f64) as f32 / (b - a).max(1) as f32 * w
    }

    /// Slice whose marker is within 4 px of x.
    fn marker_at(&self, x: f32, w: f32) -> Option<usize> {
        self.slices.iter().enumerate().filter(|(_, s)| !s.deleted).map(|(i, s)| (i, (self.x_of(s.start, w) - x).abs())).filter(|(_, d)| *d <= 4.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
    }

    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        if let (UiEvent::Animated { path, tag: SLIDE_OUT }, Some(op)) = (ev, self.overlay.as_ref()) {
            if self.closing && path.starts_with(op) {
                self.closing = false;
                self.shown = false;
                if let Some(o) = app.ctrl_mut(op) {
                    o.visible = false;
                }
                return true;
            }
        }
        match ev {
            UiEvent::Clicked { path, name, on, .. } if self.mine(path) => {
                let parent = app.ctrl(&path[..path.len() - 1]).map(|c| c.name.clone()).unwrap_or_default();
                if parent == "ToolsButton" {
                    self.tool = *path.last().unwrap();
                    return true;
                }
                match name.as_str() {
                    // Skin lambdas [re:6886ec] / [re:de2e31].
                    "PreviewWave" => {
                        self.previewing = *on;
                        if *on {
                            let (a, b) = self.region();
                            self.preview(a, b, self.loop_preview, out);
                        } else {
                            out.push(crate::command::msg(dreist_engine::binding::id::STOP_PREVIEW, &[], &[]));
                        }
                    }
                    "LoopPreview" => {
                        self.loop_preview = *on;
                        if self.previewing {
                            out.push(crate::command::msg(dreist_engine::binding::id::SET_PREVIEW_LOOP_MODE, &[], &[if *on { 1.0 } else { 0.0 }]));
                        }
                    }
                    "AutoDetect" => self.auto_tempo = *on,
                    "SliceMode" => app.open_menu(path, MODES.iter().map(|s| s.to_string()).collect(), Some(self.mode)),
                    "MultiLayersButton" => {
                        self.multi_layer = !self.multi_layer;
                        self.update_pattern();
                    }
                    "BothButton" => self.extract(model, Extract::Both, out),
                    "AudioButton" => self.extract(model, Extract::Audio, out),
                    "PatternButton" => self.extract(model, Extract::Pattern, out),
                    "ZoomInButton" | "ZoomOutButton" => {
                        let (a, b) = self.view;
                        let mid = (a as u64 + b as u64) / 2;
                        let half = ((b - a) as u64 / 2).max(1);
                        let half = if name == "ZoomInButton" { (half / 2).max(32) } else { half * 2 };
                        let n = self.frames() as u64;
                        let a2 = mid.saturating_sub(half);
                        let b2 = (mid + half).min(n);
                        self.view = (a2 as u32, b2.max(a2 + 1) as u32);
                    }
                    "CancelButton" => {
                        out.push(crate::command::msg(dreist_engine::binding::id::STOP_PREVIEW, &[], &[]));
                        if self.extracted {
                            if let Some(d) = self.snapshot.take() {
                                model.doc = d;
                                Self::rebuild(model, out);
                            }
                        }
                        self.close();
                    }
                    "DoneButton" => {
                        out.push(crate::command::msg(dreist_engine::binding::id::STOP_PREVIEW, &[], &[]));
                        if let Some(f) = &self.file {
                            // The analysis is saved next to the audio file.
                            let mut a = self.analysis.clone();
                            a.slices = self.slices.clone();
                            let _ = a.save(&f.with_extension("gsa"));
                        }
                        self.close();
                    }
                    _ => {}
                }
                true
            }
            UiEvent::MenuSelected { anchor, name, index } if self.mine(anchor) && name == "SliceMode" => {
                self.mode = (*index).min(1);
                self.reslice();
                true
            }
            UiEvent::ValueChanged { path, name, value, .. } if self.mine(path) && name == "Sensitivity" => {
                self.sensitivity = value.clamp(0.0, 1.0);
                self.reslice();
                true
            }
            UiEvent::TextCommitted { path, name, text } if self.mine(path) => {
                let v: f32 = text.trim().parse().unwrap_or(f32::NAN);
                if v.is_nan() {
                    return true;
                }
                match name.as_str() {
                    "BPMBox" => {
                        self.analysis.bpm = v.clamp(20.0, 1000.0);
                        if self.auto_tempo {
                            self.bars_from_tempo();
                        }
                    }
                    "BarsBox" | "BeatsBox" => {
                        if name == "BarsBox" {
                            self.analysis.bars = v.clamp(0.0, 1000.0).floor();
                        } else {
                            self.analysis.beats = v.clamp(0.0, 100.0);
                        }
                        if self.auto_tempo {
                            while self.analysis.beats >= 4.0 {
                                self.analysis.bars += 1.0;
                                self.analysis.beats -= 4.0;
                            }
                            self.tempo_from_bars();
                        }
                    }
                    _ => return false,
                }
                self.update_pattern();
                true
            }
            UiEvent::Mouse { path, overload, kind, x, .. } if self.mine(path) && overload == "SliceChopStepper" => {
                if *kind == MouseKind::Down {
                    let w = app.ctrl(path).map(|c| c.rect.w as f32).unwrap_or(1.0);
                    self.divisions = if *x < w / 2.0 { self.divisions.saturating_sub(1).max(1) } else { (self.divisions + 1).min(256) };
                    self.reslice();
                }
                true
            }
            UiEvent::Wheel { path, overload, dy } if self.mine(path) && overload == "SliceChopStepper" => {
                self.divisions = (self.divisions as i32 + dy.signum() as i32).clamp(1, 256) as u32;
                self.reslice();
                true
            }
            UiEvent::Mouse { path, overload, kind, x, .. } if self.mine(path) && overload == "SliceWaveEditor" => {
                let w = app.ctrl(path).map(|c| c.rect.w as f32).unwrap_or(1.0);
                match kind {
                    MouseKind::Down => {
                        let hit = self.marker_at(*x, w);
                        match (self.tool, hit) {
                            // Insert: a manual slice at the click.
                            (0, None) => {
                                let f = self.frame_at(*x, w);
                                self.slices.push(SlicePoint::manual(f, false));
                                self.slices.sort_by_key(|s| s.start);
                                self.selected = self.slices.iter().position(|s| s.start == f);
                            }
                            (1, Some(i)) => {
                                self.slices.remove(i);
                                self.selected = None;
                            }
                            (2, Some(i)) => self.slices[i].locked = !self.slices[i].locked,
                            (_, Some(i)) => {
                                self.selected = Some(i);
                                self.drag = Some(i);
                            }
                            _ => {}
                        }
                        self.update_pattern();
                        true
                    }
                    MouseKind::Drag => {
                        let Some(i) = self.drag else { return false };
                        let f = self.frame_at(*x, w).min(self.frames().saturating_sub(1));
                        if let Some(s) = self.slices.get_mut(i) {
                            s.start = f;
                            s.pos = f as i32;
                        }
                        true
                    }
                    MouseKind::Up => {
                        if self.drag.take().is_some() {
                            self.slices.sort_by_key(|s| s.start);
                            self.update_pattern();
                        }
                        true
                    }
                    // Right press in a zone [re:d538e0]: audition the slice.
                    MouseKind::RightDown => {
                        if let Some((a, b)) = self.slice_span(self.frame_at(*x, w)) {
                            self.preview(a, b, false, out);
                        }
                        true
                    }
                }
            }
            // The modal panel swallows everything else aimed at it.
            UiEvent::Mouse { path, overload, kind, x, y } if self.mine(path) && overload == "WaveDisplayZoomSlider" => {
                let Some((tw, h)) = app.ctrl(path).map(|c| (c.rect.w as f32, c.rect.h as f32)) else { return false };
                let (mut tx, mut thw) = self.zoom_thumb(tw);
                match kind {
                    // Left press [re:89f320]: within 2 px of an edge
                    // resizes, inside the thumb moves it.
                    MouseKind::Down => {
                        let inside_y = *y >= 0.0 && *y < h;
                        let mode = if !inside_y {
                            None
                        } else if (*x - tx).abs() < 2.0 {
                            Some(2)
                        } else if (*x - (tx + thw)).abs() < 2.0 {
                            Some(3)
                        } else if *x >= tx && *x < tx + thw {
                            Some(1)
                        } else {
                            None
                        };
                        self.zoom_drag = mode.map(|m| (m, *x));
                        mode.is_some()
                    }
                    MouseKind::Drag => {
                        let Some((mode, last)) = self.zoom_drag else { return false };
                        let min_w = self.min_zoom_proportion() * tw;
                        match mode {
                            // Centre zone drag [re:109246].
                            1 => tx = (tx + *x - last).clamp(0.0, (tw - thw).max(0.0)),
                            // Left zone drag [re:3ef21b].
                            2 => {
                                let right = tx + thw;
                                let nx = x.min(right - 20.0).min(right - min_w).max(0.0);
                                thw = (right - nx).max(0.0);
                                tx = nx;
                            }
                            // Right zone drag [re:4f55d9].
                            _ => {
                                let right = x.max(tx + 20.0f32.max(min_w)).min(tw);
                                thw = (right - tx).max(0.0);
                            }
                        }
                        self.zoom_drag = Some((mode, *x));
                        self.set_zoom_from_thumb(tx, thw, tw);
                        true
                    }
                    MouseKind::Up => self.zoom_drag.take().is_some(),
                    MouseKind::RightDown => false,
                }
            }
            // Double click [re:009295]: the whole file.
            UiEvent::DoubleClicked { path, .. } if self.mine(path) && app.ctrl(path).is_some_and(|c| c.overload == "WaveDisplayZoomSlider") => {
                self.zoom_drag = None;
                self.view = (0, self.frames().max(1));
                true
            }
            // Mouse wheel [re:207f1c]: the thumb moves 5 px.
            UiEvent::Wheel { path, overload, dy } if self.mine(path) && overload == "WaveDisplayZoomSlider" => {
                let Some(tw) = app.ctrl(path).map(|c| c.rect.w as f32) else { return false };
                let (tx, thw) = self.zoom_thumb(tw);
                let nx = (tx + if *dy > 0.0 { 5.0 } else { -5.0 }).clamp(0.0, (tw - thw).max(0.0));
                self.set_zoom_from_thumb(nx, thw, tw);
                true
            }
            UiEvent::Mouse { path, .. } | UiEvent::Wheel { path, .. } => self.mine(path),
            _ => false,
        }
    }

    /// Extract slices to the selected engine's pads and/or current pattern
    /// (from the document as it was when the panel opened):
    /// [re:dd2b54] with the engine's locks,
    /// then [re:cb6b75]:
    /// - fewer than two slices: the whole file into the current pad's first
    ///   layer;
    /// - else every (pad, layer) of the engine: a slice → the layer gets the
    ///   file with the slice's region; none → the layer's sample is removed
    ///   (layer clear [re:7d1e9f], 0xe1, unless pad or layer is locked);
    /// - each loaded layer: `e_resetLayerEnvelopes` (0x86,
    ///   [re:4220aa]) then AmpEnvAttack 0.1;
    /// - pattern (unless the current pattern is locked): a new pattern
    ///   (0xa7) with the hits, the old `MonoModeActive` and the old locked
    ///   tracks.
    pub fn extract(&mut self, model: &mut Model, what: Extract, out: &mut Vec<Command>) {
        let (Some(file), Some(snap)) = (self.file.clone(), self.snapshot.clone()) else { return };
        model.doc = snap;
        let e = model.engine;
        let frames = self.frames();
        let sr = self.audio.as_ref().map(|a| a.data.sample_rate).unwrap_or(44100.0);
        let (start, end) = self.region();
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let pat_index = model.current_pattern(e);
        let blocked = |p: usize, l: usize| {
            model.pad_node(e, p).is_some_and(|n| n.locked())
                || model.layer_node(e, p, l).is_some_and(|n| n.locked())
                || model.pattern_node(e, pat_index).and_then(|sp| sp.children.iter().find(|t| t.class == "Track" && t.name.as_deref() == Some(&format!("Track{p}")))).is_some_and(|n| n.locked())
        };
        self.pattern = slice::extract_pattern_with_locks(&self.slices, start, end, self.analysis.bars, self.analysis.beats, self.multi_layer, quantize_rex(), &blocked);
        let usable: Vec<SlicePoint> = self.slices.iter().filter(|s| !s.deleted).copied().collect();
        // Load `file` with region (s0, s1) into (p, l), reset its envelopes.
        let load = |model: &mut Model, p: usize, l: usize, s0: u32, s1: u32, name: &str| {
            if !crate::browser::load_sample_into_layer(model, e, p, l, &file) {
                return;
            }
            let layer = model.layer_mut(e, p, l);
            layer.children.retain(|c| c.class != "AudioFileMarkers");
            for nm in ["StartEndMarkers", "LoopMarkers"] {
                let mut m = Node::new("AudioFileMarkers", Some(nm));
                m.set("StartSample", s0).set("EndSample", s1).set("SampleLength", frames);
                layer.children.push(m);
            }
            reset_envelopes(layer, (s1 - s0) as f64, frames as f64, sr);
            layer.set(dreist_formats::params::LayerParam::AmpEnvAttack.name(), 0.1);
            if !name.is_empty() {
                model.set_layer_name(e, p, l, name);
            }
        };
        if what != Extract::Pattern {
            if usable.len() < 2 {
                let p = model.pad[e];
                load(model, p, 0, 0, frames, "");
            } else {
                let mut layout = [[None::<(usize, usize)>; crate::model::LAYERS]; 64];
                for h in &self.pattern.hits {
                    if let Some(k) = usable.iter().position(|s| s.start == self.slices[h.slice].start) {
                        layout[h.pad][h.layer] = Some((k, h.slice));
                    }
                }
                for (p, row) in layout.iter().enumerate() {
                    for (l, slot) in row.iter().enumerate() {
                        match slot {
                            Some((k, si)) => {
                                let (s0, s1) = slice::slice_markers(&usable, *k, end);
                                load(model, p, l, s0, s1, &format!("{stem} {}", si + 1));
                            }
                            None => clear_layer(model, e, p, l),
                        }
                    }
                }
            }
        }
        let pattern_locked = model.pattern_node(e, pat_index).is_some_and(|n| n.locked());
        if what != Extract::Audio && !pattern_locked {
            let eng = model.engine_mut(e);
            let sp = eng.child_mut_or_insert("StepPattern", &format!("StepPattern{pat_index}"));
            let mono = sp.attr("MonoModeActive").map(str::to_string);
            let locked: Vec<Node> = sp.children.iter().filter(|t| t.class == "Track" && t.locked()).cloned().collect();
            let name = sp.name.clone();
            let version = sp.version;
            *sp = Node::new("StepPattern", name.as_deref());
            sp.version = version;
            sp.set("PatternSteps", self.pattern.steps);
            if let Some(m) = mono {
                sp.set("MonoModeActive", m);
            }
            for h in &self.pattern.hits {
                let tr = sp.child_mut_or_insert("Track", &format!("Track{}", h.pad));
                let st = tr.child_mut_or_insert("Step", &format!("S{}", h.step));
                st.set("State", "true").set("Velocity", 1);
                if (h.shift - 0.5).abs() > 1e-6 {
                    st.set("Shift", h.shift);
                }
                if self.multi_layer {
                    st.set("ForceLayer", h.force_layer);
                }
            }
            for t in locked {
                sp.children.retain(|c| !(c.class == "Track" && c.name == t.name));
                sp.children.push(t);
            }
        }
        self.extracted = true;
        Self::rebuild(model, out);
    }

    pub fn draw(&self, r: &Renderer, p: &mut Painter, c: &Ctrl) -> bool {
        let s = r.styles;
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        match c.overload.as_str() {
            // The original adds the panel to MainPanel directly: no shade.
            "SlicerOverlay" => {
                self.main.is_some() || {
                    p.fill_rect(RectF::new(0.0, 0.0, w, h), Colour::rgba(0, 0, 0, 120));
                    true
                }
            }
            "SliceWaveEditor" => {
                let back = s.colour("WaveDisplayStyle", "colFill", Colour::rgba(210, 216, 222, 255));
                let wave = s.colour("WaveDisplayStyle", "colAltFill", Colour::rgba(80, 88, 96, 255));
                p.fill_rect(RectF::new(0.0, 0.0, w, h), back);
                let Some(a) = &self.audio else { return true };
                let (v0, v1) = self.view;
                let mid = h / 2.0;
                let per = (v1 - v0) as f32 / w.max(1.0);
                for px in 0..w as usize {
                    let f0 = (v0 as f32 + px as f32 * per) as usize;
                    let f1 = ((v0 as f32 + (px + 1) as f32 * per) as usize).max(f0 + 1).min(a.data.left.len());
                    let (lo, hi) = if per >= BUCKET as f32 {
                        a.buckets[f0 / BUCKET..(f1.div_ceil(BUCKET)).min(a.buckets.len())].iter().fold((f32::MAX, f32::MIN), |(l, hh), &(x, y)| (l.min(x), hh.max(y)))
                    } else {
                        a.data.left[f0.min(a.data.left.len() - 1)..f1.max(f0.min(a.data.left.len() - 1) + 1)].iter().fold((f32::MAX, f32::MIN), |(l, hh), &x| (l.min(x), hh.max(x)))
                    };
                    if lo <= hi {
                        let (y0, y1) = (mid - hi * mid * 0.95, mid - lo * mid * 0.95);
                        p.fill_rect(RectF::new(px as f32, y0, 1.0, (y1 - y0).max(1.0)), wave);
                    }
                }
                // Selected slice region.
                if let Some(i) = self.selected.filter(|&i| i < self.slices.len()) {
                    let (s0, s1) = slice::slice_markers(&self.slices, i, self.analysis.end);
                    let (x0, x1) = (self.x_of(s0, w), self.x_of(s1, w));
                    p.fill_rect(RectF::new(x0, 0.0, (x1 - x0).max(0.0), h), YELLOW.with_alpha(0.15));
                }
                let line = s.colour("WaveDisplayStyle", "colBorder", Colour::rgba(40, 40, 40, 255));
                for (i, sl) in self.slices.iter().enumerate() {
                    if sl.deleted || sl.start < v0 || sl.start >= v1 {
                        continue;
                    }
                    let x = self.x_of(sl.start, w).floor() + 0.5;
                    p.line(x, 0.0, x, h, line, 1.0);
                    let head = if self.selected == Some(i) { YELLOW } else { SLATE };
                    let (hx, hy) = (x - 6.0, 0.0);
                    if sl.locked {
                        p.fill_rect(RectF::new(hx + 1.5, hy + 5.0, 9.0, 6.0), head);
                        p.stroke_rect(RectF::new(hx + 3.5, hy + 1.0, 5.0, 5.0), head, 1.0);
                    } else {
                        p.fill_polygon(&[(hx, hy), (hx + 12.0, hy), (hx + 6.0, hy + 10.0)], head);
                        p.polyline(&[(hx, hy), (hx + 12.0, hy), (hx + 6.0, hy + 10.0), (hx, hy)], Colour::rgba(100, 100, 100, 255), 1.0);
                    }
                }
                true
            }
            "WaveDisplayZoomSlider" => {
                // [re:867658]: the thumb in
                // colFill, 2 px white edge lines, and on each edge a white
                // 10 px disc with a black 6 px centre.
                let (tx, tw) = self.zoom_thumb(w);
                p.fill_rect(RectF::new(tx, 0.0, tw, h), s.colour("WaveDisplayZoomSliderStyle", "colFill", Colour::rgba(255, 192, 203, 255)));
                for ex in [tx, tx + tw] {
                    p.fill_rect(RectF::new(ex - 1.0, 0.0, 2.0, h), Colour::WHITE);
                }
                let cy = (h as i32 / 2) as f32;
                for ex in [tx, tx + tw] {
                    p.fill_ellipse(RectF::new(ex - 5.0, cy - 5.0, 10.0, 10.0), Colour::WHITE);
                    p.fill_ellipse(RectF::new(ex - 3.0, cy - 3.0, 6.0, 6.0), Colour::BLACK);
                }
                true
            }
            "SliceChopStepper" => {
                let st = r.style(c);
                p.fill_rect(RectF::new(0.0, 0.0, w, h), s.colour(st, "colFill", Colour::WHITE));
                p.stroke_rect(RectF::new(0.5, 0.5, w - 1.0, h - 1.0), s.colour(st, "colBorder", Colour::BLACK), 1.0);
                let font = s.font(st);
                let text = s.colour(st, "colText", Colour::BLACK);
                p.text(RectF::new(0.0, 0.0, w, h), &format!("◂ {} ▸", self.divisions), text, &font, HAlign::Center, VAlign::Center);
                true
            }
            "SlicePanelPatternDisplay" => {
                p.fill_rect(RectF::new(0.0, 0.0, w, h), Colour::rgba(40, 44, 48, 255));
                let steps = self.pattern.steps.max(1) as f32;
                let rows = self.pattern.hits.iter().map(|h| h.pad + 1).max().unwrap_or(1).max(4) as f32;
                let (cw, rh) = (w / steps, (h / rows).min(16.0));
                for k in 0..self.pattern.steps {
                    if k % 4 == 0 {
                        p.fill_rect(RectF::new(k as f32 * cw, 0.0, 1.0, h), Colour::rgba(70, 76, 82, 255));
                    }
                }
                let on = s.colour("PatternStepOnStyle", "colFill", Colour::rgba(0, 228, 161, 255));
                for hit in &self.pattern.hits {
                    let x = hit.step as f32 * cw + (hit.shift - 0.5) * cw;
                    p.fill_rect(RectF::new(x + 1.0, hit.pad as f32 * rh + 1.0, (cw - 2.0).max(1.0), rh - 2.0), on);
                }
                true
            }
            _ => false,
        }
    }
}

/// Frames shown for a zoom slider thumb at `x`, `w` wide, in a track `tw`
/// wide over `n` frames (size at least `min`).
fn zoom_view(x: f32, w: f32, tw: f32, n: u32, min: f32) -> (u32, u32) {
    let nf = n.max(1) as f64;
    let size = ((w / tw.max(1.0)).max(min)).min(1.0) as f64;
    let start = ((x / tw.max(1.0)) as f64).clamp(0.0, 1.0 - size);
    let a = (start * nf).round() as u32;
    let b = ((start + size) * nf).round().min(nf) as u32;
    (a, b.max(a + 1))
}

/// [re:4220aa]: amp and free envelopes to attack 0,
/// decay 0.02, sustain 1, release 0.001, curves 0.5, then the hold reset
/// (no pitch compensation) for both holds.
fn reset_envelopes(layer: &mut Node, region: f64, total: f64, sr: f32) {
    use dreist_formats::params::LayerParam as P;
    for (a, d, su, r, ac, dc, rc) in [
        (P::AmpEnvAttack, P::AmpEnvDecay, P::AmpEnvSustain, P::AmpEnvRelease, P::AmpEnvAttackCurve, P::AmpEnvDecayCurve, P::AmpEnvReleaseCurve),
        (P::FreeEnvAttack, P::FreeEnvDecay, P::FreeEnvSustain, P::FreeEnvRelease, P::FreeEnvAttackCurve, P::FreeEnvDecayCurve, P::FreeEnvReleaseCurve),
    ] {
        layer.set(a.name(), 0.0).set(d.name(), 0.02).set(su.name(), 1.0).set(r.name(), 0.001);
        layer.set(ac.name(), 0.5).set(dc.name(), 0.5).set(rc.name(), 0.5);
    }
    let n = layer.clone();
    let get = |q: P| n.f32(q.name()).unwrap_or(q.default_value()).clamp(0.0, 1.0);
    let bound = n.attr("PlaybackMode").map(dreist_dsp::voice::PlaybackMode::canonical) == Some("OneShotLengthBound");
    let h = dreist_engine::hold::reset_hold_time(&get, bound, region, total, sr, false);
    layer.set(P::AmpEnvHold.name(), h).set(P::FreeEnvHold.name(), h);
}

/// [re:b30133] → 0xe1 `e_deallocateSample`:
/// the layer loses its sample unless the pad or the layer is locked.
fn clear_layer(model: &mut Model, e: usize, p: usize, l: usize) {
    if model.layer_node(e, p, l).is_none() || model.pad_node(e, p).is_some_and(|n| n.locked()) || model.layer_node(e, p, l).is_some_and(|n| n.locked()) {
        return;
    }
    let layer = model.layer_mut(e, p, l);
    layer.attrs.retain(|(k, _)| k != "AudioFileName");
    layer.children.retain(|c| c.class != "AudioFileMarkers");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_slider_view() {
        assert_eq!(zoom_view(0.0, 100.0, 100.0, 1000, 0.064), (0, 1000));
        assert_eq!(zoom_view(25.0, 50.0, 100.0, 1000, 0.064), (250, 750));
        // Narrower than the minimum, past the end: clamped.
        assert_eq!(zoom_view(99.0, 1.0, 100.0, 1000, 0.064), (936, 1000));
    }

    #[test]
    fn reslice_modes_keep_locked() {
        let mut s = Slicer {
            overlay: None,
            main: None,
            shown: false,
            closing: false,
            open: true,
            file: None,
            audio: None,
            analysis: SliceFile::new(44100),
            slices: Vec::new(),
            mode: 1,
            sensitivity: 0.5,
            divisions: 4,
            auto_tempo: false,
            multi_layer: false,
            tool: 0,
            selected: None,
            drag: None,
            view: (0, 44100),
            zoom_drag: None,
            snapshot: None,
            extracted: false,
            pattern: SlicedPattern { steps: 16, hits: Vec::new() },
            previewing: false,
            loop_preview: false,
            rex: false,
        };
        assert_eq!(s.slice_span(500), Some((0, 44100)), "no slices: whole region");
        s.reslice();
        assert_eq!(s.slices.len(), 4);
        s.slices.push(SlicePoint::manual(1000, true));
        s.divisions = 2;
        s.reslice();
        assert_eq!(s.slices.len(), 3, "{:?}", s.slices);
        assert!(s.slices.iter().any(|p| p.start == 1000 && p.locked));
        assert_eq!(s.pattern.hits.len(), 3);
        // Chop 2 over 44100 frames plus the locked slice at 1000.
        assert_eq!(s.slice_span(1500), Some((1000, 22050)));
        assert_eq!(s.slice_span(30000), Some((22050, 44100)));
    }
}
