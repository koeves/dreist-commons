//! Song tab (`SongPanel.skin`, [re:f184d3]): one row per
//! engine (`SongTrackHeaderPanel.skin` headers), a timeline and the parts.
//!
//! - Rows start 20 px down ([re:5029dc]: `20 + id·h`).
//! - Grid ([re:f7dd45]): bar lines in
//!   `colGridBar`, beat lines at α0.5, labels "bar.beat" (fine zoom) or the
//!   bar number, in `SongTimelineStyle`.
//! - Parts ([re:b452f0] /
//!   [re:6c3c15]): rounded sections per pattern
//!   repeat in `colPart` ([re:86878c] 0x20), border `colBorder`, the
//!   pattern name in `colText` at the top left.
//! - The blackout overlay (`SongPanelBlackoutPanelStyle`) covers the tracks
//!   while song mode is off.
//!
//! Edits post `e_songAddPart` 0x119 [track] [start, length, pattern],
//! `e_songDeletePart` 0x11a [track] [time] and `e_songSetActive` 0x117
//! (payloads are objects in the original; the port uses QN times).

use std::collections::BTreeSet;

use dreist_engine::binding::id;
use dreist_engine::song::{Song, SongPart, phase_mod, quantize_round};
use dreist_engine::songrec::{STOP_PART, SongEvent};
use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::render::Renderer;
use dreist_gui::style::{HAlign, VAlign};
use dreist_gui::{Colour, Ctrl, Rect, Skin};

use crate::command::{Command, msg};
use crate::model::{ENGINES, Model};
use crate::{EditorError, err};

const TIMELINE_H: f32 = 20.0;
/// Editor tab page id of the song panel.
const SONG_TAB: usize = 2;
const PART_STYLE: &str = "SongPartStyle";

/// [re:207106] (left-button operation per tool [re:588ddb]).
#[derive(Clone, Copy, Debug, PartialEq)]
enum OpKind {
    Move,
    /// Rubber band; `true` keeps the selection (shift).
    Band(bool),
    Start,
    Resize,
    Phase,
}

struct Op {
    kind: OpKind,
    /// Grab position (QN, px).
    qn0: f64,
    x0: f32,
    y0: f32,
    /// Song and selection when the operation started.
    snapshot: Song,
    saved: BTreeSet<(usize, i64)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MenuAction {
    Cut,
    Copy,
    Paste,
    Delete,
    Duplicate,
    QuantizeStart,
    QuantizeBoth,
    QuantizeModeMenu,
    QuantizeMode(usize),
    SelectAll,
    SelectEmpty,
    ClearAll,
    EditPattern,
    RenamePattern,
    None,
}

/// `QuantizeMode` items ("SongLastQuantizeMode").
pub const QUANTIZE_MODES: [&str; 3] = ["Auto", "Maintain Start Phase", "Maintain Sync"];

pub struct SongPanel {
    page: Vec<usize>,
    /// `SongBody` (under `page`).
    named: dreist_gui::app::PathCache,
    /// Pixels per quarter note.
    zoom: f32,
    insert_pattern: usize,
    /// EraseButton held (0x123 `e_songSetEraseState`).
    erase: bool,
    /// Last clicked part (the part buttons' part).
    anchor: Option<(usize, i64)>,
    /// The pattern popup is open (for the selection).
    pattern_menu: bool,
    /// First visible quarter note.
    scroll: f32,
    /// Selected parts (track, start in 1/960 QN).
    pub(crate) selected: BTreeSet<(usize, i64)>,
    op: Option<Op>,
    /// The song while an operation is in progress.
    working: Option<Song>,
    /// Band rectangle (px, body coordinates).
    band: Option<RectF>,
    /// Copied parts: (track, part with start relative to the earliest).
    clipboard: Vec<(usize, SongPart)>,
    quantize_mode: usize,
    /// Context menu actions and the right-click time.
    menu: Option<(Vec<MenuAction>, f64)>,
    /// Insert marker (QN): paste position.
    marker: f64,
    /// Timeline drag: operation 13 loop start, 14 loop end, 15 move loop,
    /// 16 marker; with the grab QN and the loop at the start.
    timeline: Option<(u8, f64, f64, f64)>,
    body_w: std::cell::Cell<f32>,
    /// Song recording ([re:463c61]: one part per track being
    /// recorded, with whether the song played when it started) and the
    /// retro buffer.
    pub rec_state: u8,
    recording: [Option<(SongPart, bool)>; ENGINES],
    retro: [Vec<(SongPart, bool)>; ENGINES],
}

#[cfg(test)]
pub(crate) fn retro_len_for_test(model: &Model, t: usize, pattern: usize) -> f64 {
    retro_len(model, t, pattern)
}

/// Pattern length for retro placement: 1 QN for stopped parts or unknown
/// lengths ([re:2cd575]).
fn retro_len(model: &Model, t: usize, pattern: usize) -> f64 {
    if pattern == STOP_PART {
        return 1.0;
    }
    let l = cycle_qn(model, t, pattern);
    if l < 0.001 { 1.0 } else { l }
}

fn key(start: f64) -> i64 {
    (start * 960.0).round() as i64
}

/// Selection sets from the song (by start key).
fn selected_parts(song: &Song, sel: &BTreeSet<(usize, i64)>) -> Vec<(usize, SongPart)> {
    song.tracks.iter().enumerate().flat_map(|(t, ps)| ps.iter().filter(move |p| sel.contains(&(t, key(p.start)))).map(move |p| (t, p.clone()))).collect()
}

/// Pattern length in QN divided by the engine tempo multiplier
/// (pattern length in QN / tempo multiplier).
fn cycle_qn(model: &Model, e: usize, p: usize) -> f64 {
    let mult = model.engine_node(e).map(|n| {
        let num = n.f32("TempoNumerator").unwrap_or(1.0).max(1.0) as f64;
        let den = n.f32("TempoDenominator").unwrap_or(1.0).max(1.0) as f64;
        num / den
    });
    pattern_qn(model, e, p) / mult.unwrap_or(1.0).max(1e-6)
}

fn song_of(model: &Model) -> Song {
    model.doc.children.iter().find(|c| c.class == "SongSequencer").map(Song::from_node).unwrap_or_else(|| Song::from_node(&dreist_formats::Node::new("SongSequencer", None)))
}

fn row_h(c: &Ctrl) -> f32 {
    ((c.rect.h as f32 - TIMELINE_H) / ENGINES as f32).floor().max(12.0)
}

/// Pattern length in quarter notes (steps at its resolution).
fn pattern_qn(model: &Model, e: usize, p: usize) -> f64 {
    let pat = model.pattern(e, p);
    pat.steps as f64 * 0.25 / dreist_engine::pattern::steps_per_16th(pat.resolution).max(1e-6)
}

impl SongPanel {
    pub fn build(skin: &mut Skin, root: &mut Ctrl, page: &[usize]) -> Result<SongPanel, EditorError> {
        let mut host = &mut *root;
        for &i in page {
            host = &mut host.children[i];
        }
        skin.mount(host, "SongPanel.skin").map_err(err)?;
        if let Some(sv) = host.find_mut("TrackScrollView") {
            let (w, h) = (sv.rect.w, sv.rect.h);
            sv.children.push(Ctrl {
                class: "CtrlPanel".into(),
                name: "SongBody".into(),
                overload: "SongBody".into(),
                style: "InvisibleStyle".into(),
                rect: Rect::new(0, 0, w, h),
                anchors: dreist_gui::anchors_fill(),
                visible: true,
                enabled: true,
                zorder: 1,
                ..Ctrl::default()
            });
        }
        let rel = skin.find("SongTrackHeaderPanel.skin").unwrap_or_else(|| "Song/SongTrackHeaderPanel.skin".into());
        let tpl = skin.load_skin(&rel).ok().and_then(|r| r.children.into_iter().next());
        if let (Some(hp), Some(t)) = (host.find_mut("TrackHeaderPanel"), tpl) {
            let rh = ((hp.rect.h as f32 - TIMELINE_H) / ENGINES as f32).floor() as i32;
            let w = hp.rect.w;
            for e in 0..ENGINES {
                let mut h = t.clone();
                h.set_rect(Rect::new(0, TIMELINE_H as i32 + e as i32 * rh, w, rh));
                if let Some(l) = h.find_mut("TrackNumberLabel") {
                    l.text = format!("{}", e + 1);
                }
                h.set_attr("songTrack", e.to_string());
                hp.children.push(h);
            }
        }
        Ok(SongPanel {
            page: page.to_vec(),
            named: Default::default(),
            zoom: 16.0,
            insert_pattern: 0,
            erase: false,
            anchor: None,
            pattern_menu: false,
            scroll: 0.0,
            selected: BTreeSet::new(),
            op: None,
            working: None,
            band: None,
            clipboard: Vec::new(),
            quantize_mode: 0,
            menu: None,
            marker: 0.0,
            timeline: None,
            body_w: std::cell::Cell::new(800.0),
            rec_state: 0,
            recording: Default::default(),
            retro: Default::default(),
        })
    }

    /// Snap [re:e72501]: with fewer than 4 bars visible a 16th, under 16
    /// bars a beat, else whole bars (a power of two of them per 32 bars).
    pub(crate) fn snap_qn(&self) -> f64 {
        let bars = self.body_w.get() as f64 / (4.0 * self.zoom as f64);
        if bars < 4.0 {
            0.25
        } else if bars < 16.0 {
            1.0
        } else {
            4.0 * (2f64.powf((bars * 0.03125).log2().floor()) as i64).max(1) as f64
        }
    }

    fn snap(&self, d: f64) -> f64 {
        let q = self.snap_qn();
        (d / q).round() * q
    }

    /// Apply the current operation's delta to its snapshot.
    fn apply_op(&mut self, model: &Model, qn: f64, x: f32, y: f32, rh: f32) {
        let Some(op) = &self.op else { return };
        let delta = self.snap(qn - op.qn0);
        let mut song = op.snapshot.clone();
        let sel = op.saved.clone();
        let snapq = self.snap_qn();
        let picked = selected_parts(&op.snapshot, &sel);
        match op.kind {
            OpKind::Band(keep) => {
                let r = RectF::new(op.x0.min(x), op.y0.min(y), (x - op.x0).abs(), (y - op.y0).abs());
                self.band = Some(r);
                let (q0, q1) = (r.x as f64 / self.zoom as f64 + self.scroll as f64, (r.x + r.w) as f64 / self.zoom as f64 + self.scroll as f64);
                let mut set = if keep { sel } else { BTreeSet::new() };
                for (t, ps) in op.snapshot.tracks.iter().enumerate() {
                    let (ty0, ty1) = (TIMELINE_H + t as f32 * rh, TIMELINE_H + (t + 1) as f32 * rh);
                    if ty1 < r.y || ty0 > r.y + r.h {
                        continue;
                    }
                    for p in ps.iter().filter(|p| p.end() > q0 && p.start < q1.max(q0 + 1e-6)) {
                        set.insert((t, key(p.start)));
                    }
                }
                self.selected = set;
                return;
            }
            // Move: no earlier than 0.
            OpKind::Move => {
                let min_start = picked.iter().map(|(_, p)| p.start).fold(f64::MAX, f64::min);
                let d = delta.max(-min_start);
                for (t, p) in &picked {
                    let tr = &mut song.tracks[*t];
                    tr.retain(|q| key(q.start) != key(p.start));
                }
                let mut new_sel = BTreeSet::new();
                for (t, p) in &picked {
                    let np = SongPart { start: quantize_round(p.start + d), ..p.clone() };
                    new_sel.insert((*t, key(np.start)));
                    song.add_part(*t, np);
                }
                self.selected = new_sel;
            }
            // Start change [re:ccc0f2]: end fixed, phase follows.
            OpKind::Start => {
                let min_len = picked.iter().map(|(_, p)| p.length).fold(f64::MAX, f64::min);
                let min_start = picked.iter().map(|(_, p)| p.start).fold(f64::MAX, f64::min);
                let d = delta.min((min_len - snapq).max(snapq)).max(-min_start);
                let mut new_sel = BTreeSet::new();
                for (t, p) in &picked {
                    let cycle = cycle_qn(model, *t, p.pattern).max(1e-6);
                    let np = SongPart { start: quantize_round(p.start + d), length: p.length - d, phase: phase_mod(p.phase + d, cycle), ..p.clone() };
                    song.tracks[*t].retain(|q| key(q.start) != key(p.start));
                    new_sel.insert((*t, key(np.start)));
                    song.add_part(*t, np);
                }
                self.selected = new_sel;
            }
            // Resize: length, at least one snap step.
            OpKind::Resize => {
                for (t, p) in &picked {
                    let len = (p.length + delta).max(snapq);
                    song.tracks[*t].retain(|q| key(q.start) != key(p.start));
                    song.add_part(*t, SongPart { length: len, ..p.clone() });
                }
            }
            // Phase change [re:64218f]: phase wraps in the cycle.
            OpKind::Phase => {
                for (t, p) in &picked {
                    let cycle = cycle_qn(model, *t, p.pattern).max(1e-6);
                    let ph = (p.phase - delta).rem_euclid(cycle);
                    for q in song.tracks[*t].iter_mut().filter(|q| key(q.start) == key(p.start)) {
                        q.phase = ph;
                    }
                }
            }
        }
        self.working = Some(song);
    }

    /// Emit the changed tracks of `song` against `before`.
    fn commit(before: &Song, song: &Song, out: &mut Vec<Command>) {
        let changed: Vec<(usize, Vec<SongPart>)> = song
            .tracks
            .iter()
            .enumerate()
            .filter(|(t, ps)| before.tracks.get(*t) != Some(*ps))
            .map(|(t, ps)| {
                // Sorted here: the engine swaps them in on the audio thread.
                let mut ps = ps.clone();
                ps.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
                (t, ps)
            })
            .collect();
        if !changed.is_empty() {
            out.push(Command::SongTracks(Box::new(changed)));
        }
    }

    /// Pattern IDs by delta [re:7474af] (delta clamped so every selected
    /// part stays in 0..=23) / set [re:5a5315] on the selection,
    /// committed as "Change pattern IDs".
    fn change_patterns(&mut self, model: &Model, delta: Option<i32>, set: Option<usize>, out: &mut Vec<Command>) {
        let before = song_of(model);
        let sel = selected_parts(&before, &self.selected);
        if sel.is_empty() {
            return;
        }
        let lo = sel.iter().map(|(_, p)| p.pattern as i32).min().unwrap_or(0);
        let hi = sel.iter().map(|(_, p)| p.pattern as i32).max().unwrap_or(0);
        let d = delta.map(|d| d.max(-lo).min(23 - hi)).unwrap_or(0);
        if delta.is_some() && d == 0 {
            return;
        }
        let mut song = before.clone();
        for (t, ps) in song.tracks.iter_mut().enumerate() {
            for p in ps.iter_mut().filter(|p| self.selected.contains(&(t, key(p.start)))) {
                p.pattern = match set {
                    Some(n) => n,
                    None => (p.pattern as i32 + d) as usize,
                };
            }
        }
        Self::commit(&before, &song, out);
    }

    /// `ShowPatternPopup(false, ...)` [re:ae8c40]: the 24 patterns of the
    /// track, the selection's common pattern checked (the original shows a
    /// grid of pattern keys, state 2 = common, 1 = not empty).
    fn open_pattern_menu(&mut self, app: &mut App, model: &Model, anchor: &[usize]) {
        let song = song_of(model);
        let sel = selected_parts(&song, &self.selected);
        let Some(&(t, _)) = sel.first() else { return };
        let common = sel.first().map(|(_, p)| p.pattern).filter(|c| sel.iter().all(|(_, p)| p.pattern == *c));
        let items = (0..24)
            .map(|n| {
                let name = model.pattern_name(t, n);
                let used = if model.pattern_used(t, n) { "● " } else { "" };
                if name.is_empty() { format!("{used}Pattern {}", n + 1) } else { format!("{used}{}: {name}", n + 1) }
            })
            .collect();
        app.open_menu(anchor, items, common);
        self.pattern_menu = true;
    }

    /// Part buttons [re:90f6ff]: the three part buttons inside the anchor
    /// part's top-right corner, hidden when the part is narrower than the
    /// buttons plus 8 px.
    fn place_part_buttons(&self, app: &mut App, model: &Model) {
        let body = self.named.path_in(app, &self.page, "SongBody");
        let geom = body.as_ref().and_then(|b| {
            let c = app.ctrl(b)?;
            let (bx, by) = app.origin(b);
            let (px, py) = app.origin(&self.page);
            Some((bx - px, by - py, row_h(c)))
        });
        let song = song_of(model);
        let part = self.anchor.and_then(|(t, k)| song.tracks.get(t).and_then(|ps| ps.iter().find(|p| key(p.start) == k)).map(|p| (t, p.clone())));
        let Some(page) = app.ctrl_mut(&self.page) else { return };
        let widths: Vec<i32> = ["PartPrevPatternButton", "PartNextPatternButton", "PartSelectPatternButton"].iter().map(|n| page.find(n).map(|c| c.rect.w).unwrap_or(10)).collect();
        let show = match (part, geom) {
            (Some((t, p)), Some((bx, by, rh))) => {
                let x0 = bx + ((p.start as f32 - self.scroll) * self.zoom) as i32;
                let w = ((p.end() - p.start) as f32 * self.zoom) as i32;
                let y0 = by + (TIMELINE_H + t as f32 * rh) as i32;
                (w >= widths.iter().sum::<i32>() + 8).then_some((x0, w, y0))
            }
            _ => None,
        };
        let names = ["PartPrevPatternButton", "PartNextPatternButton", "PartSelectPatternButton"];
        for (i, n) in names.iter().enumerate() {
            let Some(c) = page.find_mut(n) else { continue };
            match show {
                Some((x0, w, y0)) => {
                    let right = x0 + w;
                    let x = match i {
                        2 => right - 2 - widths[2],
                        1 => right - 4 - widths[2] - widths[1],
                        _ => right - 6 - widths[2] - widths[1] - widths[0],
                    };
                    c.rect.x = x;
                    c.rect.y = y0 + 2;
                    c.visible = true;
                    c.zorder = 10;
                }
                None => c.visible = false,
            }
        }
    }

    fn context_menu(&mut self, app: &mut App, anchor: &[usize], qn: f64) {
        let any = !self.selected.is_empty();
        let items: Vec<(String, MenuAction, bool)> = vec![
            ("Cut Parts".into(), MenuAction::Cut, any),
            ("Copy Parts".into(), MenuAction::Copy, any),
            ("Paste Parts".into(), MenuAction::Paste, !self.clipboard.is_empty()),
            ("Delete Parts".into(), MenuAction::Delete, any),
            ("Duplicate Parts".into(), MenuAction::Duplicate, true),
            (String::new(), MenuAction::None, true),
            ("Quantize Parts (start)".into(), MenuAction::QuantizeStart, any),
            ("Quantize Parts (start/end)".into(), MenuAction::QuantizeBoth, any),
            ("Quantize Mode ▸".into(), MenuAction::QuantizeModeMenu, true),
            (String::new(), MenuAction::None, true),
            ("Select All Parts".into(), MenuAction::SelectAll, true),
            ("Select Empty Parts".into(), MenuAction::SelectEmpty, true),
            ("Clear All Parts".into(), MenuAction::ClearAll, true),
            (String::new(), MenuAction::None, true),
            ("Edit Pattern".into(), MenuAction::EditPattern, self.selected.len() == 1),
            ("Rename Pattern".into(), MenuAction::RenamePattern, self.selected.len() == 1),
        ];
        let labels = items.iter().map(|(s, _, on)| if *on || s.is_empty() { s.clone() } else { format!("({s})") }).collect();
        self.menu = Some((items.into_iter().map(|(_, a, on)| if on { a } else { MenuAction::None }).collect(), qn));
        app.open_menu(anchor, labels, None);
    }

    /// Quantize [re:156a89]: start (and end) to the snap grid;
    /// the phase follows the start in "Maintain Sync", stays in "Maintain
    /// Start Phase", and in "Auto" follows unless the pattern restarts.
    fn quantize(&self, model: &Model, song: &mut Song, end: bool) {
        let q = self.snap_qn();
        for (t, p) in selected_parts(song, &self.selected) {
            let s0 = quantize_round((p.start / q).round() * q);
            let mut e0 = p.end();
            if end {
                e0 = quantize_round((e0 / q).round() * q);
            }
            if s0 >= e0 {
                continue;
            }
            let restart = model.pattern(t, p.pattern).sync.restarts();
            let cycle = cycle_qn(model, t, p.pattern).max(1e-6);
            let phase = match self.quantize_mode {
                1 => p.phase,
                0 if restart => p.phase,
                _ => phase_mod(p.phase + s0 - p.start, cycle),
            };
            song.tracks[t].retain(|x| key(x.start) != key(p.start));
            song.add_part(t, SongPart { start: s0, length: e0 - s0, phase, ..p });
        }
    }

    pub fn sync(&mut self, app: &mut App, model: &Model) {
        self.tick(model);
        self.place_part_buttons(app, model);
        let song = song_of(model);
        let ip = self.insert_pattern;
        let erase = self.erase;
        let name = {
            let n = model.pattern_name(model.engine, ip);
            if n.is_empty() { format!("Pattern {}", ip + 1) } else { n }
        };
        let Some(page) = app.ctrl_mut(&self.page) else { return };
        if let Some(b) = page.find_mut("BlackoutPanel") {
            b.visible = !song.active;
            b.passive = true;
        }
        let _ = name;
        // Pattern buttons [re:ea233f]: enabled with a selection.
        let any = !self.selected.is_empty();
        for n in ["SelectPatternButton", "NextPatternButton", "PrevPatternButton"] {
            if let Some(b) = page.find_mut(n) {
                b.enabled = any;
            }
        }
        if let Some(b) = page.find_mut("EraseButton") {
            b.on = erase;
        }
        // Scroll bar over the song length plus eight bars.
        let visible = self.body_w.get() / self.zoom;
        let total = (song.length_qn() as f32 + 32.0).max(visible);
        let (page_frac, pos) = (visible / total, if total > visible { self.scroll / (total - visible) } else { 0.0 });
        if let Some(b) = page.find_mut("HorizontalScrollBar") {
            b.set_attr("realPage", page_frac.to_string());
            b.value = pos.clamp(0.0, 1.0);
        }
        if let Some(hp) = page.find_mut("TrackHeaderPanel") {
            for h in &mut hp.children {
                let sel = h.attr("songTrack").and_then(|v| v.parse::<usize>().ok()) == Some(model.engine);
                h.style = if sel { "SongTrackPanelSelectedStyle" } else { "SongTrackPanelStyle0" }.into();
            }
        }
    }

    /// Hover cursor over the song body: the operation a left press would
    /// start (as `handle`'s Down) through the operation's cursor [re:01ad57] —
    /// 2/3 band Selection, 5 change start ResizeRight, 6 resize ResizeLeft,
    /// 7 phase ResizeBiDir, 10 insert Pen, 13 / 14 / 15 loop start / end /
    /// move ResizeLeft / ResizeRight / ResizeBiDir, others the arrow.
    pub fn hover_cursor(&self, app: &App, model: &Model, path: &[usize], overload: &str, x: f32, y: f32) -> Option<dreist_gui::cursor::Cursor> {
        use dreist_gui::cursor::Cursor;
        if overload != "SongBody" {
            return None;
        }
        let c = app.ctrl(path)?;
        let rh = row_h(c);
        let qn = (x / self.zoom + self.scroll) as f64;
        let song = song_of(model);
        let op = if y < TIMELINE_H {
            let (ls, le) = ((song.loop_start as f32 - self.scroll) * self.zoom, (song.loop_end as f32 - self.scroll) * self.zoom);
            if song.loop_enabled && y > 9.0 && x > ls - 4.0 && x < le + 4.0 {
                let (ds, de) = ((x - ls).abs(), (x - le).abs());
                if ds < 4.0 && ds < de {
                    13
                } else if de < 4.0 && de <= ds {
                    14
                } else {
                    15
                }
            } else {
                16
            }
        } else {
            let track = ((y - TIMELINE_H) / rh) as usize;
            if track >= ENGINES {
                return Some(Cursor::Arrow);
            }
            if app.mods.shift || app.mods.cmd {
                3
            } else {
                match song.tracks.get(track).and_then(|t| t.iter().find(|p| p.start <= qn && qn < p.end())) {
                    None => 10,
                    Some(p) => {
                        let (x0, x1) = ((p.start as f32 - self.scroll) * self.zoom, (p.end() as f32 - self.scroll) * self.zoom);
                        let wide = x1 - x0 >= 16.0;
                        let row_y = y - TIMELINE_H - track as f32 * rh;
                        if wide && (x - x0).abs() < 4.0 {
                            5
                        } else if wide && (x1 - x).abs() < 4.0 {
                            6
                        } else if row_y > rh - 8.0 && wide {
                            7
                        } else {
                            1
                        }
                    }
                }
            }
        };
        Some(match op {
            2 | 3 => Cursor::image("Selection"),
            5 | 14 => Cursor::image("ResizeRight"),
            6 | 13 => Cursor::image("ResizeLeft"),
            7 | 15 => Cursor::image("ResizeBiDir"),
            8 | 12 => Cursor::image("Delete"),
            9 => Cursor::image("DragCopy"),
            10 | 11 => Cursor::image("Pen"),
            _ => Cursor::Arrow,
        })
    }

    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        match ev {
            UiEvent::Mouse { path, overload, kind, x, y } if overload == "SongBody" => {
                let Some(c) = app.ctrl(path) else { return false };
                let rh = row_h(c);
                let qn = (*x / self.zoom + self.scroll) as f64;
                // Timeline (left press [re:d8f27a] / mouse move).
                if let Some((op, grab, s0, e0)) = self.timeline {
                    let snap = |v: f64| quantize_round((v / self.snap_qn()).round() * self.snap_qn()).max(0.0);
                    match kind {
                        MouseKind::Drag => {
                            match op {
                                13 => out.push(msg(id::SONG_SET_INTERNAL_LOOP_RANGE, &[], &[snap(qn).min(e0 - self.snap_qn()) as f32, e0 as f32])),
                                14 => out.push(msg(id::SONG_SET_INTERNAL_LOOP_RANGE, &[], &[s0 as f32, snap(qn).max(s0 + self.snap_qn()) as f32])),
                                15 => {
                                    let d = (snap(qn - grab + s0) - s0).max(-s0);
                                    out.push(msg(id::SONG_SET_INTERNAL_LOOP_RANGE, &[], &[(s0 + d) as f32, (e0 + d) as f32]));
                                }
                                _ => {
                                    self.marker = snap(qn);
                                    if song_of(model).active && !model.playing {
                                        out.push(msg(id::SET_CURRENT_TIME, &[], &[self.marker as f32]));
                                    }
                                }
                            }
                            return true;
                        }
                        MouseKind::Up => {
                            self.timeline = None;
                            return true;
                        }
                        _ => {}
                    }
                }
                if *y < TIMELINE_H && *kind == MouseKind::Down {
                    let song = song_of(model);
                    let (ls, le) = ((song.loop_start as f32 - self.scroll) * self.zoom, (song.loop_end as f32 - self.scroll) * self.zoom);
                    let op = if song.loop_enabled && *y > 9.0 && *x > ls - 4.0 && *x < le + 4.0 {
                        let (ds, de) = ((*x - ls).abs(), (*x - le).abs());
                        if ds < 4.0 && ds < de {
                            13
                        } else if de < 4.0 && de <= ds {
                            14
                        } else {
                            15
                        }
                    } else {
                        self.marker = quantize_round((qn / self.snap_qn()).round() * self.snap_qn()).max(0.0);
                        // Current time reset [re:a4e021]: with the song on and the
                        // transport stopped, 0x1c `e_setCurrentTime`.
                        if song.active && !model.playing {
                            out.push(msg(id::SET_CURRENT_TIME, &[], &[self.marker as f32]));
                        }
                        16
                    };
                    self.timeline = Some((op, qn, song.loop_start, song.loop_end));
                    return true;
                }
                match kind {
                    MouseKind::Drag => {
                        if self.op.is_none() {
                            return false;
                        }
                        self.apply_op(model, qn, *x, *y, rh);
                        return true;
                    }
                    MouseKind::Up => {
                        let Some(op) = self.op.take() else { return false };
                        self.band = None;
                        if let Some(song) = self.working.take() {
                            Self::commit(&op.snapshot, &song, out);
                        }
                        return true;
                    }
                    _ => {}
                }
                if *y < TIMELINE_H {
                    return false;
                }
                let track = ((*y - TIMELINE_H) / rh) as usize;
                if track >= ENGINES {
                    return false;
                }
                let song = song_of(model);
                let hit = song.tracks.get(track).and_then(|t| t.iter().find(|p| p.start <= qn && qn < p.end())).cloned();
                match kind {
                    MouseKind::Down => {
                        model.engine = track;
                        let mods = app.mods;
                        let start_op = |me: &mut Self, kind: OpKind| {
                            me.op = Some(Op { kind, qn0: qn, x0: *x, y0: *y, snapshot: song.clone(), saved: me.selected.clone() });
                        };
                        // Shift / cmd: rubber band (op 3 / op 2).
                        if mods.shift || mods.cmd {
                            if !mods.shift {
                                self.selected.clear();
                            }
                            start_op(self, OpKind::Band(mods.shift));
                            self.apply_op(model, qn, *x, *y, rh);
                            return true;
                        }
                        let Some(p) = hit else {
                            // Empty space: a new default part (op 10).
                            let len = pattern_qn(model, track, self.insert_pattern);
                            let start = (qn / len.max(0.25)).floor() * len.max(0.25);
                            out.push(msg(id::SONG_ADD_PART, &[track as i32], &[start as f32, len as f32, self.insert_pattern as f32]));
                            self.selected.clear();
                            self.selected.insert((track, key(start)));
                            return true;
                        };
                        self.anchor = Some((track, key(p.start)));
                        if !self.selected.contains(&(track, key(p.start))) {
                            self.selected.clear();
                            self.selected.insert((track, key(p.start)));
                        }
                        // Edges within 4 px of a part at least 16 px wide.
                        let (x0, x1) = ((p.start as f32 - self.scroll) * self.zoom, (p.end() as f32 - self.scroll) * self.zoom);
                        let wide = x1 - x0 >= 16.0;
                        let row_y = *y - TIMELINE_H - track as f32 * rh;
                        let kind = if wide && (*x - x0).abs() < 4.0 {
                            OpKind::Start
                        } else if wide && (x1 - *x).abs() < 4.0 {
                            OpKind::Resize
                        } else if row_y > rh - 8.0 && wide {
                            OpKind::Phase
                        } else {
                            OpKind::Move
                        };
                        start_op(self, kind);
                        true
                    }
                    MouseKind::RightDown => {
                        if let Some(p) = &hit {
                            if !self.selected.contains(&(track, key(p.start))) {
                                self.selected.clear();
                                self.selected.insert((track, key(p.start)));
                            }
                        }
                        let path = path.clone();
                        self.context_menu(app, &path, qn);
                        true
                    }
                    _ => false,
                }
            }
            // Erase on while pressed.
            UiEvent::Pressed { path, name } if name == "EraseButton" && path.starts_with(&self.page) => {
                self.erase = true;
                out.push(msg(id::SONG_SET_ERASE_STATE, &[model.engine as i32], &[1.0]));
                true
            }
            // A pattern picked from the popup → pattern IDs set.
            UiEvent::MenuSelected { index, .. } if self.pattern_menu => {
                self.pattern_menu = false;
                self.change_patterns(model, None, Some((*index).min(23)), out);
                true
            }
            UiEvent::MenuSelected { index, .. } if self.menu.is_some() => {
                let (actions, qn) = self.menu.take().unwrap();
                let a = actions.get(*index).copied().unwrap_or(MenuAction::None);
                self.menu_action(app, model, a, qn, out)
            }
            UiEvent::Shortcut { key: k, cmd, .. } if model.editor_tab == SONG_TAB => {
                let a = match (k.as_str(), *cmd) {
                    ("delete", _) => MenuAction::Delete,
                    ("x", true) => MenuAction::Cut,
                    ("c", true) => MenuAction::Copy,
                    ("v", true) => MenuAction::Paste,
                    ("d", true) => MenuAction::Duplicate,
                    ("a", true) => MenuAction::SelectAll,
                    _ => return false,
                };
                let at = self.selected.iter().next().map(|s| s.1 as f64 / 960.0).unwrap_or(0.0);
                self.menu_action(app, model, a, at, out)
            }
            UiEvent::Wheel { overload, dy, .. } if overload == "SongBody" => {
                self.scroll = (self.scroll - dy * 4.0).max(0.0);
                true
            }
            UiEvent::ValueChanged { path, name, value, .. } if name == "HorizontalScrollBar" && path.starts_with(&self.page) => {
                let song = song_of(model);
                let visible = self.body_w.get() / self.zoom;
                let total = (song.length_qn() as f32 + 32.0).max(visible);
                self.scroll = value * (total - visible).max(0.0);
                true
            }
            UiEvent::Clicked { path, name, .. } if path.starts_with(&self.page) => {
                match name.as_str() {
                    // Control event [re:e3a0fd] → pattern IDs by delta.
                    "NextPatternButton" => self.change_patterns(model, Some(1), None, out),
                    "PrevPatternButton" => self.change_patterns(model, Some(-1), None, out),
                    "SelectPatternButton" => self.open_pattern_menu(app, model, path),
                    // Part buttons: select the anchor part, then as above.
                    "PartNextPatternButton" | "PartPrevPatternButton" | "PartSelectPatternButton" => {
                        if let Some(a) = self.anchor {
                            if !self.selected.contains(&a) {
                                self.selected.clear();
                            }
                            self.selected.insert(a);
                        }
                        match name.as_str() {
                            "PartNextPatternButton" => self.change_patterns(model, Some(1), None, out),
                            "PartPrevPatternButton" => self.change_patterns(model, Some(-1), None, out),
                            _ => self.open_pattern_menu(app, model, path),
                        }
                    }
                    // Momentary: the release ends the live erase.
                    "EraseButton" => {
                        self.erase = false;
                        out.push(msg(id::SONG_SET_ERASE_STATE, &[model.engine as i32], &[0.0]));
                    }
                    "ClearAllButton" => out.push(msg(id::SONG_CLEAR_ALL, &[], &[])),
                    "DuplicateButton" => {
                        self.menu_action(app, model, MenuAction::Duplicate, 0.0, out);
                    }
                    "ZoomInButton" => self.zoom = (self.zoom * 2.0).min(512.0),
                    "ZoomOutButton" => self.zoom = (self.zoom / 2.0).max(0.25),
                    _ => return false,
                }
                true
            }
            _ => false,
        }
    }

    fn menu_action(&mut self, app: &mut App, model: &mut Model, a: MenuAction, qn: f64, out: &mut Vec<Command>) -> bool {
        let song = song_of(model);
        let mut next = song.clone();
        let picked = selected_parts(&song, &self.selected);
        let remove = |s: &mut Song, parts: &[(usize, SongPart)]| {
            for (t, p) in parts {
                s.tracks[*t].retain(|q| key(q.start) != key(p.start));
            }
        };
        match a {
            MenuAction::None => return false,
            MenuAction::Copy | MenuAction::Cut => {
                let t0 = picked.iter().map(|(_, p)| p.start).fold(f64::MAX, f64::min);
                self.clipboard = picked.iter().map(|(t, p)| (*t, SongPart { start: p.start - t0, ..p.clone() })).collect();
                if a == MenuAction::Copy {
                    return false;
                }
                remove(&mut next, &picked);
                self.selected.clear();
            }
            MenuAction::Delete => {
                remove(&mut next, &picked);
                self.selected.clear();
            }
            MenuAction::Paste => {
                let _ = qn;
                let at = self.marker;
                self.selected.clear();
                for (t, p) in self.clipboard.clone() {
                    let np = SongPart { start: quantize_round(at + p.start), ..p };
                    self.selected.insert((t, key(np.start)));
                    next.add_part(t, np);
                }
            }
            // Duplicate [re:5e972b]: everything when nothing is selected;
            // copies follow the selection's end and become the selection.
            MenuAction::Duplicate => {
                let picked = if picked.is_empty() { song.tracks.iter().enumerate().flat_map(|(t, ps)| ps.iter().map(move |p| (t, p.clone()))).collect() } else { picked };
                if picked.is_empty() {
                    return false;
                }
                let lo = picked.iter().map(|(_, p)| p.start).fold(f64::MAX, f64::min);
                let hi = picked.iter().map(|(_, p)| p.end()).fold(0.0, f64::max);
                self.selected.clear();
                for (t, p) in picked {
                    let np = SongPart { start: quantize_round(p.start + hi - lo), ..p };
                    self.selected.insert((t, key(np.start)));
                    next.add_part(t, np);
                }
            }
            MenuAction::QuantizeStart | MenuAction::QuantizeBoth => {
                self.quantize(model, &mut next, a == MenuAction::QuantizeBoth);
                self.selected.clear();
            }
            MenuAction::QuantizeModeMenu => {
                if let Some(p) = app.root.path_of("SongBody") {
                    self.menu = Some(((0..3).map(MenuAction::QuantizeMode).collect(), qn));
                    app.open_menu(&p, QUANTIZE_MODES.iter().map(|s| s.to_string()).collect(), Some(self.quantize_mode));
                }
                return false;
            }
            MenuAction::QuantizeMode(m) => {
                self.quantize_mode = m;
                return false;
            }
            MenuAction::SelectAll => {
                self.selected = song.tracks.iter().enumerate().flat_map(|(t, ps)| ps.iter().map(move |p| (t, key(p.start)))).collect();
                return true;
            }
            // Parts whose pattern has no steps.
            MenuAction::SelectEmpty => {
                let m: &Model = model;
                let mut set = BTreeSet::new();
                for (t, ps) in song.tracks.iter().enumerate() {
                    for p in ps.iter().filter(|p| !m.pattern_used(t, p.pattern)) {
                        set.insert((t, key(p.start)));
                    }
                }
                self.selected = set;
                return true;
            }
            MenuAction::ClearAll => {
                out.push(msg(id::SONG_CLEAR_ALL, &[], &[]));
                self.selected.clear();
                return true;
            }
            MenuAction::EditPattern | MenuAction::RenamePattern => {
                let Some((t, p)) = picked.first().cloned() else { return false };
                model.engine = t;
                out.push(msg(id::SELECT_PATTERN, &[t as i32, -1, -1, p.pattern as i32], &[0.0]));
                let st = model.engine_mut(t).child_mut_or_insert("EngineState", "EngineState");
                st.set("CurrentPattern", p.pattern);
                model.editor_tab = 0;
                if a == MenuAction::RenamePattern {
                    if let Some(b) = app.root.path_of("PatternNameBox") {
                        app.begin_edit(&b);
                    }
                }
                return true;
            }
        }
        // Selection keys after the change.
        Self::commit(&song, &next, out);
        true
    }

    /// Recorder messages ([re:2af005]: 500, 0x1f2, 499, 0x1f8).
    /// Returns whether a new recording started (the editor's undo point
    /// "Record Song Parts").
    pub fn song_event(&mut self, model: &Model, ev: &SongEvent, out: &mut Vec<Command>) -> bool {
        match ev {
            SongEvent::State(st) => {
                let started = *st == 2 && self.rec_state != 2;
                self.rec_state = *st;
                if started {
                    self.retro = Default::default();
                    self.recording = Default::default();
                }
                started
            }
            SongEvent::Started { track, pattern, time, phase, .. } => {
                if let Some(slot) = self.recording.get_mut(*track) {
                    let playing = model.song_pos.is_some() && song_of(model).active;
                    *slot = Some((SongPart { start: *time, length: 1.0 / 960.0, pattern: *pattern, phase: *phase, mute: false }, playing));
                }
                false
            }
            // `PatternStopped` [re:be8eab].
            SongEvent::Stopped { track, pattern, time, recording } => {
                let Some((mut part, playing)) = self.recording.get_mut(*track).and_then(Option::take) else { return false };
                if part.pattern != *pattern {
                    return false;
                }
                let end = quantize_round(*time);
                if end > part.start {
                    part.length = end - part.start;
                    if *recording {
                        let before = song_of(model);
                        let mut song = before.clone();
                        song.add_part(*track, part);
                        Self::commit(&before, &song, out);
                    } else {
                        self.retro[*track].push((part, playing));
                    }
                }
                false
            }
            SongEvent::NewRetro => {
                self.retro = Default::default();
                false
            }
        }
    }

    /// Timer tick [re:220222]: parts being recorded grow to the
    /// song position; a jump back before a part's start drops it.
    pub fn tick(&mut self, model: &Model) {
        let Some(pos) = model.song_pos else { return };
        for slot in &mut self.recording {
            if let Some((p, _)) = slot {
                if pos > p.start {
                    p.length = pos - p.start;
                } else if quantize_round(pos.floor()) < p.start {
                    *slot = None;
                }
            }
        }
    }

    pub fn has_retro(&self) -> bool {
        self.retro.iter().any(|t| !t.is_empty())
    }

    /// Retro record [re:2cd575] (Retro button): the buffered
    /// parts go into the song, selected. Unless every part was played with
    /// the song running they are moved to start after the song's end (each
    /// track's last part rounded up to its pattern length), keeping their
    /// spacing from the earliest pattern-aligned start.
    pub fn apply_retro(&mut self, model: &Model, out: &mut Vec<Command>) -> bool {
        if !self.has_retro() {
            return false;
        }
        let mut earliest = 999999.0f64;
        let mut all_playing = true;
        for (t, parts) in self.retro.iter().enumerate() {
            for (p, playing) in parts {
                let len = retro_len(model, t, p.pattern);
                earliest = earliest.min(quantize_round((p.start / len).floor() * len));
                all_playing &= *playing;
            }
        }
        let before = song_of(model);
        let offset = if all_playing {
            0.0
        } else {
            let mut end = 0.0f64;
            for (t, parts) in before.tracks.iter().enumerate() {
                if let Some(last) = parts.last() {
                    let len = retro_len(model, t, last.pattern);
                    let e = quantize_round(last.start + last.length);
                    end = end.max(quantize_round(((e + len) / len).floor() * len).max(0.0));
                }
            }
            end - earliest
        };
        let mut song = before.clone();
        self.selected.clear();
        for (t, parts) in std::mem::take(&mut self.retro).into_iter().enumerate() {
            for (mut p, _) in parts {
                p.start += offset;
                self.selected.insert((t, key(p.start)));
                song.add_part(t, p);
            }
        }
        Self::commit(&before, &song, out);
        true
    }

    /// [re:6a5a7e] with
    /// `PatternEditorFollow`: a new playback position outside the visible
    /// body (no drag going on) scrolls it to the middle. True when it moved.
    pub fn follow(&mut self, pos: f64) -> bool {
        if self.op.is_some() || self.band.is_some() {
            return false;
        }
        let (left, w, p) = (self.scroll, self.body_w.get() / self.zoom, pos as f32);
        if p >= left && p <= left + w {
            return false;
        }
        let s = (p - w / 2.0).max(0.0);
        let moved = s != self.scroll;
        self.scroll = s;
        moved
    }

    /// Body-local x of the playback position line, or `None` when parts
    /// are being recorded (they grow with the position: the whole body
    /// changes).
    pub fn playhead_x(&self, pos: f64) -> Option<f32> {
        if self.recording.iter().any(Option::is_some) {
            return None;
        }
        Some((pos as f32 * self.zoom - self.scroll * self.zoom).round() + 0.5)
    }

    pub fn draw(&self, model: &Model, r: &Renderer, p: &mut Painter, c: &Ctrl) -> bool {
        if c.overload != "SongBody" {
            return false;
        }
        let s = r.styles;
        let song = self.working.clone().unwrap_or_else(|| song_of(model));
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        self.body_w.set(w);
        let rh = row_h(c);
        let pink = Colour::rgba(255, 200, 200, 255);
        let off = self.scroll * self.zoom;
        // Track rows.
        for e in 0..ENGINES {
            let st = if e == model.engine { "SongTrackPanelSelectedStyle" } else { "SongBodyPanelStyle" };
            let y = TIMELINE_H + e as f32 * rh;
            p.fill_rect(RectF::new(0.0, y, w, rh), s.colour(st, "colFill", Colour::WHITE));
            let b = s.colour(st, "colBorder", Colour::BLACK);
            p.line(0.0, y + rh - 0.5, w, y + rh - 0.5, b, 1.0);
        }
        // Timeline + grid.
        p.fill_rect(RectF::new(0.0, 0.0, w, TIMELINE_H), s.colour("SongTimelineStyle", "colFill", Colour::TRANSPARENT));
        let bar = s.colour("PatternBackPanelStyle", "colGridBar", pink);
        let font = s.font("SongTimelineStyle");
        let text = s.colour("SongTimelineStyle", "colText", Colour::BLACK);
        let bar_qn = 4.0f32;
        let first_bar = (self.scroll / bar_qn).floor() as usize;
        let bars = (w / (bar_qn * self.zoom)).ceil() as usize + 2;
        let label_every = ((48.0 / (bar_qn * self.zoom)).ceil() as usize).max(1);
        for b in first_bar..first_bar + bars {
            let x = (b as f32 * bar_qn * self.zoom - off).round() + 0.5;
            p.line(x, 0.0, x, h, bar, 1.0);
            if b % label_every == 0 {
                p.text(RectF::new(x + 2.0, 2.0, 50.0, 12.0), &format!("{}", b + 1), text, &font, HAlign::Left, VAlign::Center);
            }
            if self.zoom * bar_qn >= 32.0 {
                for beat in 1..4 {
                    let bx = ((b as f32 * bar_qn + beat as f32) * self.zoom - off).round() + 0.5;
                    p.line(bx, TIMELINE_H, bx, h, bar.with_alpha(bar.f_alpha() * 0.5), 1.0);
                }
            }
        }
        // Loop range.
        if song.loop_enabled {
            let col = s.colour("SongPanelStyle", "colLoopInternal", pink);
            p.fill_rect(RectF::new(song.loop_start as f32 * self.zoom - off, 0.0, ((song.loop_end - song.loop_start) as f32 * self.zoom).max(0.0), TIMELINE_H), col);
        }
        // Parts.
        let fill = s.colour(PART_STYLE, "colPart", pink);
        let border = s.colour(PART_STYLE, "colBorder", Colour::BLACK);
        let ptext = s.colour(PART_STYLE, "colText", Colour::BLACK);
        let pfont = s.font(PART_STYLE);
        let radius = s.float(PART_STYLE, "realCornerRounding", 7.0).floor().min(rh / 2.0);
        let sel_col = s.colour(PART_STYLE, "colSelected", pink);
        for (e, parts) in song.tracks.iter().enumerate() {
            let y = TIMELINE_H + e as f32 * rh;
            for part in parts {
                let start = part.start;
                let x0 = start as f32 * self.zoom - off;
                let x1 = (start + part.length) as f32 * self.zoom - off;
                if x1 < 0.0 || x0 > w {
                    continue;
                }
                let rect = RectF::new(x0.round() - 1.0, y + 1.0, (x1 - x0).round() + 1.0, rh - 2.0);
                let selected = self.selected.contains(&(e, key(part.start)));
                let base = if selected { sel_col } else { fill };
                let col = if part.mute { base.with_alpha(base.f_alpha() * 0.5) } else { base };
                p.fill_rounded(rect, radius, [true; 4], col);
                p.stroke_rounded(rect.inset(0.5), radius, [true; 4], border, 1.0);
                // Pattern repeat dividers.
                let plen = cycle_qn(model, e, part.pattern) as f32;
                if plen > 0.0 {
                    // Pattern starts after the phase offset.
                    let mut t = start as f32 + (plen - (part.phase as f32).rem_euclid(plen)) % plen;
                    if t <= start as f32 + 1e-3 {
                        t += plen;
                    }
                    while t < (start + part.length) as f32 - 1e-3 {
                        let dx = (t * self.zoom - off).round() + 0.5;
                        p.line(dx, y + 4.0, dx, y + rh - 4.0, border.with_alpha(0.4), 1.0);
                        t += plen;
                    }
                }
                let name = if part.pattern == STOP_PART {
                    String::new()
                } else {
                    let n = model.pattern_name(e, part.pattern);
                    if n.is_empty() { format!("Pattern {}", part.pattern + 1) } else { n }
                };
                if rect.w > 14.0 {
                    p.text(RectF::new(rect.x + 6.0, y + 4.0, rect.w - 10.0, 12.0), &name, ptext, &pfont, HAlign::Left, VAlign::Center);
                }
            }
        }
        // Parts being recorded (colRecording) and retro parts (colRetro).
        let rec_col = s.colour(PART_STYLE, "colRecording", pink);
        let retro_col = s.colour(PART_STYLE, "colRetro", pink);
        let grow = |p: &SongPart| model.song_pos.filter(|pos| *pos > p.start).map(|pos| pos - p.start).unwrap_or(p.length);
        let extra = self
            .recording
            .iter()
            .enumerate()
            .filter_map(|(t, x)| x.as_ref().map(|(p, _)| (t, p.start, grow(p), rec_col)))
            .chain(self.retro.iter().enumerate().flat_map(|(t, ps)| ps.iter().map(move |(p, _)| (t, p.start, p.length, retro_col))));
        for (t, start, length, col) in extra {
            let y = TIMELINE_H + t as f32 * rh;
            let x0 = start as f32 * self.zoom - off;
            let x1 = (start + length) as f32 * self.zoom - off;
            if x1 < 0.0 || x0 > w {
                continue;
            }
            let rect = RectF::new(x0.round() - 1.0, y + 1.0, (x1 - x0).round().max(1.0) + 1.0, rh - 2.0);
            p.fill_rounded(rect, radius, [true; 4], col);
            p.stroke_rounded(rect.inset(0.5), radius, [true; 4], border, 1.0);
        }
        // Insert marker.
        let mx = ((self.marker as f32 - self.scroll) * self.zoom).round() + 0.5;
        p.line(mx, 0.0, mx, TIMELINE_H, s.colour("SongPanelStyle", "colInsertMarker", s.colour("SongPanelStyle", "colPosition", Colour::BLACK)), 1.0);
        // Rubber band.
        if let Some(b) = self.band {
            let col = s.colour(PART_STYLE, "colSelected", pink);
            p.fill_rect(b, col.with_alpha(0.25));
            p.stroke_rect(b.inset(0.5), col, 1.0);
        }
        // Playback position.
        if let Some(pos) = model.song_pos.filter(|_| song.active) {
            let x = (pos as f32 * self.zoom - off).round() + 0.5;
            p.line(x, 0.0, x, h, s.colour("SongPanelStyle", "colPosition", Colour::BLACK), 1.0);
        }
        true
    }
}
