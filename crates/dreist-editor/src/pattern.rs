//! Pattern panel: `PatternPanel.skin` with the toolbar (`Toolbar.skin`,
//! [re:bca08c]) and the pattern editor ([re:677fc4]).
//! The step grid, track rows and timeline are code-drawn as in
//! [re:f3c26a] / [re:f32484] /
//! [re:477245] (see the dreist-re GUI notes):
//!
//! - background: groups of a beat's steps alternate `colFill` /
//!   `colAltFill` ([re:ac984a]);
//! - the selected track's row in `colSelectedTrack` ([re:5a68a7]);
//! - grid lines: one per step in `colGridBeat`, every 4th in `colGridBar`,
//!   and track separators in `colGridBar` α0.35 ([re:60f2b5], [re:ecd2fe]);
//! - steps ([re:cef393], [re:b3cfd7]): inset
//!   2/1/4/3 when the row is ≥ 10 px, rounded rect radius 2 (or h/4 when
//!   small) in `colStep` at alpha `vel·0.85 + 0.15`; repeats subdivide
//!   one-shot steps;
//! - beyond the track length: `colOverlay` (track drawing);
//! - the current step column in `colPosition` ([re:bfe463]);
//! - pattern power off: `colPowerOverlay` α0.3 and "Pattern power off"
//!   ([re:33022b]);
//! - timeline: a line and the step number every 4 steps (every 16 when
//!   steps are ≤ 7 px wide) ([re:34ab57]).
//!
//! Editing posts `e_setStepOn` 0xd2 / `e_setStepOff` 0xd3 (index: engine,
//! −1, −1, pattern, track, step), `e_selectPattern` 0xa9,
//! `e_setPatternStepCount` 0xb5 and the track mute/solo messages 0xbb/0xbc.

use dreist_engine::binding::id;
use dreist_engine::pattern::{PatternData, RESOLUTION_NAMES, lane};
use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::render::Renderer;
use dreist_gui::{Colour, Ctrl, Rect, Skin};

use crate::command::{Command, msg};
use crate::model::Model;
use crate::{EditorError, err};

const BACK_STYLE: &str = "PatternBackPanelStyle";
const TRACK_STYLE: &str = "TrackStyle";
const TIMELINE_STYLE: &str = "PatternTimelineStyle";

/// Items above the templates in the swing menu.
const SWING_HEAD: usize = 4;
/// The text box the Swing knob's Edit puts over it.
const SWING_EDIT: &str = "SwingValueEdit";

pub struct PatternPanel {
    /// The panel's page: its control clicks come from under it (the header
    /// shares names like OptionsButton and QuantizeButton).
    page: Vec<usize>,
    grid: Option<Vec<usize>>,
    rows: Option<Vec<usize>>,
    row_template: Option<Ctrl>,
    selectors: Option<Vec<usize>>,
    /// Multi-tool press on a step ([re:86dd7e]..).
    brush: Option<Brush>,
    /// A track length marker drag: the track, the marker's x and whether
    /// every track follows (shift / cmd).
    marker_drag: Option<(usize, f32, bool)>,
    /// Drag Edits value carried between presses (starts at 1).
    brush_value: f32,
    /// Right-button erase: track and last erased step.
    erase: Option<(usize, usize)>,
    /// Presses drag a duration chain's end (kept between presses).
    dur_drag: bool,
    /// Grid width at the last sync (the timeline uses the same steps).
    grid_w: std::cell::Cell<i32>,
    /// The current pattern as the draw functions read it, built once per
    /// frame ([`PatternPanel::begin_frame`]): (engine, pattern, data).
    frame_pattern: std::cell::RefCell<Option<(usize, usize, std::rc::Rc<PatternData>)>>,
    /// The controls `sync` sets, and the toolbar's (under `ToolBar`).
    named: dreist_gui::app::PathCache,
    toolbar: dreist_gui::app::PathCache,
    /// The graph lane view ([re:e9d6e3]) and its last edited step.
    graph: Option<Vec<usize>>,
    graph_last: Option<usize>,
    /// Graph drag state ([re:79aee7] anchor step,
    /// value, last y, line mode, erasing).
    graph_drag_state: GraphDrag,
    /// Track, lane and first step the graph menu acts on (right-click on
    /// a graph; None = the selected track's lane from step 0).
    graph_target: Option<(usize, usize, usize)>,
    /// The lane menu shows the FX lanes.
    lane_menu_fx: bool,
    /// Lane values when a graph knob started moving (copied),
    /// the knob, and the last offset.
    graph_snap: Option<(String, Vec<f32>, f32)>,
    rng: u32,
    /// Selected steps (track, step) and the rubber band being dragged.
    pub(crate) selection: std::collections::BTreeSet<(usize, usize)>,
    band: Option<((usize, usize), (usize, usize))>,
    /// Copied steps relative to the top-left of the copied selection.
    clipboard: Vec<(usize, usize, dreist_engine::pattern::StepData)>,
    /// Graph Options menu open (false = top level, true = Insert submenu).
    graph_menu: Option<bool>,
    /// Copied lane values (graph menu Copy).
    lane_clip: Vec<f32>,
    /// Toolbar Options menu open: the actions of its items, or the Drag
    /// Edits lanes when the submenu / DragEdit popup is open.
    toolbar_menu: Option<Vec<ToolbarAction>>,
    /// `dreist_formats::paths::skin_dir` (factory swing templates live
    /// beside it).
    resources: std::path::PathBuf,
    /// Swing menu entries (template paths) while open.
    swing_menu: Option<Vec<std::path::PathBuf>>,
    /// Swing menu › Extract timing from current pattern: the pattern whose
    /// groove the editor saves ([re:51ad7b]).
    pub groove: Option<usize>,
    /// Swing menu › Import MIDI template ([re:0c1fbf]).
    pub import_midi_template: bool,
    /// Swing menu › Import Guru shuffle was picked.
    pub import_guru_swing: bool,
    /// The swing menu was opened from the Swing knob (adds Reset / Edit).
    swing_knob_menu: bool,
    /// `PatternEditorViewMode` "MultiTrack" (inline graph lanes under each
    /// opened track) or "SingleGraph" (toolbar View: one global graph).
    pub multi: bool,
    /// MultiTrack lanes per (engine, track) ([re:b8d7a4])
    /// and the tracks whose automation is open ([re:a42ffd]).
    auto_lanes: std::collections::HashMap<(usize, usize), Vec<usize>>,
    auto_open: std::collections::HashSet<(usize, usize)>,
    /// `AutomationControls.skin` for MultiTrack lane rows.
    auto_template: Option<Ctrl>,
    /// Lane slot whose GraphCombo menu is open: (track, slot).
    auto_ctx: Option<(usize, usize)>,
    /// Graph row being drawn on: (track, lane, top y, height).
    graph_drag: Option<(usize, usize, f32, f32)>,
    /// Layout signature of the controls column.
    rows_sig: String,
    splitter: Option<Vec<usize>>,
}

/// One MultiTrack graph row: lane `lane` (slot `slot`) of `track`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphRow {
    pub track: usize,
    pub slot: usize,
    pub lane: usize,
    pub y: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ToolbarAction {
    DragEdits,
    DragLane(usize),
    SingleTrack,
    Quantize,
    Duplicate,
    Remix,
    Gated,
    Restart,
    None,
}

/// Lanes offered by "Drag Edits..." ([re:4120dd] 0..0x27 without
/// Duration and LinkToPrevious).
fn drag_lanes() -> Vec<usize> {
    (0..CORE_LANES).filter(|&l| l != 9 && l != 10).collect()
}

/// [re:e12d93] / duplicating the selected steps
/// [re:7b48a6] ([re:34a571]): with no selection the
/// pattern doubles (at most 1024 steps) and the steps repeat; otherwise the
/// selected steps are copied one selection span later (the pattern doubles
/// when the copies would not fit) and the selection moves to the copies.
fn duplicate(pat: &PatternData, selection: &std::collections::BTreeSet<(usize, usize)>, e: usize, cur: i32, out: &mut Vec<Command>) -> std::collections::BTreeSet<(usize, usize)> {
    let n = pat.steps as usize;
    let get = |t: usize, s: usize| pat.tracks.get(t).and_then(|tr| tr.steps.get(s)).cloned();
    let grow = |out: &mut Vec<Command>, to: usize| out.push(msg(id::SET_PATTERN_STEP_COUNT, &[e as i32, -1, -1, cur], &[to as f32]));
    if selection.is_empty() {
        let m = (n * 2).min(1024);
        if m == n {
            return Default::default();
        }
        grow(out, m);
        for t in 0..pat.tracks.len() {
            for s in 0..m - n {
                if let Some(d) = get(t, s).filter(|d| d.on) {
                    write_step(out, e, cur, t, n + s, &d);
                }
            }
        }
        return Default::default();
    }
    let first = selection.iter().map(|x| x.1).min().unwrap_or(0);
    let last = selection.iter().map(|x| x.1).max().unwrap_or(0);
    let span = last - first + 1;
    let mut m = n;
    if last + span >= n {
        m = (n * 2).min(1024);
        if m != n {
            grow(out, m);
        }
    }
    let mut moved = std::collections::BTreeSet::new();
    for &(t, s) in selection {
        let Some(d) = get(t, s).filter(|d| d.on) else { continue };
        if s + span < m {
            write_step(out, e, cur, t, s + span, &d);
            moved.insert((t, s + span));
        }
    }
    moved
}

/// [re:45e9a3] (CopyToNext) [re:8c2861]: copy the pattern to
/// the first empty pattern after it (up to 23), named "<name> (Copy)", and
/// select it. Returns the new pattern.
pub fn copy_to_next(model: &mut Model, e: usize) -> Option<usize> {
    let cur = model.current_pattern(e);
    let to = (cur + 1..24).find(|&p| !model.pattern_used(e, p))?;
    let mut node = model.pattern_node(e, cur).cloned().unwrap_or_else(|| dreist_formats::Node::new("StepPattern", Some(&format!("StepPattern{cur}"))));
    node.name = Some(format!("StepPattern{to}"));
    let name = format!("{} (Copy)", model.pattern_name(e, cur));
    let eng = model.doc.children.iter_mut().find(|c| c.class == "Engine" && c.name.as_deref() == Some(&format!("Engine{e}")))?;
    eng.children.retain(|c| !(c.class == "StepPattern" && c.name.as_deref() == Some(&format!("StepPattern{to}"))));
    eng.children.push(node);
    model.set_pattern_name(e, to, &name);
    set_current_pattern(model, e, to);
    Some(to)
}

/// [re:e64ec5].
pub const INSERTABLE: [&str; 15] =
    ["Ramp Up", "Ramp Down", "Sine", "Cosine", "Triangle", "Square Up", "Square Down", "Alternate", "Even On", "Even Off", "Log Curve", "Exp Curve", "Pow2", "Pow3", "Randomise"];
const GRAPH_MENU: [&str; 13] = [
    "Reset to default values", "", "Cut", "Copy", "Paste", "", "Clear automation track", "Clear all automation tracks", "Clear all pattern automation tracks", "Invert automation values",
    "Reverse automation values", "", "Insert ▸",
];

#[derive(Clone, Copy, Debug, Default)]
struct GraphDrag {
    anchor: usize,
    value: f32,
    last_y: f32,
    line: bool,
    erase: bool,
}

/// [re:73dd2d]: lane values for `n`
/// steps (`r` supplies uniform randoms for Randomise).
pub fn insert_shape(shape: usize, n: usize, r: &mut dyn FnMut() -> f32) -> Vec<f32> {
    insert_from(shape, n, 0, r).into_iter().map(|(_, v)| v).collect()
}

/// Automation inserted from the right-clicked step `start`: the shape's
/// phase starts at `start` (ramps, sines, curves restart there), while the
/// triangle's falling half, the square switch and the parity patterns use
/// absolute step numbers as the original (the falling half of Triangle is
/// written from the middle even when `start` lies past it). (step, value).
pub fn insert_from(shape: usize, n: usize, start: usize, r: &mut dyn FnMut() -> f32) -> Vec<(usize, f32)> {
    let mut out = Vec::new();
    if start >= n && shape != 4 {
        return out;
    }
    let dt = 1.0 / n.max(1) as f32;
    let half = n / 2;
    match shape {
        4 => {
            let d = 1.0 / half.max(1) as f32;
            let mut v = 0.0f32;
            for i in start..half {
                out.push((i, v));
                v += d;
            }
            for i in half..n {
                out.push((i, v));
                v -= d;
            }
        }
        5 | 6 => {
            let mut v = if shape == 5 { 1.0 } else { 0.0 };
            for i in start..n {
                out.push((i, v));
                if i + 1 >= half {
                    v = if shape == 5 { 0.0 } else { 1.0 };
                }
            }
        }
        7 => {
            let d = 1.0 / half.max(1) as f32;
            let (mut even, mut odd) = (1.0f32, 0.0f32);
            for i in start..n {
                if i % 2 == 0 {
                    out.push((i, even));
                    even -= d;
                } else {
                    out.push((i, odd));
                    odd += d;
                }
            }
        }
        8 | 9 => out.extend((start..n).map(|i| (i, ((i % 2 == 0) == (shape == 8)) as i32 as f32))),
        _ => {
            for (k, i) in (start..n).enumerate() {
                let t = k as f32 * dt;
                let v = match shape {
                    0 => t,
                    1 => 1.0 - t,
                    2 => ((t * std::f32::consts::TAU).sin() + 1.0) * 0.5,
                    3 => ((t * std::f32::consts::TAU).cos() + 1.0) * 0.5,
                    10 => ((t + 0.0001).ln().abs() * 0.25).clamp(0.0, 1.0),
                    11 => ((t + 0.0001).ln().abs() * 0.367_879_45).clamp(0.0, 1.0),
                    // "Pow2"/"Pow3" are sqrt and fourth root (rsqrtss, powf 0.25).
                    12 => (1.0 - t).max(0.0).sqrt(),
                    13 => (1.0 - t).max(0.0).powf(0.25),
                    _ => r().clamp(0.0, 1.0),
                };
                out.push((i, v));
            }
        }
    }
    out.into_iter().map(|(i, v)| (i, v.clamp(0.0, 1.0))).collect()
}

/// [re:6fb427] 0x4530 state of the 24 `Key<n>`
/// selectors under `path`: on = current pattern, `patternUsed` (not empty),
/// `patternLocked` (the lock glyph).
pub(crate) fn sync_selectors(app: &mut App, model: &Model, path: &[usize]) {
    let e = model.engine;
    let cur = model.current_pattern(e);
    let used: Vec<bool> = (0..24).map(|p| model.pattern_used(e, p)).collect();
    let locked: Vec<bool> = (0..24).map(|p| model.pattern_node(e, p).is_some_and(|n| n.locked())).collect();
    let Some(s) = app.ctrl_mut(path) else { return };
    fn walk(c: &mut Ctrl, cur: usize, used: &[bool], locked: &[bool]) {
        for k in &mut c.children {
            if let Some(n) = k.name.strip_prefix("Key").and_then(|n| n.parse::<usize>().ok()) {
                k.on = n == cur;
                k.set_attr("patternUsed", used.get(n).copied().unwrap_or(false).to_string());
                k.set_attr("patternLocked", locked.get(n).copied().unwrap_or(false).to_string());
            }
            walk(k, cur, used, locked);
        }
    }
    walk(s, cur, &used, &locked);
}

/// [re:3e59b6] over the stock button: a disc
/// (x 4, w − 8 wide, centred) in LightGrey (empty), DarkGrey (used) or
/// White (current), and U+E915 in colText when the pattern is locked.
pub(crate) fn draw_selector(r: &Renderer, p: &mut Painter, c: &Ctrl) {
    let (w, h) = (c.rect.w as f32, c.rect.h as f32);
    let col = if c.on {
        Colour::WHITE
    } else if c.attr("patternUsed") == Some("true") {
        dreist_gui::colour::standard("DarkGrey")
    } else {
        dreist_gui::colour::standard("LightGrey")
    };
    let wi = c.rect.w;
    let hi = c.rect.h;
    let x = (wi / 2 + 4 - wi / 2) as f32;
    let y = (hi / 2 + 4 - wi / 2) as f32;
    let d = (wi - 8) as f32;
    p.fill_ellipse(RectF::new(x, y, d, d), col);
    if c.attr("patternLocked") == Some("true") {
        let st = r.style(c);
        let font = dreist_gui::style::FontSpec { family: dreist_gui::text::icon_family().into(), size: r.styles.font(st).size, bold: false };
        let text = r.styles.colour(st, "colText", Colour::BLACK);
        p.text(RectF::new(0.0, 0.0, w, h - 1.0), "\u{e915}", text, &font, dreist_gui::style::HAlign::Center, dreist_gui::style::VAlign::Center);
    }
    // Layer 3: the drop highlight (set on drag move) in
    // StandardControlHighlight at the pulsing alpha.
    if let Some(a) = c.attr("dragAlpha").and_then(|v| v.parse::<u8>().ok()) {
        let col = dreist_gui::colour::standard("StandardControlHighlight");
        p.fill_rect(RectF::new(0.0, 0.0, w, h), Colour::rgba(col.r, col.g, col.b, a));
    }
}

/// Post the messages that make step (`track`, `step`) equal `d`.
pub(crate) fn write_step(out: &mut Vec<Command>, e: usize, pat: i32, track: usize, step: usize, d: &dreist_engine::pattern::StepData) {
    let index = [e as i32, -1, -1, pat, track as i32, step as i32];
    if !d.on {
        out.push(msg(id::SET_STEP_OFF, &index, &[0.0]));
        return;
    }
    out.push(msg(id::SET_STEP_ON, &index, &[d.lanes[lane::VELOCITY], d.lanes[lane::DURATION], d.lanes[lane::LINK]]));
    for (k, v) in d.lanes.iter().enumerate().skip(1) {
        if (v - dreist_formats::params::step_lane_default(k)).abs() > 1e-6 {
            out.push(msg(id::SET_STEP_GRAPH_STATE, &[e as i32, track as i32, -1, pat, track as i32, step as i32], &[k as f32, *v, 0.0]));
        }
    }
}

/// Pattern editor view: horizontal zoom (steps magnified ×h_zoom), row
/// height multiplier, and the first visible step / row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternView {
    pub h_zoom: u32,
    pub v_zoom: f32,
    pub h_scroll: usize,
    pub v_scroll: usize,
    /// Lane shown in the graph view ([re:0e63fa]).
    pub lane: usize,
    /// Toolbar "Drag Edits..." lane ([re:60eade]).
    pub drag_lane: usize,
}

impl Default for PatternView {
    fn default() -> Self {
        PatternView { h_zoom: 1, v_zoom: 1.0, h_scroll: 0, v_scroll: 0, lane: 0, drag_lane: 0 }
    }
}

/// Track length marker size ([re:f32484] ctor [re:74d707]).
const MARKER_W: f32 = 10.0;
const MARKER_H: f32 = 15.0;

/// Lanes offered directly in the graph menu; the FX lanes go in a second
/// menu.
const CORE_LANES: usize = 39;

fn lane_label(lane: usize) -> String {
    dreist_formats::STEP_LANES.get(lane).map(|s| crate::transmod::camel_words(s)).unwrap_or_default()
}

/// Grid geometry for the visible tracks.
#[derive(Clone, Debug)]
pub struct GridLayout {
    /// First visible track (absolute pad index).
    pub first_track: usize,
    /// Visible track rows.
    pub tracks: usize,
    /// Tracks in the bank (16 or 64).
    pub total_tracks: usize,
    pub row_h: f32,
    pub step_w: f32,
    /// Steps in the pattern.
    pub steps: usize,
    pub first_step: usize,
    /// Steps that fit the width.
    pub visible_steps: usize,
    /// Top of each visible track row.
    pub track_y: Vec<f32>,
    /// MultiTrack graph rows below opened tracks.
    pub graphs: Vec<GraphRow>,
}

impl GridLayout {
    pub fn compute(model: &Model, pat: &PatternData, w: i32, h: i32) -> GridLayout {
        let view = model.pattern_view[model.engine];
        let (first, count) = model.visible_pads();
        let steps = pat.steps.max(1) as usize;
        // Track dimensions: rows at least 12 px, times the zoom.
        let row_h = ((h as f32 / count as f32).max(12.0) * view.v_zoom.max(1.0)).floor();
        let rows_fit = ((h as f32 / row_h).floor() as usize).clamp(1, count);
        let first_row = view.v_scroll.min(count - rows_fit);
        let step_w = (w as f32 / steps as f32).max(4.0) * view.h_zoom.max(1) as f32;
        let visible_steps = ((w as f32 / step_w).floor() as usize).clamp(1, steps);
        let first_step = view.h_scroll.min(steps - visible_steps);
        let track_y = (0..rows_fit).map(|r| r as f32 * row_h).collect();
        GridLayout { first_track: first + first_row, tracks: rows_fit, total_tracks: count, row_h, step_w, steps, first_step, visible_steps, track_y, graphs: Vec::new() }
    }

    /// MultiTrack layout (track dimensions [re:b32367]): each opened
    /// track is followed by one graph row per lane, max(2·row height, 75)
    /// px high; rows start at the scrolled-to track.
    pub fn compute_multi(model: &Model, pat: &PatternData, w: i32, h: i32, lanes_of: &dyn Fn(usize) -> Vec<usize>) -> GridLayout {
        let mut lay = GridLayout::compute(model, pat, w, h);
        let view = model.pattern_view[model.engine];
        let (first, count) = model.visible_pads();
        let graph_h = (lay.row_h * 2.0).max(75.0).floor();
        let first_row = view.v_scroll.min(count.saturating_sub(1));
        let (mut y, mut track_y, mut graphs) = (0.0f32, Vec::new(), Vec::new());
        for t in first + first_row..first + count {
            if y >= h as f32 && !track_y.is_empty() {
                break;
            }
            track_y.push(y);
            y += lay.row_h;
            for (slot, lane) in lanes_of(t).into_iter().enumerate() {
                graphs.push(GraphRow { track: t, slot, lane, y, h: graph_h });
                y += graph_h;
            }
        }
        lay.first_track = first + first_row;
        lay.tracks = track_y.len().max(1);
        lay.track_y = if track_y.is_empty() { vec![0.0] } else { track_y };
        lay.graphs = graphs;
        lay
    }

    /// Top of visible track row `row`.
    pub fn row_y(&self, row: usize) -> f32 {
        self.track_y.get(row).copied().unwrap_or(row as f32 * self.row_h)
    }

    /// The graph row at `y`.
    pub fn graph_at(&self, y: f32) -> Option<GraphRow> {
        self.graphs.iter().copied().find(|g| y >= g.y && y < g.y + g.h)
    }

    /// Visible step columns.
    pub fn step_range(&self) -> std::ops::Range<usize> {
        self.first_step..(self.first_step + self.visible_steps + 1).min(self.steps)
    }

    pub fn step_x(&self, step: usize) -> f32 {
        (step as f32 - self.first_step as f32) * self.step_w
    }

    pub fn step_rect(&self, row: usize, step: usize) -> RectF {
        RectF::new(self.step_x(step), self.row_y(row), self.step_w, self.row_h)
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let row = self.track_y.iter().position(|&ty| y >= ty && y < ty + self.row_h)?;
        let step = self.first_step + (x / self.step_w) as usize;
        (row < self.tracks && step < self.steps).then_some((self.first_track + row, step))
    }
}

fn mount(skin: &mut Skin, root: &mut Ctrl, host_name: &str, overload: &str, skin_name: &str) -> Result<(), EditorError> {
    let found = {
        fn find<'a>(c: &'a mut Ctrl, name: &str, overload: &str) -> Option<&'a mut Ctrl> {
            if c.name == name && (overload.is_empty() || c.overload == overload) && c.children.is_empty() {
                return Some(c);
            }
            c.children.iter_mut().find_map(|ch| find(ch, name, overload))
        }
        find(root, host_name, overload)
    };
    if let Some(h) = found {
        skin.mount(h, skin_name).map_err(err)?;
    }
    Ok(())
}

fn new_custom(name: &str, overload: &str, w: i32, h: i32) -> Ctrl {
    Ctrl {
        class: "CtrlPanel".into(),
        name: name.into(),
        overload: overload.into(),
        style: "InvisibleStyle".into(),
        rect: Rect::new(0, 0, w, h),
        anchors: dreist_gui::anchors_fill(),
        visible: true,
        enabled: true,
        zorder: 1,
        ..Ctrl::default()
    }
}

impl PatternPanel {
    pub fn build(skin: &mut Skin, root: &mut Ctrl, page: &[usize]) -> Result<PatternPanel, EditorError> {
        {
            let mut host = &mut *root;
            for &i in page {
                host = &mut host.children[i];
            }
            skin.mount(host, "PatternPanel.skin").map_err(err)?;
            mount(skin, host, "ToolBar", "PatternEditorToolBar", "Toolbar.skin")?;
            mount(skin, host, "Selectors", "PatternSelectors", "PatternSelectors.skin")?;
            mount(skin, host, "PatternEditor", "PatternEditor", "PatternEditor.skin")?;
            // The View toggle sits under the Options button; Options › Single
            // Track View drives it.
            if let Some(v) = host.find_mut("ViewPanel") {
                v.visible = false;
            }
            // The single-graph view (`SingleViewPanel`, intMaxSize1 75 px)
            // under the step panel: graph controls + the lane graph.
            if let Some(sp) = host.find_mut("Splitter") {
                let (h, dim) = (sp.rect.h, sp.attr_i32("intSplitterDim", 1));
                let lower = sp.attr_i32("intMaxSize1", 75);
                sp.set_attr("intSplitPosition", (h - lower - dim).to_string());
                sp.layout_splitter();
            }
            if let Some(ac) = host.find_mut("SingleAutoControl") {
                ac.overload.clear();
                if ac.children.is_empty() {
                    skin.mount(ac, "AutomationControls.skin").map_err(err)?;
                }
                for n in ["AddButton", "RemoveButton", "Learn"] {
                    if let Some(b) = ac.find_mut(n) {
                        b.visible = false;
                    }
                }
            }
            if let Some(gv) = host.find_mut("SingleScrollView") {
                let (w, h) = (gv.rect.w, gv.rect.h);
                gv.children.push(new_custom("PatternGraph", "PatternGraph", w, h));
            }
            // The wdButton overloads behave as stock momentary buttons.
            fn stock(c: &mut Ctrl) {
                for k in &mut c.children {
                    if k.overload == "wdButton" {
                        k.overload.clear();
                    }
                    stock(k);
                }
            }
            stock(host);
            if let Some(sv) = host.find_mut("PatternScrollView") {
                let (w, h) = (sv.rect.w, sv.rect.h);
                sv.children.push(new_custom("PatternGrid", "PatternGrid", w, h));
            }
            if let Some(tv) = host.find_mut("TimelineView") {
                let (w, h) = (tv.rect.w, tv.rect.h);
                tv.style = TIMELINE_STYLE.into();
                tv.children.push(new_custom("PatternTimeline", "PatternTimeline", w, h));
            }
        }
        let auto_rel = skin.find("AutomationControls.skin").unwrap_or_else(|| "Patterns/AutomationControls.skin".into());
        let auto_template = skin.load_skin(&auto_rel).ok().and_then(|r| r.children.into_iter().next()).map(|mut t| {
            // Lane MIDI learn is not ported.
            if let Some(b) = t.find_mut("Learn") {
                b.visible = false;
            }
            t
        });
        let rel = skin.find("TrackControls.skin").unwrap_or_else(|| "Patterns/TrackControls.skin".into());
        let mut row_template = skin.load_skin(&rel).ok().and_then(|r| r.children.into_iter().next());
        // The track name area takes clicks (select / right-click menu).
        if let Some(sa) = row_template.as_mut().and_then(|r| r.children.iter_mut().find(|c| c.name == "SelectArea")) {
            sa.overload = "TrackSelectArea".into();
        }
        Ok(PatternPanel {
            page: page.to_vec(),
            grid: root.path_of("PatternGrid"),
            rows: root.path_of("ControlsScrollView"),
            row_template,
            selectors: root.path_of("Selectors"),
            brush: None,
            marker_drag: None,
            brush_value: 1.0,
            erase: None,
            dur_drag: true,
            grid_w: std::cell::Cell::new(626),
            frame_pattern: Default::default(),
            named: Default::default(),
            toolbar: Default::default(),
            graph: root.path_of("PatternGraph"),
            graph_last: None,
            graph_drag_state: GraphDrag::default(),
            graph_target: None,
            lane_menu_fx: false,
            graph_snap: None,
            rng: 0x9e37_79b9,
            selection: Default::default(),
            band: None,
            clipboard: Vec::new(),
            graph_menu: None,
            lane_clip: Vec::new(),
            toolbar_menu: None,
            resources: dreist_formats::paths::skin_dir(),
            swing_menu: None,
            groove: None,
            import_midi_template: false,
            import_guru_swing: false,
            swing_knob_menu: false,
            multi: crate::options::Options::load().string("PatternEditorViewMode", "MultiTrack") != "SingleGraph",
            auto_lanes: Default::default(),
            auto_open: Default::default(),
            auto_template,
            auto_ctx: None,
            graph_drag: None,
            rows_sig: String::new(),
            splitter: {
                // The pattern editor's own splitter (the pads panel has one too).
                let mut host = &*root;
                for &i in page {
                    host = &host.children[i];
                }
                host.path_of("Splitter").map(|p| [page.to_vec(), p].concat())
            },
        })
    }

    /// [re:dc19e2] (multi
    /// tool): NotAllowed on a locked pattern / track, Selection with
    /// shift, ResizeRightThin over the last step of a gated pad's duration
    /// chain (unless it is editor-selected), else Multi.
    pub fn hover_cursor(&self, app: &App, model: &Model, path: &[usize], overload: &str, x: f32, y: f32) -> Option<dreist_gui::cursor::Cursor> {
        use dreist_gui::cursor::Cursor;
        if overload != "PatternGrid" || self.grid.as_deref() != Some(path) {
            return None;
        }
        let lay = self.layout(app, model)?;
        let Some((track, step)) = lay.hit(x, y) else { return Some(Cursor::Arrow) };
        let e = model.engine;
        let cur = model.current_pattern(e);
        let locked = |n: Option<&dreist_formats::Node>| n.is_some_and(|n| n.locked());
        if locked(model.pattern_node(e, cur)) || locked(crate::padmenu::track_node(model, e, track)) {
            return Some(Cursor::image("NotAllowed"));
        }
        if app.mods.shift {
            return Some(Cursor::image("Selection"));
        }
        let gated = !model.pad_one_shot(e, track);
        if !gated {
            return Some(Cursor::image("Multi"));
        }
        let pat = model.pattern(e, cur);
        let on = pat.tracks[track].step(step as u32).on;
        let end = on && last_in_duration_chain(&pat, track, step, true) && !self.selection.contains(&(track, step));
        Some(Cursor::image(if end { "ResizeRightThin" } else { "Multi" }))
    }

    /// [re:65ed38] → scroll to make the step visible
    /// [re:3583e2]: while playing with `PatternEditorFollow`, a current step
    /// outside the visible steps becomes the first one shown. True when
    /// it scrolled.
    pub fn follow(&self, app: &App, model: &mut Model) -> bool {
        let e = model.engine;
        let Some(step) = model.steps[e].map(|s| s as usize) else { return false };
        if !self.grid.as_ref().is_some_and(|g| app.shown(g)) {
            return false;
        }
        let Some(lay) = self.layout(app, model) else { return false };
        if step >= lay.steps || (lay.first_step..lay.first_step + lay.visible_steps).contains(&step) {
            return false;
        }
        model.pattern_view[e].h_scroll = step;
        true
    }

    /// A knob double click's reset value: the graph controls 0.5
    /// ([re:66a130] [re:3199e7]), the
    /// toolbar's swing 0 ([re:587c2e] [re:37b5f9]).
    pub fn reset_value(&self, path: &[usize], name: &str) -> Option<f32> {
        if !path.starts_with(&self.page) {
            return None;
        }
        match name {
            "ScaleKnob" | "CompKnob" | "RandKnob" | "VarianceKnob" => Some(0.5),
            "SwingKnob" => Some(0.0),
            _ => None,
        }
    }

    /// The x of a track's length marker ([re:071acd]
    ///  [re:8811c6]: the left edge of the step at the
    /// track length, past the last step's end for the full length), kept
    /// inside the grid.
    fn marker_x(lay: &GridLayout, len: usize, w: f32) -> f32 {
        let x = if len < lay.steps { lay.step_x(len) } else { lay.step_x(lay.steps) + 1.0 };
        x.round().min(w - MARKER_W).max(0.0)
    }

    /// The track length markers ([re:f32484] ctor: a 10 × 15
    /// button at the top of each track row): a press starts a drag
    /// ([re:82fafd]: clamped to the steps, shift / cmd move every track's
    /// marker), the release snaps to the step under it and posts
    /// `e_setTrackLength` 0xca [e, track, −1, pattern, track, len]
    /// [min(len, steps)] for the track or all 64 ([re:8633da]); a right
    /// press resets it to the pattern length ([re:714b55]).
    #[allow(clippy::too_many_arguments)]
    fn track_marker(&mut self, app: &App, model: &Model, lay: &GridLayout, kind: MouseKind, x: f32, y: f32, out: &mut Vec<Command>) -> bool {
        let e = model.engine;
        let cur = model.current_pattern(e);
        let pat = model.pattern(e, cur);
        let w = self.grid.as_ref().and_then(|g| app.ctrl(g)).map(|c| c.rect.w as f32).unwrap_or(0.0);
        let all = app.mods.shift || app.mods.cmd;
        let send = |track: usize, len: usize, out: &mut Vec<Command>| {
            let tracks: Vec<usize> = if all { (0..64).collect() } else { vec![track] };
            for t in tracks {
                out.push(msg(id::SET_TRACK_LENGTH, &[e as i32, t as i32, -1, cur as i32, t as i32, len as i32], &[len.min(pat.steps as usize).max(1) as f32]));
            }
        };
        if let Some((track, _, a)) = self.marker_drag {
            match kind {
                MouseKind::Drag => {
                    let x = x.clamp(lay.step_x(0), lay.step_x(lay.steps) + 1.0);
                    self.marker_drag = Some((track, x, a));
                }
                MouseKind::Up => {
                    self.marker_drag = None;
                    let len = (0..lay.steps).find(|&s| x >= lay.step_x(s) && x < lay.step_x(s) + lay.step_w).unwrap_or(lay.steps).max(1);
                    send(track, len, out);
                }
                _ => {}
            }
            return true;
        }
        if !matches!(kind, MouseKind::Down | MouseKind::RightDown) {
            return false;
        }
        let Some(row) = (0..lay.tracks).find(|&r| {
            let ry = lay.row_y(r);
            let len = pat.tracks.get(lay.first_track + r).map(|t| t.length as usize).filter(|l| *l > 0).unwrap_or(pat.steps as usize);
            let mx = Self::marker_x(lay, len, w);
            x >= mx && x < mx + MARKER_W && y >= ry && y < ry + lay.row_h.min(MARKER_H)
        }) else {
            return false;
        };
        let track = lay.first_track + row;
        if kind == MouseKind::RightDown {
            send(track, pat.steps as usize, out);
        } else {
            self.marker_drag = Some((track, x, all));
        }
        true
    }

    pub(crate) fn layout(&self, app: &App, model: &Model) -> Option<GridLayout> {
        let g = app.ctrl(self.grid.as_ref()?)?;
        Some(self.layout_for(model, g.rect.w, g.rect.h))
    }

    /// A new frame: the document may have changed since the last one.
    pub fn begin_frame(&self) {
        self.frame_pattern.borrow_mut().take();
    }

    /// The selected engine's current pattern (cached for the frame).
    fn drawn_pattern(&self, model: &Model) -> std::rc::Rc<PatternData> {
        let e = model.engine;
        let cur = model.current_pattern(e);
        let mut cache = self.frame_pattern.borrow_mut();
        match cache.as_ref() {
            Some((ce, cp, pat)) if *ce == e && *cp == cur => pat.clone(),
            _ => {
                let pat = std::rc::Rc::new(model.pattern(e, cur));
                *cache = Some((e, cur, pat.clone()));
                pat
            }
        }
    }

    pub(crate) fn layout_for(&self, model: &Model, w: i32, h: i32) -> GridLayout {
        let e = model.engine;
        self.layout_of(model, &model.pattern(e, model.current_pattern(e)), w, h)
    }

    fn layout_of(&self, model: &Model, pat: &PatternData, w: i32, h: i32) -> GridLayout {
        let e = model.engine;
        if self.multi { GridLayout::compute_multi(model, pat, w, h, &|t| self.lanes(e, t)) } else { GridLayout::compute(model, pat, w, h) }
    }

    /// Lanes shown under track `t` (none unless its automation is open).
    fn lanes(&self, e: usize, t: usize) -> Vec<usize> {
        if !self.auto_open.contains(&(e, t)) {
            return Vec::new();
        }
        self.auto_lanes.get(&(e, t)).cloned().unwrap_or_else(|| vec![0])
    }

    /// Switch MultiTrack (`multi`) / SingleGraph and save the option.
    fn set_view(&mut self, multi: bool, model: &mut Model) {
        self.multi = multi;
        let mut o = crate::options::Options::load();
        o.set("PatternEditorViewMode", if multi { "MultiTrack" } else { "SingleGraph" });
        o.save();
        model.pattern_view[model.engine].v_scroll = 0;
        self.rows_sig.clear();
    }

    /// Layout update [re:2013b6]: the splitter gives the single graph 75 px in
    /// SingleGraph mode and nothing in MultiTrack mode.
    fn apply_view(&self, app: &mut App) {
        let Some(sp) = self.splitter.as_ref().and_then(|p| app.ctrl_mut(p)) else { return };
        let (h, dim) = (sp.rect.h, sp.attr_i32("intSplitterDim", 1));
        let lower = if self.multi { 0 } else { sp.attr_i32("intMaxSize1", 75) };
        sp.set_attr("intSplitPosition", (h - lower - dim).max(0).to_string());
        sp.layout_splitter();
    }

    pub fn sync(&mut self, app: &mut App, model: &Model) {
        layout_toolbar(app, &self.named, &self.toolbar);
        let e = model.engine;
        let cur = model.current_pattern(e);
        let pat = model.pattern(e, cur);
        // Toolbar values.
        let named = &self.named;
        let set = |app: &mut App, name: &'static str, f: &dyn Fn(&mut Ctrl)| {
            if let Some(c) = named.ctrl_mut(app, name) {
                f(c);
            }
        };
        set(app, "LengthBox", &|c| c.text = pat.steps.to_string());
        let lane = lane_label(model.pattern_view[e].lane);
        set(app, "GraphCombo", &|c| c.text = lane.clone());
        let res = RESOLUTION_NAMES.get(pat.resolution).copied().unwrap_or("1/16").to_string();
        set(app, "StepLengthCombo", &|c| c.text = res.clone());
        let name = model.pattern_name(e, cur);
        set(app, "PatternNameBox", &|c| c.text = name.clone());
        let sync = pat.sync;
        set(app, "Gated", &|c| c.on = sync.gated());
        set(app, "Restart", &|c| c.on = sync.restarts());
        let mono = pat.mono;
        set(app, "Remix", &|c| c.on = mono);
        let drag = lane_label(model.pattern_view[e].drag_lane);
        set(app, "DragEdit", &|c| c.text = drag.clone());
        // Engine controls: tempo modifier and swing depth.
        let en = model.engine_node(e);
        let num = en.and_then(|n| n.u32("TempoNumerator")).unwrap_or(1).clamp(1, 16);
        let den = en.and_then(|n| n.u32("TempoDenominator")).unwrap_or(1).clamp(1, 16);
        let depth = en.and_then(|n| n.f32("SwingTemplateDepth")).unwrap_or(0.0);
        if !app.is_editing() {
            set(app, "TempoNum", &|c| {
                c.text = num.to_string();
                c.value = num as f32;
            });
            set(app, "TempoDenom", &|c| {
                c.text = den.to_string();
                c.value = den as f32;
            });
        }
        set(app, "SwingKnob", &|c| c.value = depth);
        // Pattern selector keys.
        if let Some(sp) = &self.selectors {
            sync_selectors(app, model, sp);
        }
        // Track rows.
        let Some(g) = self.grid.as_ref().and_then(|g| app.ctrl(g)) else { return };
        let lay = self.layout_of(model, &pat, g.rect.w, g.rect.h);
        // Scroll bars: visible fraction and position.
        let vfrac = lay.tracks as f32 / lay.total_tracks as f32;
        let vpos = if lay.total_tracks > lay.tracks { (lay.first_track - model.visible_pads().0) as f32 / (lay.total_tracks - lay.tracks) as f32 } else { 0.0 };
        let hfrac = lay.visible_steps as f32 / lay.steps as f32;
        let hpos = if lay.steps > lay.visible_steps { lay.first_step as f32 / (lay.steps - lay.visible_steps) as f32 } else { 0.0 };
        set(app, "VScrollBar", &|c| {
            c.set_attr("realPage", vfrac.to_string());
            c.value = vpos;
        });
        set(app, "HScrollBar", &|c| {
            c.set_attr("realPage", hfrac.to_string());
            c.value = hpos;
        });
        if let Some(g) = self.grid.as_ref().and_then(|g| app.ctrl(g)) {
            self.grid_w.set(g.rect.w);
        }
        let selected = model.pad[e];
        let names: Vec<String> = (0..lay.tracks).map(|r| model.pad_name(e, lay.first_track + r).unwrap_or_else(|| format!("Pad {}", lay.first_track + r + 1))).collect();
        let gains: Vec<f32> = (0..lay.tracks).map(|r| model.pad_gain(e, lay.first_track + r)).collect();
        let multi = self.multi;
        set(app, "View", &|c| c.on = !multi);
        self.apply_view(app);
        let sig = format!("{:?}{:?}{}", lay.track_y, lay.graphs, lay.row_h);
        let open: Vec<bool> = (0..lay.tracks).map(|r| self.auto_open.contains(&(e, lay.first_track + r))).collect();
        let (Some(rp), Some(t)) = (self.rows.clone(), self.row_template.clone()) else { return };
        let auto_t = self.auto_template.clone();
        let Some(rows) = app.ctrl_mut(&rp) else { return };
        let w = rows.rect.w;
        if self.rows_sig != sig || rows.children.is_empty() {
            self.rows_sig = sig;
            rows.children.clear();
            for r in 0..lay.tracks {
                let mut row = t.clone();
                row.set_rect(Rect::new(0, lay.row_y(r) as i32, w, lay.row_h as i32));
                row.set_attr("trackRow", r.to_string());
                rows.children.push(row);
            }
            // MultiTrack lane controls ([re:bc7b1d]).
            if let Some(at) = &auto_t {
                for g in &lay.graphs {
                    let mut c = at.clone();
                    c.set_rect(Rect::new(0, g.y as i32, w, g.h as i32));
                    c.set_attr("autoTrack", g.track.to_string());
                    c.set_attr("autoSlot", g.slot.to_string());
                    rows.children.push(c);
                }
            }
        }
        for row in rows.children.iter_mut() {
            if row.attr("autoTrack").is_some() {
                let lane =
                    lay.graphs.iter().find(|g| Some(g.track.to_string().as_str()) == row.attr("autoTrack") && Some(g.slot.to_string().as_str()) == row.attr("autoSlot")).map(|g| g.lane).unwrap_or(0);
                let sel_track = row.attr("autoTrack").and_then(|v| v.parse::<usize>().ok()) == Some(selected);
                if let Some(c) = row.find_mut("GraphCombo") {
                    c.text = lane_label(lane);
                }
                if let Some(c) = row.find_mut("HighlightPanel") {
                    c.style = if sel_track { "TrackSelectedStyle" } else { "TrackNotSelectedStyle" }.into();
                }
                continue;
            }
            let Some(r) = row.attr("trackRow").and_then(|v| v.parse::<usize>().ok()) else { continue };
            let track = lay.first_track + r;
            let is_open = open.get(r).copied().unwrap_or(false);
            if let Some(b) = row.find_mut("AutomateButton") {
                b.on = is_open;
                b.visible = multi;
            }
            let td = pat.tracks.get(track);
            row.set_attr("track", track.to_string());
            row.style = if track == selected { "TrackSelectedStyle" } else { "TrackNotSelectedStyle" }.into();
            for c in row.children.iter_mut() {
                match c.name.as_str() {
                    "SelectArea" => c.text = names[r].clone(),
                    "MuteButton" => c.on = td.is_some_and(|t| t.mute),
                    "SoloButton" => c.on = td.is_some_and(|t| t.solo),
                    "PadLevel" => c.value = gains[r],
                    _ => {}
                }
            }
        }
    }

    /// The graph's tooltip at pointer x (graph coordinates)
    /// ([re:407d1e], placed by
    /// [re:a4325a] above the graph at the
    /// pointer): "<lane>: step <n> to <value>".
    pub fn graph_tooltip(&self, app: &App, model: &Model, x: f32) -> Option<String> {
        let lay = self.layout(app, model)?;
        let e = model.engine;
        let step = lay.first_step + (x.max(0.0) / lay.step_w) as usize;
        if step >= lay.steps {
            return None;
        }
        let (track, lane) = (model.pad[e], model.pattern_view[e].lane);
        let pat = model.pattern(e, model.current_pattern(e));
        let f = pat.tracks.get(track).and_then(|t| t.steps.get(step)).and_then(|s| s.lanes.get(lane).copied()).unwrap_or_else(|| dreist_formats::params::step_lane_default(lane));
        let name = crate::transmod::camel_words(dreist_formats::params::STEP_LANES.get(lane)?);
        Some(format!("{name}: step {} to {}", step + 1, graph_value_text(model, track, lane, f)))
    }

    /// Returns true when the model changed.
    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        if let Some(r) = self.auto_event(app, model, ev) {
            return r;
        }
        let e = model.engine;
        let cur = model.current_pattern(e) as i32;
        match ev {
            UiEvent::Mouse { path, overload, kind, x, y } if overload == "PatternGraph" => {
                let (Some(gp), Some(lay)) = (self.graph.clone(), self.layout(app, model)) else { return false };
                let Some(h) = app.ctrl(&gp).map(|c| c.rect.h as f32) else { return false };
                let (track, lane) = (model.pad[e], model.pattern_view[e].lane);
                self.graph_edit(app, model, path, *kind, *x, *y, h, track, lane, &lay, out)
            }
            // [re:ca3223] [re:c0f351]: a track
            // row's level knob posts `e_setPadParameter` 0x4e [engine,
            // track] [value, Gain, 0] (the drag keeps an undo point).
            UiEvent::ValueChanged { path, name, value, .. } if name == "PadLevel" && self.rows.as_ref().is_some_and(|r| path.starts_with(r)) => {
                let track = (1..path.len()).rev().find_map(|n| app.ctrl(&path[..n]).and_then(|c| c.attr("track")).and_then(|t| t.parse::<i32>().ok()));
                match track {
                    Some(t) => {
                        out.push(msg(id::SET_PAD_PARAMETER, &[e as i32, t], &[*value, dreist_formats::PadParam::Gain as u8 as f32, 0.0]));
                        true
                    }
                    None => false,
                }
            }
            // Scale [re:23318b] / compress [re:f1235c] /
            // randomise [re:2a1033]: applied to the lane values copied
            // when the knob started moving, posted as 0xd6 per step.
            UiEvent::ValueChanged { name, value, .. } if matches!(name.as_str(), "ScaleKnob" | "CompKnob" | "RandKnob" | "VarianceKnob") => {
                let track = model.pad[e];
                let lane = model.pattern_view[e].lane;
                if name == "VarianceKnob" {
                    out.push(msg(id::SET_GRAPH_RANDOMISATION, &[e as i32, track as i32, -1, cur, track as i32], &[*value, lane as f32]));
                    return true;
                }
                let pat = model.pattern(e, cur as usize);
                let Some(t) = pat.tracks.get(track) else { return false };
                if self.graph_snap.as_ref().map(|s| s.0.as_str()) != Some(name.as_str()) {
                    let vals = t.steps.iter().take(pat.steps as usize).map(|s| s.lanes.get(lane).copied().unwrap_or(0.0)).collect();
                    self.graph_snap = Some((name.clone(), vals, 0.0));
                }
                let target = [1.0f32, 0.5, 1.0, 1.0, 0.5][dreist_engine::pattern::lane_style(lane).min(4) as usize];
                let mut rng = self.rng;
                let (_, snap, prev) = self.graph_snap.as_mut().unwrap();
                let off = value * 2.0 - 1.0;
                for (st, base) in snap.iter_mut().enumerate() {
                    let v = match name.as_str() {
                        "ScaleKnob" => {
                            *base += off - *prev;
                            *base
                        }
                        "CompKnob" => compressed(*base, *value, target),
                        _ => {
                            rng ^= rng << 13;
                            rng ^= rng >> 17;
                            rng ^= rng << 5;
                            randomised(*base, *value, (rng >> 8) as f32 / (1u32 << 24) as f32)
                        }
                    };
                    out.push(msg(id::SET_STEP_GRAPH_STATE, &[e as i32, track as i32, -1, cur, track as i32, st as i32], &[lane as f32, v.clamp(0.0, 1.0), 0.0]));
                }
                if name == "ScaleKnob" {
                    *prev = off;
                }
                self.rng = rng;
                true
            }
            UiEvent::Clicked { path, name, .. }
                if name == "OptionsButton" && (app.root.path_of("SingleAutoControl").is_some_and(|p| path.starts_with(&p)) || self.auto_slot_of(app, path).is_some()) =>
            {
                self.graph_menu = Some(false);
                self.graph_target = None;
                app.open_menu(path, GRAPH_MENU.iter().map(|s| s.to_string()).collect(), None);
                true
            }
            // Tempo modifier (`e_setEngineTempoNum` 0x93 / `Denom` 0x94, 1..16).
            UiEvent::TextCommitted { name, text, .. } if name == "TempoNum" || name == "TempoDenom" => {
                let Ok(v) = text.trim().parse::<i32>() else { return true };
                let k = if name == "TempoNum" { id::SET_ENGINE_TEMPO_NUM } else { id::SET_ENGINE_TEMPO_DENOM };
                out.push(msg(k, &[e as i32], &[v.clamp(1, 16) as f32]));
                true
            }
            UiEvent::ValueChanged { name, value, .. } if name == "TempoNum" || name == "TempoDenom" => {
                let k = if name == "TempoNum" { id::SET_ENGINE_TEMPO_NUM } else { id::SET_ENGINE_TEMPO_DENOM };
                out.push(msg(k, &[e as i32], &[value.round().clamp(1.0, 16.0)]));
                true
            }
            // `e_setEngineSwingTemplateDepth` 0x95.
            // Graph Rec (`InstoGraph`, toolbar lambda [re:a6cf3a] →
            // `e_setInstoGraphValue` 0xd8; the pattern message handler: with the
            // gate on and record armed the value goes into the selected
            // pad's graph lane at the current step and is queued for the
            // next step). Port: a step graph message at the engine's current
            // step plus 0xd8 (with the lane) for the engine's queue.
            UiEvent::ValueChanged { name, value, .. } if name == "InstoGraph" => {
                let e = model.engine;
                let armed = app.root.path_of("RecButton").and_then(|p| app.ctrl(&p)).is_some_and(|c| c.on);
                if let (true, true, Some(step)) = (model.playing, armed, model.steps[e]) {
                    let (track, lane, cur) = (model.pad[e], model.pattern_view[e].lane, model.current_pattern(e) as i32);
                    out.push(msg(id::SET_STEP_GRAPH_STATE, &[e as i32, track as i32, -1, cur, track as i32, step as i32], &[lane as f32, value.clamp(0.0, 1.0), 0.0]));
                    out.push(msg(id::SET_INSTO_GRAPH_VALUE, &[e as i32, track as i32, -1, cur], &[value.clamp(0.0, 1.0), lane as f32]));
                }
                true
            }
            UiEvent::ValueChanged { name, value, .. } if name == "SwingKnob" => {
                out.push(msg(id::SET_ENGINE_SWING_TEMPLATE_DEPTH, &[e as i32], &[value.clamp(0.0, 1.0)]));
                true
            }
            // Swing template menu ([re:587c2e]
            // [re:05f028]): Extract timing from current pattern, Import MIDI
            // template, Import Guru shuffle, divider, the templates
            // (swing template paths, current checked).
            UiEvent::RightClicked { path, name } if name == "SwingKnob" => {
                self.swing_knob_menu = true;
                self.handle(app, model, &UiEvent::Clicked { path: path.clone(), name: "SwingDrop".into(), id: 0, on: false }, out)
            }
            UiEvent::TextCommitted { path, name, text } if name == SWING_EDIT => {
                let parent = path[..path.len() - 1].to_vec();
                if let Some(c) = app.ctrl_mut(&parent) {
                    c.children.retain(|k| k.name != SWING_EDIT);
                }
                if let Some(v) = crate::conv::number(text.trim().trim_end_matches('%')) {
                    let v = if text.contains('%') || v > 1.0 { v / 100.0 } else { v };
                    out.push(msg(id::SET_ENGINE_SWING_TEMPLATE_DEPTH, &[e as i32], &[v.clamp(0.0, 1.0)]));
                }
                true
            }
            UiEvent::Clicked { path, name, .. } if name == "SwingDrop" => {
                let paths = crate::swing::template_paths(&self.resources);
                let current = model.engine_node(e).and_then(|n| n.children.iter().find(|c| c.class == "SwingTemplate")).map(|n| crate::swing::name_of(Some(n), std::path::Path::new("")));
                let names: Vec<String> = paths.iter().map(|p| crate::swing::name_of(crate::swing::load(p).as_ref(), p)).collect();
                let checked = names.iter().position(|l| Some(l) == current.as_ref()).map(|i| i + SWING_HEAD);
                let mut labels = vec!["Extract timing from current pattern".to_string(), "Import MIDI template".into(), "Import Guru shuffle".into(), String::new()];
                labels.extend(names);
                if self.swing_knob_menu {
                    labels.extend([String::new(), "Reset".into(), "Edit".into()]);
                }
                self.swing_menu = Some(paths);
                app.open_menu(path, labels, checked);
                false
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.swing_menu.is_some() => {
                let paths = self.swing_menu.take().unwrap();
                let knob = std::mem::take(&mut self.swing_knob_menu);
                if knob && *index >= SWING_HEAD + paths.len() {
                    match *index - SWING_HEAD - paths.len() {
                        1 => out.push(msg(id::SET_ENGINE_SWING_TEMPLATE_DEPTH, &[e as i32], &[0.0])),
                        2 => {
                            // [re:c8b896]: a text box over the knob.
                            let Some(knob) = app.root.path_of("SwingKnob").and_then(|p| app.ctrl(&p).cloned().map(|c| (p, c))) else { return false };
                            let (kp, kc) = knob;
                            let rect = dreist_gui::Rect::new(kc.rect.x - 10, kc.rect.y + kc.rect.h / 2 - 9, kc.rect.w + 20, 18);
                            app.open_edit_box(&kp[..kp.len() - 1], SWING_EDIT, rect, format!("{:.0}%", kc.value * 100.0));
                        }
                        _ => return false,
                    }
                    let _ = anchor;
                    return true;
                }
                match *index {
                    0 => {
                        self.groove = Some(cur as usize);
                        return true;
                    }
                    1 => {
                        self.import_midi_template = true;
                        return true;
                    }
                    2 => {
                        self.import_guru_swing = true;
                        return true;
                    }
                    3 => return false,
                    _ => {}
                }
                let Some(p) = paths.get(*index - SWING_HEAD) else { return false };
                let Some(n) = crate::swing::load(p) else { return false };
                let sw = dreist_engine::pattern::Swing::from_node(&n);
                out.push(Command::SwingTemplate(e, Box::new(n), Box::new(sw)));
                true
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.toolbar_menu.is_some() => {
                let actions = self.toolbar_menu.take().unwrap();
                let Some(&a) = actions.get(*index) else { return false };
                self.toolbar_action(app, model, anchor, a, out)
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.graph_menu.is_some() => {
                let insert = self.graph_menu.take().unwrap();
                let (track, lane, start) = self.graph_target.unwrap_or((model.pad[e], model.pattern_view[e].lane, 0));
                let pat = model.pattern(e, cur as usize);
                let n = pat.steps as usize;
                let Some(t) = pat.tracks.get(track) else { return false };
                let vals: Vec<f32> = t.steps.iter().take(n).map(|s| s.lanes.get(lane).copied().unwrap_or(0.0)).collect();
                let send_all = |out: &mut Vec<Command>, v: &[f32]| {
                    for (st, x) in v.iter().enumerate() {
                        out.push(msg(id::SET_STEP_GRAPH_STATE, &[e as i32, track as i32, -1, cur, track as i32, st as i32], &[lane as f32, x.clamp(0.0, 1.0), 0.0]));
                    }
                };
                if insert {
                    self.graph_target = None;
                    let mut rng = self.rng;
                    let shape = insert_from(*index, n, start, &mut || {
                        rng ^= rng << 13;
                        rng ^= rng >> 17;
                        rng ^= rng << 5;
                        (rng >> 8) as f32 / (1u32 << 24) as f32
                    });
                    self.rng = rng;
                    for (st, v) in shape {
                        out.push(msg(id::SET_STEP_GRAPH_STATE, &[e as i32, track as i32, -1, cur, track as i32, st as i32], &[lane as f32, v, 0.0]));
                    }
                    return true;
                }
                let idx = [e as i32, -1, -1, cur, track as i32];
                match *index {
                    0 => send_all(out, &vec![dreist_formats::params::step_lane_default(lane); n]),
                    2 | 3 => {
                        self.lane_clip = vals.clone();
                        if *index == 2 {
                            out.push(msg(id::CLEAR_TRACK_AUTOMATION, &idx, &[lane as f32]));
                        }
                    }
                    4 => {
                        let clip = self.lane_clip.clone();
                        send_all(out, &clip[..clip.len().min(n)]);
                    }
                    6 => out.push(msg(id::CLEAR_TRACK_AUTOMATION, &idx, &[lane as f32])),
                    7 => out.push(msg(id::CLEAR_ALL_TRACK_AUTOMATION, &idx, &[0.0])),
                    8 => out.push(msg(id::CLEAR_ALL_PATTERN_AUTOMATION, &idx[..4], &[0.0])),
                    9 => send_all(out, &vals.iter().map(|v| 1.0 - v).collect::<Vec<_>>()),
                    10 => send_all(out, &vals.iter().rev().copied().collect::<Vec<_>>()),
                    12 => {
                        self.graph_menu = Some(true);
                        app.open_menu(anchor, INSERTABLE.iter().map(|s| s.to_string()).collect(), None);
                        return false;
                    }
                    _ => {}
                }
                self.graph_target = None;
                true
            }
            UiEvent::MenuSelected { anchor, name, index } if name == "GraphCombo" => {
                if !self.lane_menu_fx && *index == CORE_LANES {
                    self.lane_menu_fx = true;
                    let items = (CORE_LANES..dreist_engine::pattern::LANES).map(lane_label).collect();
                    app.open_menu(anchor, items, None);
                    return false;
                }
                let lane = if self.lane_menu_fx { CORE_LANES + index } else { *index };
                model.pattern_view[e].lane = lane;
                self.lane_menu_fx = false;
                // A MultiTrack lane row's combo changes that lane.
                if let Some((t, slot)) = self.auto_ctx.take() {
                    if let Some(l) = self.auto_lanes.get_mut(&(e, t)).and_then(|v| v.get_mut(slot)) {
                        *l = lane;
                    }
                }
                true
            }
            UiEvent::Shortcut { key, cmd, .. } if model.editor_tab == 0 => self.shortcut(model, key, *cmd, out),
            UiEvent::Wheel { overload, dy, .. } if overload == "PatternGrid" => {
                let v = &mut model.pattern_view[e];
                v.v_scroll = (v.v_scroll as i32 - dy.signum() as i32).max(0) as usize;
                // Clamp through the layout.
                if let Some(lay) = self.layout(app, model) {
                    model.pattern_view[e].v_scroll = lay.first_track - model.visible_pads().0;
                }
                true
            }
            UiEvent::ValueChanged { name, value, .. } if name == "VScrollBar" || name == "HScrollBar" => {
                let Some(lay) = self.layout(app, model) else { return false };
                let v = &mut model.pattern_view[e];
                if name == "VScrollBar" {
                    v.v_scroll = (value * (lay.total_tracks - lay.tracks) as f32).round() as usize;
                } else {
                    v.h_scroll = (value * (lay.steps - lay.visible_steps) as f32).round() as usize;
                }
                true
            }
            UiEvent::Mouse { path, overload, kind, x, y } if overload == "PatternGrid" => {
                let Some(lay) = self.layout(app, model) else { return false };
                if self.track_marker(app, model, &lay, *kind, *x, *y, out) {
                    return true;
                }
                // MultiTrack graph rows.
                if matches!(kind, MouseKind::Down | MouseKind::RightDown) {
                    self.graph_drag = lay.graph_at(*y).map(|g| (g.track, g.lane, g.y, g.h));
                }
                if let Some((t, l, gy, gh)) = self.graph_drag {
                    if *kind == MouseKind::Up {
                        self.graph_drag = None;
                    }
                    return self.graph_edit(app, model, path, *kind, *x, *y - gy, gh, t, l, &lay, out);
                }
                let pat = model.pattern(e, cur as usize);
                let Some((track, step)) = lay.hit(*x, *y) else {
                    if *kind == MouseKind::Up {
                        self.brush = None;
                        self.band = None;
                    }
                    return false;
                };
                // Shift-drag: rubber-band selection of steps that are on.
                if *kind == MouseKind::Down && app.mods.shift {
                    if !app.mods.cmd {
                        self.selection.clear();
                    }
                    self.band = Some(((track, step), (track, step)));
                    self.update_band(&pat);
                    return true;
                }
                if let Some((a, _)) = self.band {
                    match kind {
                        MouseKind::Drag => {
                            self.band = Some((a, (track, step)));
                            self.update_band(&pat);
                        }
                        MouseKind::Up => self.band = None,
                        _ => {}
                    }
                    return true;
                }
                let on_now = pat.tracks.get(track).and_then(|t| t.steps.get(step)).is_some_and(|s| s.on);
                let drag_lane = model.pattern_view[e].drag_lane;
                // [re:f32484] multi tool (tool 3).
                let ctx = StepCtx { e, cur, drag_lane, pat: &pat, lay: &lay };
                match kind {
                    // Left press [re:12e437] / multi-tool press [re:e6464c]:
                    // with editor-selected steps a click elsewhere only clears
                    // the selection (0xc8); otherwise a brush press [re:bf6801].
                    MouseKind::Down => {
                        self.erase = None;
                        let selected_on = on_now && self.selection.contains(&(track, step));
                        if !self.selection.is_empty() && !selected_on {
                            self.selection.clear();
                            return true;
                        }
                        self.selection.clear();
                        model.pad[e] = track;
                        let gated = !model.pad_one_shot(e, track);
                        let mut b = Brush { track, last: step, min: step, max: step, gated, dur_drag: false, last_y: *y };
                        if !on_now {
                            let dur = if gated && app.mods.alt { ((*x - lay.step_x(step)) / lay.step_w.max(1.0)).clamp(0.0, 1.0) } else { 1.0 };
                            if drag_lane != 0 {
                                self.brush_value = pat.tracks[track].steps[step].lanes[drag_lane];
                            }
                            ctx.note_on(out, track, step, self.brush_value, dur, false);
                        } else if gated {
                            self.dur_drag = last_in_duration_chain(&pat, track, step, gated);
                        }
                        // The chain-end flag is only set by a press on an on step of a
                        // gated pad and otherwise keeps its value (initially
                        // set), so drawing from an off step makes a chain.
                        b.dur_drag = self.dur_drag;
                        self.brush = Some(b);
                        true
                    }
                    MouseKind::Drag if self.erase.is_some() => {
                        let (t, last) = self.erase.unwrap();
                        for k in last.min(step)..=last.max(step) {
                            ctx.note_off(out, track, k, true);
                        }
                        let _ = t;
                        self.erase = Some((track, step));
                        true
                    }
                    MouseKind::Drag => {
                        let Some(mut b) = self.brush else { return false };
                        if b.track != track {
                            return false;
                        }
                        let i = step;
                        let (v, alt) = (self.brush_value, app.mods.alt);
                        if !b.gated {
                            // Brush move [re:04db30], one-shot pads: every
                            // step passed turns on; the vertical drag sets the
                            // Drag Edits value of the brushed range.
                            if i > b.max {
                                b.max = i;
                            } else if i < b.min {
                                b.min = i;
                            }
                            if i != b.last {
                                let (a, z) = (b.last.min(i), b.last.max(i).min(pat.steps as usize - 1));
                                for k in a..=z {
                                    ctx.note_on(out, track, k, v, 1.0, false);
                                }
                                b.last = i;
                            }
                            let fade = alt && b.min != b.max;
                            self.brush_value = ctx.set_velocity(out, track, *x, *y, &mut b, i, fade, v);
                        } else {
                            // Gated pads: drags make duration chains (tied
                            // steps), dragging back releases them; alt makes
                            // the step under the pointer a partial one.
                            let mut fill_to = i;
                            let mut fill = false;
                            if i > b.max {
                                b.max = i;
                            } else if i < b.min {
                                b.min = i;
                            }
                            let mut from = b.last;
                            if b.dur_drag {
                                if b.last <= i {
                                    fill = b.last <= b.max;
                                } else {
                                    fill_to = b.last;
                                    for k in b.last..=b.max {
                                        ctx.note_off(out, track, k, false);
                                    }
                                    b.last = i;
                                    from = i;
                                    fill = i <= b.max;
                                }
                            } else if i < b.last {
                                fill_to = b.last;
                                b.last = i;
                            }
                            if fill {
                                for k in from..=b.max {
                                    if k > fill_to {
                                        ctx.note_off(out, track, k, false);
                                    } else {
                                        let dur = if alt && k == fill_to { ((*x - lay.step_x(k)) / lay.step_w.max(1.0)).clamp(0.0, 1.0) } else { 1.0 };
                                        ctx.note_on(out, track, k, v, dur, b.last < k);
                                    }
                                }
                            }
                            let (lo, hi) = (b.last, b.max);
                            self.brush_value = ctx.set_velocity_no_fade(out, track, *y, &mut b, lo, hi, v);
                        }
                        self.brush = Some(b);
                        true
                    }
                    // Multi-tool press (right button): the graph menu with
                    // option (or without it in the swapped right-click mode),
                    // else a delete move [re:a5603b] — an editor-selected step
                    // clears the selection's steps (0xb1), otherwise steps go
                    // off from the last erased one to here.
                    MouseKind::RightDown => {
                        if on_now && self.selection.contains(&(track, step)) {
                            // The selection is editor-side: its steps go off.
                            for (t, k) in std::mem::take(&mut self.selection) {
                                ctx.note_off(out, t, k, true);
                            }
                            return true;
                        }
                        self.erase = Some((track, step));
                        ctx.note_off(out, track, step, true);
                        true
                    }
                    MouseKind::Up => {
                        self.brush = None;
                        self.erase = None;
                        false
                    }
                }
            }
            UiEvent::Clicked { path, name, on, .. } if path.starts_with(&self.page) => {
                if let Some(n) = name.strip_prefix("Key").and_then(|n| n.parse::<i32>().ok()) {
                    if self.selectors.as_ref().is_some_and(|s| path.starts_with(s)) {
                        out.push(msg(id::SELECT_PATTERN, &[e as i32, -1, -1, n], &[0.0]));
                        set_current_pattern(model, e, n as usize);
                        return true;
                    }
                }
                let pat = model.pattern(e, cur as usize);
                let track = model.pad[e] as i32;
                match name.as_str() {
                    "GraphCombo" => {
                        let mut items: Vec<String> = (0..CORE_LANES).map(lane_label).collect();
                        items.push("FX ▸".into());
                        self.lane_menu_fx = false;
                        app.open_menu(path, items, Some(model.pattern_view[e].lane.min(CORE_LANES)));
                        false
                    }
                    // Toolbar View ("Show a Single Global Graph", lambda
                    // [re:ec3067]): writes `PatternEditorViewMode`.
                    "View" if path.len() > 1 => {
                        self.set_view(!on, model);
                        true
                    }
                    "ZoomInH" | "ZoomOutH" => {
                        let v = &mut model.pattern_view[e];
                        v.h_zoom = if name == "ZoomInH" { (v.h_zoom * 2).min(16) } else { (v.h_zoom / 2).max(1) };
                        true
                    }
                    "ZoomInV" | "ZoomOutV" => {
                        let v = &mut model.pattern_view[e];
                        v.v_zoom = if name == "ZoomInV" { (v.v_zoom * 1.5).min(4.0) } else { (v.v_zoom / 1.5).max(1.0) };
                        true
                    }
                    "NudgeTrackBackButton" | "NudgeTrackForwardButton" => {
                        let k = if name == "NudgeTrackForwardButton" { id::NUDGE_TRACK_FORWARD } else { id::NUDGE_TRACK_BACKWARDS };
                        out.push(msg(k, &[e as i32, -1, -1, cur, track], &[0.0]));
                        true
                    }
                    "NudgePatternBackButton" | "NudgePatternForwardButton" => {
                        let k = if name == "NudgePatternForwardButton" { id::NUDGE_PATTERN_FORWARD } else { id::NUDGE_PATTERN_BACKWARDS };
                        out.push(msg(k, &[e as i32, -1, -1, cur], &[0.0]));
                        true
                    }
                    "NextTrackButton" | "PreviousTrackButton" => {
                        let p = model.pad[e] as i32 + if name == "NextTrackButton" { 1 } else { -1 };
                        model.pad[e] = p.clamp(0, 63) as usize;
                        true
                    }
                    "PlusButton" | "MinusButton" => {
                        let n = if name == "PlusButton" { pat.steps + 1 } else { pat.steps.saturating_sub(1).max(1) };
                        out.push(msg(id::SET_PATTERN_STEP_COUNT, &[e as i32, -1, -1, cur], &[n as f32]));
                        true
                    }
                    "Gated" => self.toolbar_action(app, model, path, ToolbarAction::Gated, out),
                    "Restart" => self.toolbar_action(app, model, path, ToolbarAction::Restart, out),
                    "QuantizeButton" => self.toolbar_action(app, model, path, ToolbarAction::Quantize, out),
                    "RepeatButton" => self.toolbar_action(app, model, path, ToolbarAction::Duplicate, out),
                    "Remix" => self.toolbar_action(app, model, path, ToolbarAction::Remix, out),
                    "DragEdit" => self.toolbar_action(app, model, path, ToolbarAction::DragEdits, out),
                    "CopyToNext" => {
                        if copy_to_next(model, e).is_some() {
                            self.selection.clear();
                            out.push(model.rebuild_command());
                        } else {
                            self.toolbar_menu = Some(vec![ToolbarAction::None]);
                            app.open_menu(path, vec!["No available empty patterns could be found".into()], None);
                        }
                        true
                    }
                    // [re:45e9a3] [re:d7bdef].
                    "OptionsButton" => {
                        let check = |on: bool, t: &str| format!("{}{t}", if on { "✓ " } else { "" });
                        let mut items = vec![
                            ("Drag Edits... ▸".to_string(), ToolbarAction::DragEdits),
                            (String::new(), ToolbarAction::None),
                            (check(!self.multi, "Single Track View"), ToolbarAction::SingleTrack),
                        ];
                        let shown = |n: &str| app.root.path_of(n).and_then(|p| app.ctrl(&p[..p.len() - 1]).map(|c| c.visible)).unwrap_or(true);
                        if !shown("QuantizeButton") {
                            items.push((String::new(), ToolbarAction::None));
                            items.push(("Quantize".into(), ToolbarAction::Quantize));
                            items.push(("Duplicate".into(), ToolbarAction::Duplicate));
                            items.push((check(pat.mono, "Remix"), ToolbarAction::Remix));
                        }
                        items.push((String::new(), ToolbarAction::None));
                        items.push((check(pat.sync.gated(), "Gated"), ToolbarAction::Gated));
                        items.push((check(pat.sync.restarts(), "Restart"), ToolbarAction::Restart));
                        let (labels, actions): (Vec<String>, Vec<ToolbarAction>) = items.into_iter().unzip();
                        self.toolbar_menu = Some(actions);
                        app.open_menu(path, labels, None);
                        false
                    }
                    "MuteButton" | "SoloButton" => {
                        let Some(rp) = &self.rows else { return false };
                        if !path.starts_with(rp) || path.len() < rp.len() + 2 {
                            return false;
                        }
                        let Some(track) = app.ctrl(&path[..rp.len() + 1]).and_then(|r| r.attr("track")).and_then(|v| v.parse::<i32>().ok()) else { return false };
                        let kind = if name == "MuteButton" { id::SET_TRACK_MUTE_STATUS } else { id::SET_TRACK_SOLO_STATUS };
                        out.push(msg(kind, &[e as i32, -1, -1, cur, track], &[*on as i32 as f32]));
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// [re:d47fd5] / mouse move: value from
    /// the height (on/off lanes: upper half on); dragging fills the steps
    /// passed; the right button restores the lane default. `y` is relative
    /// to the graph, `h` its height.
    #[allow(clippy::too_many_arguments)]
    /// [re:e9d6e3] mouse handling (left press [re:fc66b7],
    /// move [re:6b8705], right press [re:235fb8],
    /// delete tool [re:8ed66a]) with the default tool: a click sets the
    /// step from y (switch lanes: top half on); dragging fills every step
    /// passed with the value, shift adjusts it by −0.001 per pixel from
    /// where it was; option-drag draws a line from the clicked step and
    /// value. Right button erases (dragging resets steps to the default)
    /// or, with option, opens the graph menu at the step — swapped when
    /// the `PatternEditorRightClickMode` option is on.
    #[allow(clippy::too_many_arguments)]
    fn graph_edit(&mut self, app: &mut App, model: &Model, path: &[usize], kind: MouseKind, x: f32, y: f32, h: f32, track: usize, lane: usize, lay: &GridLayout, out: &mut Vec<Command>) -> bool {
        let start = out.len();
        let r = self.graph_edit_inner(app, model, path, kind, x, y, h, track, lane, lay, out);
        self.selection_offsets(model, out, start);
        r
    }

    /// Pattern message 0xd6 with the edit flag (value 2) on a step
    /// that is editor-selected: every selected step of the
    /// pattern moves by the same delta ([re:090253]
    /// [re:57983c] → [re:d4c180], clamped
    /// 0..1, no linkage; unlocked tracks only). The selection lives in the
    /// editor here, so the editor rewrites its own messages from `start`.
    fn selection_offsets(&self, model: &Model, out: &mut Vec<Command>, start: usize) {
        if self.selection.is_empty() || out.len() <= start {
            return;
        }
        let e = model.engine;
        let cur = model.current_pattern(e);
        let pat = model.pattern(e, cur);
        let mut vals: std::collections::BTreeMap<(usize, usize, usize), f32> = Default::default();
        let get = |vals: &std::collections::BTreeMap<(usize, usize, usize), f32>, t: usize, s: usize, l: usize| {
            vals.get(&(t, s, l)).copied().or_else(|| pat.tracks.get(t).and_then(|tr| tr.steps.get(s)).and_then(|st| st.lanes.get(l).copied())).unwrap_or(0.0)
        };
        let tail: Vec<Command> = out.drain(start..).collect();
        for c in tail {
            let Command::Message(m) = &c else {
                out.push(c);
                continue;
            };
            let (t, s, l) = (m.index[1].max(0) as usize, m.index[5].max(0) as usize, m.value[0].max(0.0) as usize);
            if m.kind != id::SET_STEP_GRAPH_STATE || m.value[2] < 0.5 || m.index[3] != cur as i32 || !self.selection.contains(&(t, s)) {
                out.push(c);
                continue;
            }
            let delta = m.value[1] - get(&vals, t, s, l);
            for &(qt, qs) in &self.selection {
                let locked = model
                    .engine_node(e)
                    .and_then(|n| n.children.iter().find(|c| c.class == "StepPattern" && c.name.as_deref() == Some(&format!("StepPattern{cur}"))))
                    .and_then(|p| p.children.iter().find(|c| c.class == "Track" && c.name.as_deref() == Some(&format!("Track{qt}"))))
                    .is_some_and(|t| t.locked());
                if locked {
                    continue;
                }
                let v = (get(&vals, qt, qs, l) + delta).clamp(0.0, 1.0);
                vals.insert((qt, qs, l), v);
                out.push(msg(id::SET_STEP_GRAPH_STATE, &[e as i32, qt as i32, -1, cur as i32, qt as i32, qs as i32], &[l as f32, v, 0.0]));
            }
        }
    }

    fn graph_edit_inner(&mut self, app: &mut App, model: &Model, path: &[usize], kind: MouseKind, x: f32, y: f32, h: f32, track: usize, lane: usize, lay: &GridLayout, out: &mut Vec<Command>) -> bool {
        let e = model.engine;
        let cur = model.current_pattern(e) as i32;
        let step = lay.first_step + (x.max(0.0) / lay.step_w) as usize;
        if step >= lay.steps && kind != MouseKind::Up {
            return false;
        }
        let switch = dreist_engine::pattern::lane_style(lane) == 2;
        let at_y = if switch { (y <= h / 2.0) as i32 as f32 } else { (1.0 - y / h).clamp(0.0, 1.0) };
        let send =
            |out: &mut Vec<Command>, st: usize, v: f32| out.push(msg(id::SET_STEP_GRAPH_STATE, &[e as i32, track as i32, -1, cur, track as i32, st as i32], &[lane as f32, v.clamp(0.0, 1.0), 1.0]));
        let mods = app.mods;
        let g = &mut self.graph_drag_state;
        match kind {
            MouseKind::RightDown => {
                let menu_mode = crate::options::Options::load().bool("PatternEditorRightClickMode", false);
                if menu_mode != mods.alt {
                    self.graph_target = Some((track, lane, step));
                    self.graph_menu = Some(false);
                    app.open_menu(path, GRAPH_MENU.iter().map(|s| s.to_string()).collect(), None);
                    return false;
                }
                *g = GraphDrag { anchor: step, erase: true, ..GraphDrag::default() };
                send(out, step, dreist_formats::params::step_lane_default(lane));
                self.graph_last = Some(step);
                true
            }
            MouseKind::Down => {
                self.graph_snap = None;
                *g = GraphDrag { anchor: step, value: at_y, last_y: y, line: mods.alt && !switch, erase: false };
                send(out, step, at_y);
                self.graph_last = Some(step);
                true
            }
            MouseKind::Drag => {
                let from = self.graph_last.unwrap_or(step);
                let (lo, hi) = (from.min(step), from.max(step));
                if g.erase {
                    for st in lo..=hi {
                        send(out, st, dreist_formats::params::step_lane_default(lane));
                    }
                } else if g.line {
                    let s0 = g.anchor;
                    if step == s0 {
                        g.value = at_y;
                        send(out, step, at_y);
                    } else {
                        let slope = (g.value - at_y) / (step as f32 - s0 as f32);
                        let mut v = g.value;
                        if step > s0 {
                            for st in s0..=step {
                                send(out, st, v);
                                v -= slope;
                            }
                        } else {
                            for st in (step..=s0).rev() {
                                send(out, st, v);
                                v += slope;
                            }
                        }
                    }
                } else {
                    let v = if switch {
                        at_y
                    } else {
                        let v = if mods.shift { g.value - (y - g.last_y) * 0.001 } else { at_y };
                        g.value = v.clamp(0.0, 1.0);
                        g.last_y = y;
                        g.value
                    };
                    for st in lo..=hi {
                        send(out, st, v);
                    }
                }
                self.graph_last = Some(step);
                true
            }
            MouseKind::Up => {
                self.graph_last = None;
                *g = GraphDrag::default();
                false
            }
        }
    }

    /// (track, lane) of the graph controls containing `path`: the single
    /// graph's `SingleAutoControl` or a MultiTrack lane row.
    pub(crate) fn graph_binding_target(&self, app: &App, model: &Model, path: &[usize]) -> Option<(usize, usize)> {
        let e = model.engine;
        if app.root.path_of("SingleAutoControl").is_some_and(|p| path.starts_with(&p)) {
            return Some((model.pad[e], model.pattern_view[e].lane));
        }
        let (t, slot) = self.auto_slot_of(app, path)?;
        Some((t, self.auto_lanes.get(&(e, t)).and_then(|v| v.get(slot)).copied().unwrap_or(0)))
    }

    /// The MultiTrack lane controls containing `path`: (track, slot).
    fn auto_slot_of(&self, app: &App, path: &[usize]) -> Option<(usize, usize)> {
        let rp = self.rows.as_ref()?;
        if !path.starts_with(rp) || path.len() <= rp.len() {
            return None;
        }
        let row = app.ctrl(&path[..rp.len() + 1])?;
        Some((row.attr("autoTrack")?.parse().ok()?, row.attr("autoSlot")?.parse().ok()?))
    }

    /// MultiTrack automation: the track rows' AutomateButton
    /// ([re:180015]), the lane controls'
    /// Add / Remove ([re:f28431] / [re:90326b]) and
    /// the lane context for their other controls. `Some(changed)` when
    /// handled here.
    fn auto_event(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent) -> Option<bool> {
        let e = model.engine;
        let path = match ev {
            UiEvent::Clicked { path, .. } | UiEvent::ValueChanged { path, .. } | UiEvent::Pressed { path, .. } => path,
            _ => return None,
        };
        let name = match ev {
            UiEvent::Clicked { name, .. } | UiEvent::ValueChanged { name, .. } | UiEvent::Pressed { name, .. } => name.as_str(),
            _ => "",
        };
        if name == "AutomateButton" {
            if !matches!(ev, UiEvent::Clicked { .. }) {
                return Some(false);
            }
            let rp = self.rows.clone()?;
            let r = app.ctrl(path.get(..rp.len() + 1)?)?.attr("trackRow")?.parse::<usize>().ok()?;
            let lay = self.layout(app, model)?;
            let t = lay.first_track + r;
            if !self.auto_open.remove(&(e, t)) {
                self.auto_open.insert((e, t));
                self.auto_lanes.entry((e, t)).or_insert_with(|| vec![0]);
            }
            return Some(true);
        }
        let (t, slot) = self.auto_slot_of(app, path)?;
        let lanes = self.auto_lanes.entry((e, t)).or_insert_with(|| vec![0]);
        let lane = lanes.get(slot).copied().unwrap_or(0);
        model.pad[e] = t;
        model.pattern_view[e].lane = lane;
        if !matches!(ev, UiEvent::Clicked { .. }) {
            return None;
        }
        match name {
            "AddButton" => {
                let next = (0..dreist_engine::pattern::LANES).find(|l| !lanes.contains(l)).unwrap_or(0);
                lanes.insert((slot + 1).min(lanes.len()), next);
                Some(true)
            }
            "RemoveButton" => {
                if slot < lanes.len() {
                    lanes.remove(slot);
                }
                if lanes.is_empty() {
                    self.auto_lanes.remove(&(e, t));
                    self.auto_open.remove(&(e, t));
                }
                Some(true)
            }
            "GraphCombo" => {
                self.auto_ctx = Some((t, slot));
                None
            }
            _ => None,
        }
    }

    fn toolbar_action(&mut self, app: &mut App, model: &mut Model, anchor: &[usize], a: ToolbarAction, out: &mut Vec<Command>) -> bool {
        let e = model.engine;
        let cur = model.current_pattern(e) as i32;
        let pat = model.pattern(e, cur as usize);
        match a {
            ToolbarAction::DragEdits => {
                let lanes = drag_lanes();
                let checked = lanes.iter().position(|&l| l == model.pattern_view[e].drag_lane);
                let labels = lanes.iter().map(|&l| lane_label(l)).collect();
                self.toolbar_menu = Some(lanes.into_iter().map(ToolbarAction::DragLane).collect());
                let anchor = app.root.path_of("DragEdit").unwrap_or_else(|| anchor.to_vec());
                app.open_menu(&anchor, labels, checked);
                false
            }
            ToolbarAction::DragLane(l) => {
                model.pattern_view[e].drag_lane = l;
                true
            }
            // "Single Track View" is the toolbar View button (option
            // `PatternEditorViewMode`).
            ToolbarAction::SingleTrack => {
                self.set_view(!self.multi, model);
                true
            }
            // [re:a5c620]: e_quantizePatternTiming; with a selection only
            // the selected steps' Shift goes back to the default.
            ToolbarAction::Quantize => {
                if self.selection.is_empty() {
                    out.push(msg(id::QUANTIZE_PATTERN_TIMING, &[e as i32, -1, -1, cur, -1, -1], &[0.0]));
                } else {
                    let d = dreist_formats::params::step_lane_default(lane::SHIFT);
                    for &(t, st) in &self.selection {
                        out.push(msg(id::SET_STEP_GRAPH_STATE, &[e as i32, t as i32, -1, cur, t as i32, st as i32], &[lane::SHIFT as f32, d, 0.0]));
                    }
                }
                true
            }
            ToolbarAction::Duplicate => {
                self.selection = duplicate(&pat, &self.selection, e, cur, out);
                true
            }
            ToolbarAction::Remix => {
                out.push(msg(id::SET_PATTERN_MONO_MODE, &[e as i32, -1, -1, cur], &[!pat.mono as i32 as f32]));
                true
            }
            ToolbarAction::Gated | ToolbarAction::Restart => {
                let (g, r) = (pat.sync.gated(), pat.sync.restarts());
                let (g, r) = if a == ToolbarAction::Gated { (!g, r) } else { (g, !r) };
                out.push(msg(id::SET_PATTERN_SYNC_MODE, &[e as i32, -1, -1, cur], &[(g as u32 * 2 + r as u32) as f32]));
                true
            }
            ToolbarAction::None => false,
        }
    }

    /// Steps that are on inside the rubber band join the selection.
    fn update_band(&mut self, pat: &PatternData) {
        let Some(((t0, s0), (t1, s1))) = self.band else { return };
        for t in t0.min(t1)..=t0.max(t1) {
            for s in s0.min(s1)..=s0.max(s1) {
                if pat.tracks.get(t).and_then(|tr| tr.steps.get(s)).is_some_and(|st| st.on) {
                    self.selection.insert((t, s));
                }
            }
        }
    }

    /// Selection keys: cmd-A select all, cmd-C copy, cmd-X cut, cmd-V paste
    /// at the selected track (step of the first selected cell, else 0),
    /// Delete removes, Left / Right move the selection by a step.
    /// Steps are selected (the arrow keys nudge them).
    pub fn has_selection(&self) -> bool {
        !self.selection.is_empty()
    }

    fn shortcut(&mut self, model: &mut Model, key: &str, cmd: bool, out: &mut Vec<Command>) -> bool {
        let e = model.engine;
        let cur = model.current_pattern(e);
        let pat = model.pattern(e, cur);
        let get = |t: usize, s: usize| pat.tracks.get(t).and_then(|tr| tr.steps.get(s)).cloned();
        match (key, cmd) {
            ("a", true) => {
                self.selection = (0..pat.tracks.len()).flat_map(|t| (0..pat.steps as usize).map(move |s| (t, s))).filter(|&(t, s)| get(t, s).is_some_and(|d| d.on)).collect();
                true
            }
            ("c", true) | ("x", true) => {
                let Some(t0) = self.selection.iter().map(|x| x.0).min() else { return false };
                let s0 = self.selection.iter().map(|x| x.1).min().unwrap_or(0);
                self.clipboard = self.selection.iter().filter_map(|&(t, s)| Some((t - t0, s - s0, get(t, s)?))).collect();
                if key == "x" {
                    for &(t, s) in &self.selection {
                        write_step(out, e, cur as i32, t, s, &dreist_engine::pattern::StepData::off());
                    }
                    self.selection.clear();
                }
                true
            }
            ("v", true) => {
                if self.clipboard.is_empty() {
                    return false;
                }
                let t0 = model.pad[e];
                let s0 = self.selection.iter().map(|x| x.1).min().unwrap_or(0);
                self.selection.clear();
                for (dt, ds, d) in self.clipboard.clone() {
                    let (t, s) = (t0 + dt, s0 + ds);
                    if t < pat.tracks.len() && s < pat.steps as usize {
                        write_step(out, e, cur as i32, t, s, &d);
                        self.selection.insert((t, s));
                    }
                }
                true
            }
            ("delete", _) => {
                for &(t, s) in &self.selection {
                    write_step(out, e, cur as i32, t, s, &dreist_engine::pattern::StepData::off());
                }
                let any = !self.selection.is_empty();
                self.selection.clear();
                any
            }
            ("left" | "right", _) if !self.selection.is_empty() => {
                let n = pat.steps as usize;
                let fwd = key == "right";
                let moved: Vec<(usize, usize, dreist_engine::pattern::StepData)> = self.selection.iter().filter_map(|&(t, s)| Some((t, s, get(t, s)?))).collect();
                for (t, s, _) in &moved {
                    write_step(out, e, cur as i32, *t, *s, &dreist_engine::pattern::StepData::off());
                }
                self.selection.clear();
                for (t, s, d) in moved {
                    let s2 = if fwd { (s + 1) % n } else { (s + n - 1) % n };
                    write_step(out, e, cur as i32, t, s2, &d);
                    self.selection.insert((t, s2));
                }
                true
            }
            _ => false,
        }
    }

    /// Code-drawn parts; true replaces the control's stock drawing.
    pub fn draw(&self, model: &Model, r: &Renderer, p: &mut Painter, c: &Ctrl) -> bool {
        match c.overload.as_str() {
            "PatternGrid" => {
                self.draw_grid(model, r, p, c);
                true
            }
            "PatternTimeline" => {
                self.draw_timeline(model, r, p, c);
                true
            }
            "PatternGraph" => {
                self.draw_graph(model, r, p, c);
                true
            }
            _ => false,
        }
    }

    fn draw_grid(&self, model: &Model, r: &Renderer, p: &mut Painter, c: &Ctrl) {
        let s = r.styles;
        let e = model.engine;
        let pat = self.drawn_pattern(model);
        let lay = if self.multi { GridLayout::compute_multi(model, &pat, c.rect.w, c.rect.h, &|t| self.lanes(e, t)) } else { GridLayout::compute(model, &pat, c.rect.w, c.rect.h) };
        let (w, h) = ((lay.step_range().end as f32 - lay.first_step as f32) * lay.step_w, c.rect.h as f32);
        let pink = Colour::rgba(255, 200, 200, 255);
        let fill = s.colour(BACK_STYLE, "colFill", pink);
        let alt = s.colour(BACK_STYLE, "colAltFill", pink);
        // Background stripes by the shading mode ([re:ac984a]).
        for st in lay.step_range() {
            let col = if shaded(pat.resolution, st) { alt } else { fill };
            p.fill_rect(RectF::new(lay.step_x(st), 0.0, lay.step_w.ceil(), h), col);
        }
        // Selected track row.
        let sel = model.pad[e];
        if sel >= lay.first_track && sel < lay.first_track + lay.tracks {
            let y = lay.row_y(sel - lay.first_track);
            p.fill_rect(RectF::new(0.0, y, w, lay.row_h), s.colour(BACK_STYLE, "colSelectedTrack", pink));
        }
        // Playback position (per-track step when tracks wrap).
        if let Some(pos) = model.steps[e] {
            let col = s.colour(BACK_STYLE, "colPosition", pink);
            for row in 0..lay.tracks {
                let tl = pat.tracks.get(lay.first_track + row).map(|t| t.length.max(1)).unwrap_or(pat.steps) as usize;
                let st = pos as usize % tl.max(1);
                if lay.step_range().contains(&st) {
                    p.fill_rect(RectF::new(lay.step_x(st), lay.row_y(row), lay.step_w - 1.0, lay.row_h), col);
                }
            }
        }
        // Steps.
        let step_col = s.colour(TRACK_STYLE, "colStep", Colour::BLACK);
        let overlay = s.colour(TRACK_STYLE, "colOverlay", pink);
        for row in 0..lay.tracks {
            let Some(t) = pat.tracks.get(lay.first_track + row) else { continue };
            for (st, step) in t.steps.iter().enumerate().take(lay.steps) {
                if !step.on || !lay.step_range().contains(&st) {
                    continue;
                }
                let mut a = lay.step_rect(row, st);
                if a.h >= 10.0 {
                    a = RectF::new(a.x + 2.0, a.y + 1.0, a.w - 4.0, a.h - 3.0);
                }
                let a = RectF::new(a.x.round(), a.y.round(), a.w.round(), a.h.round());
                let a = if a.h <= 4.0 {
                    RectF::new(a.x - 1.0, a.y - 1.0, (a.w + 2.0).max(0.0), (a.h + 2.0).max(0.0))
                } else if a.w > 4.0 {
                    a.inset(2.0)
                } else {
                    a
                };
                let vel = step.lanes[lane::VELOCITY].clamp(0.0, 1.0);
                let radius = if a.h * 0.5 < 2.0 { (a.h * 0.25).floor().max(0.0) } else { 2.0 };
                p.fill_rounded(a, radius, [true; 4], step_col.with_alpha(vel * 0.85 + 0.15));
            }
            // Beyond the track length.
            let len = t.length as usize;
            if len < lay.steps {
                let x = lay.step_x(len).max(0.0);
                p.fill_rect(RectF::new(x, lay.row_y(row), w - x + 1.0, lay.row_h), overlay);
            }
            // The length marker ([re:4ad041]
            // [re:e606aa]): a left-pointing triangle, tip at x + 1, half
            // height 0.4·min(h, 12), `PatternTrackMarkerStyle` colFill
            // (colAltFill while dragged).
            let drag = self.marker_drag.filter(|(tr, _, all)| *all || *tr == lay.first_track + row);
            let len = if t.length > 0 { t.length as usize } else { lay.steps };
            let mx = drag.map(|(_, x, _)| x.min(c.rect.w as f32 - MARKER_W)).unwrap_or_else(|| Self::marker_x(&lay, len, c.rect.w as f32));
            let mh = lay.row_h.min(MARKER_H);
            let (cy, half) = (lay.row_y(row) + mh * 0.5, mh.min(12.0) * 0.4);
            let col = s.colour("PatternTrackMarkerStyle", if drag.is_some() { "colAltFill" } else { "colFill" }, Colour::rgba(40, 40, 40, 255));
            p.fill_polygon(&[(mx + 1.0, cy), (mx + MARKER_W * 0.7, cy - half), (mx + MARKER_W * 0.7, cy + half)], col);
        }
        // Selected steps and the rubber band.
        let sel = s.colour("PatternTrackMarkerStyle", "colAltFill", Colour::rgba(0, 200, 255, 255));
        for &(t, st) in &self.selection {
            if t >= lay.first_track && t < lay.first_track + lay.tracks && lay.step_range().contains(&st) {
                let a = lay.step_rect(t - lay.first_track, st).inset(1.0);
                p.stroke_rect(a, sel, 2.0);
            }
        }
        if let Some(((t0, s0), (t1, s1))) = self.band {
            let r0 = lay.row_y(t0.min(t1).saturating_sub(lay.first_track));
            let r1 = lay.row_y(t0.max(t1).saturating_sub(lay.first_track)) + lay.row_h;
            let x0 = lay.step_x(s0.min(s1));
            let x1 = lay.step_x(s0.max(s1) + 1);
            p.fill_rect(RectF::new(x0, r0, x1 - x0, r1 - r0), sel.with_alpha(0.15));
            p.stroke_rect(RectF::new(x0, r0, x1 - x0, r1 - r0), sel, 1.0);
        }
        // Grid lines.
        let bar = s.colour(BACK_STYLE, "colGridBar", pink);
        let beat = s.colour(BACK_STYLE, "colGridBeat", pink);
        for row in 0..lay.tracks {
            for y in [lay.row_y(row), lay.row_y(row) + lay.row_h] {
                let y = y.round() - 0.5;
                p.line(0.0, y, w, y, bar.with_alpha(bar.f_alpha() * 0.35), 1.0);
            }
        }
        // MultiTrack graph rows.
        for gr in &lay.graphs {
            self.draw_lane(r, p, &pat, &lay, gr.track, gr.lane, gr.y, w, gr.h);
        }
        for st in lay.first_step..=lay.step_range().end {
            let x = lay.step_x(st).round() + 0.5;
            let col = if st % 4 == 0 || st == lay.steps { bar } else { beat };
            p.line(x, 0.0, x, h, col, 1.0);
        }
        // Pattern power off.
        if !model.pattern_power(e) {
            let col = s.colour(BACK_STYLE, "colPowerOverlay", Colour::BLACK);
            p.fill_rect(RectF::new(0.0, 0.0, c.rect.w as f32, h), col.with_alpha(0.3));
            let font = s.font(BACK_STYLE);
            let text = s.colour(BACK_STYLE, "colText", Colour::BLACK);
            p.text(RectF::new(0.0, 0.0, c.rect.w as f32, h), "Pattern power off", text, &font, dreist_gui::style::HAlign::Center, dreist_gui::style::VAlign::Center);
        }
    }

    /// [re:4a55a4] / step graph drawing [re:8b937e] for
    /// the selected track's lane.
    fn draw_graph(&self, model: &Model, r: &Renderer, p: &mut Painter, c: &Ctrl) {
        let e = model.engine;
        let pat = self.drawn_pattern(model);
        let lay = GridLayout::compute(model, &pat, self.grid_w.get(), 100);
        self.draw_lane(r, p, &pat, &lay, model.pad[e], model.pattern_view[e].lane, 0.0, c.rect.w as f32, c.rect.h as f32);
    }

    /// One lane graph of `track` in the rectangle (0, y0, w, h).
    #[allow(clippy::too_many_arguments)]
    fn draw_lane(&self, r: &Renderer, p: &mut Painter, pat: &PatternData, lay: &GridLayout, track: usize, lane: usize, y0: f32, w: f32, h: f32) {
        use dreist_engine::pattern::lane_style;
        let s = r.styles;
        let pink = Colour::rgba(255, 200, 200, 255);
        let fill = s.colour(BACK_STYLE, "colFill", pink);
        let alt = s.colour(BACK_STYLE, "colAltFill", pink);
        p.fill_rect(RectF::new(0.0, y0, w, h), fill);
        for st in lay.step_range() {
            let col = if shaded(pat.resolution, st) { alt } else { fill };
            p.fill_rect(RectF::new(lay.step_x(st), y0, lay.step_w.ceil(), h), col);
        }
        let style = lane_style(lane);
        let divider = s.colour(BACK_STYLE, "colGridBar", pink);
        if style == 1 || style == 4 {
            p.line(0.0, y0 + (h / 2.0).floor() + 0.5, w, y0 + (h / 2.0).floor() + 0.5, divider, 1.0);
        }
        let step_col = s.colour(TRACK_STYLE, "colStep", Colour::BLACK);
        let Some(t) = pat.tracks.get(track) else { return };
        for st in lay.step_range() {
            let Some(step) = t.steps.get(st) else { continue };
            let col = if step.on { step_col } else { step_col.with_alpha(0.125) };
            let v = step.lanes.get(lane).copied().unwrap_or(0.0);
            let x = lay.step_x(st);
            let bw = (lay.step_w - 4.0).max(1.0);
            let radius = if h * 0.5 < 2.0 { (h * 0.25).floor().max(0.0) } else { 2.0 };
            match style {
                // Unipolar: a bar from the bottom.
                0 => {
                    let bh = (v * (h - 3.0)).floor().max(1.0);
                    p.fill_rounded(RectF::new(x + 2.0, y0 + h - 2.0 - bh, bw, bh), radius, [true; 4], col);
                }
                // Bipolar / pitch: from the centre line.
                1 | 4 => {
                    let mid = (h / 2.0).floor();
                    if v > 0.5 {
                        let top = (mid - (mid - 2.0) * 2.0 * (v - 0.5)).max(0.0);
                        p.fill_rounded(RectF::new(x + 2.0, y0 + top, bw, mid - top), radius, [true; 4], col);
                    } else if v < 0.5 {
                        let bh = (mid * (1.0 - 2.0 * v)).floor().max(2.0) - 1.0;
                        p.fill_rounded(RectF::new(x + 2.0, y0 + mid + 1.0, bw, bh), radius, [true; 4], col);
                    }
                }
                // On/off: Reverse draws a left arrow, the link two strokes.
                2 => {
                    if v >= 0.5 {
                        let (l, rr) = (x + 2.0, x + 2.0 + bw);
                        if lane == 4 {
                            let k = (bw / 6.0).floor();
                            p.fill_polygon(&[(l + k, y0 + h / 2.0), (rr - k, y0 + h * 0.25), (rr - k, y0 + h * 0.75)], col);
                        } else {
                            p.line(l + bw * 0.25, y0 + h * 0.75, l + bw * 0.5, y0 + h * 0.25, col, 2.0);
                            p.line(l + bw * 0.5, y0 + h * 0.75, l + bw * 0.75, y0 + h * 0.25, col, 2.0);
                        }
                    }
                }
                // Stepped: one box per repeat / forced layer / level.
                _ => {
                    let n = match lane {
                        2 => (v * 7.0).floor() as usize,
                        15 => {
                            if v < 0.1 {
                                0
                            } else {
                                ((v - 0.05) / 0.125).round().clamp(1.0, 8.0) as usize
                            }
                        }
                        _ => (2.0 * v + 1.0).floor() as usize,
                    };
                    let unit = ((h - 3.0) / if lane == 2 || lane == 15 { 8.0 } else { 3.0 }).floor().max(2.0);
                    for k in 0..n {
                        let y = y0 + h - 1.0 - unit * (k + 1) as f32;
                        p.fill_rect(RectF::new(x + 2.0, y, bw, unit - 2.0), col);
                    }
                }
            }
        }
        for st in lay.first_step..=lay.step_range().end {
            let x = lay.step_x(st).round() + 0.5;
            let beat = s.colour(BACK_STYLE, if st % 4 == 0 { "colGridBar" } else { "colGridBeat" }, pink);
            p.line(x, y0, x, y0 + h, beat, 1.0);
        }
    }

    fn draw_timeline(&self, model: &Model, r: &Renderer, p: &mut Painter, c: &Ctrl) {
        let s = r.styles;
        let pat = self.drawn_pattern(model);
        let lay = GridLayout::compute(model, &pat, self.grid_width(c), 100);
        let steps = lay.steps;
        let step_w = lay.step_w;
        let line = s.colour(TIMELINE_STYLE, "colBorder", Colour::rgba(128, 0, 128, 255));
        let text = s.colour(TIMELINE_STYLE, "colText", Colour::rgba(255, 255, 0, 255));
        let font = s.font(TIMELINE_STYLE);
        let every = if step_w <= 7.0 { 16 } else { 4 };
        let h = c.rect.h as f32;
        for st in lay.step_range() {
            let x = lay.step_x(st).round();
            if st % every == 0 {
                p.line(x + 0.5, 0.0, x + 0.5, h + 1.0, line, 1.0);
                p.text(RectF::new(x + 2.0, 0.0, 30.0, h), &format!("{}", st + 1), text, &font, dreist_gui::style::HAlign::Left, dreist_gui::style::VAlign::Center);
            }
        }
        if lay.step_range().end == steps {
            let end = lay.step_x(steps).round() + 0.5;
            p.line(end, 0.0, end, h + 1.0, line, 1.0);
        }
    }

    /// The timeline shares the grid's step positions (same left edge).
    fn grid_width(&self, _timeline: &Ctrl) -> i32 {
        self.grid_w.get()
    }
}

/// Mirror `e_selectPattern` into the GUI document (`EngineState CurrentPattern`).
/// A multi-tool press on a step ([re:86dd7e] last step,
/// brushed range, gated pad, dragging a
/// duration chain's end, last y).
#[derive(Clone, Copy, Debug)]
struct Brush {
    track: usize,
    last: usize,
    min: usize,
    max: usize,
    gated: bool,
    dur_drag: bool,
    last_y: f32,
}

/// [re:7cbaab]: one-shot pads
/// always; else the step is on and the next one is not tied to it.
fn last_in_duration_chain(pat: &PatternData, track: usize, step: usize, gated: bool) -> bool {
    if !gated {
        return true;
    }
    if step + 1 >= pat.steps as usize {
        return false;
    }
    let t = &pat.tracks[track];
    t.step(step as u32).on && !t.step(step as u32 + 1).tied()
}

/// What the step messages need.
struct StepCtx<'a> {
    e: usize,
    cur: i32,
    drag_lane: usize,
    pat: &'a PatternData,
    lay: &'a GridLayout,
}

impl StepCtx<'_> {
    fn index(&self, track: usize, step: usize) -> [i32; 6] {
        [self.e as i32, track as i32, -1, self.cur, track as i32, step as i32]
    }

    /// Note on [re:84f436]: nothing when the step is already on
    /// with this duration; duration 0 turns it off; else 0xd2 [velocity (1
    /// while Drag Edits is another lane), duration, tied].
    fn note_on(&self, out: &mut Vec<Command>, track: usize, step: usize, vel: f32, dur: f32, tie: bool) {
        let st = self.pat.tracks[track].step(step as u32);
        if st.on && dur == st.lanes[dreist_engine::pattern::lane::DURATION] {
            return;
        }
        if dur == 0.0 {
            return self.note_off(out, track, step, false);
        }
        let vel = if self.drag_lane != 0 { 1.0 } else { vel };
        out.push(msg(id::SET_STEP_ON, &self.index(track, step), &[vel, dur, tie as i32 as f32]));
    }

    /// Note off [re:2edb8e]: 0xd3 [erased by the user].
    fn note_off(&self, out: &mut Vec<Command>, track: usize, step: usize, user: bool) {
        out.push(msg(id::SET_STEP_OFF, &self.index(track, step), &[user as i32 as f32]));
    }

    /// Velocity set [re:98683f]: 0xd5 velocity, or 0xd6 for the
    /// Drag Edits lane, clamped to 0..1.
    fn lane_set(&self, out: &mut Vec<Command>, track: usize, step: usize, v: f32) {
        let v = v.clamp(0.0, 1.0);
        if self.drag_lane == 0 {
            out.push(msg(id::SET_STEP_VELOCITY, &self.index(track, step), &[v]));
        } else {
            out.push(msg(id::SET_STEP_GRAPH_STATE, &self.index(track, step), &[self.drag_lane as f32, v, 1.0]));
        }
    }

    /// Velocity without fade [re:387ce3]: value − 0.005 per pixel moved down
    /// (lane 4: the upper half of the row is 1, else 0) for steps a..=b.
    fn set_velocity_no_fade(&self, out: &mut Vec<Command>, track: usize, y: f32, b: &mut Brush, a: usize, z: usize, v: f32) -> f32 {
        let v = if self.drag_lane == 4 {
            let top = self.lay.track_y.get(track.saturating_sub(self.lay.first_track)).copied().unwrap_or(0.0);
            if y - top < self.lay.row_h / 2.0 { 1.0 } else { 0.0 }
        } else {
            v + (y - b.last_y) * -0.005
        }
        .clamp(0.0, 1.0);
        b.last_y = y;
        for k in a.min(z)..=a.max(z) {
            self.lane_set(out, track, k, v);
        }
        v
    }

    /// Velocity [re:8e5320]: with alt over a range, above / below the
    /// row a ramp from the value to 1 − t (t = distance / 6 rows) across it;
    /// otherwise as [`Self::set_velocity_no_fade`].
    #[allow(clippy::too_many_arguments)]
    fn set_velocity(&self, out: &mut Vec<Command>, track: usize, x: f32, y: f32, b: &mut Brush, step: usize, fade: bool, v: f32) -> f32 {
        let (lo, hi) = (b.min, b.max);
        if !fade || hi == lo {
            return self.set_velocity_no_fade(out, track, y, b, lo, hi, v);
        }
        let top = self.lay.track_y.get(track.saturating_sub(self.lay.first_track)).copied().unwrap_or(0.0);
        let (sx, rh) = (self.lay.step_x(step), self.lay.row_h);
        let span = (hi - lo) as f32;
        let six = (rh * 6.0).round().max(1.0);
        if y >= top && y < top + rh {
            if x >= sx && x < sx + self.lay.step_w {
                return self.set_velocity_no_fade(out, track, y, b, lo, hi, v);
            }
            return v;
        }
        if y >= top + rh {
            let t = ((y - (top + rh)).round() / six).clamp(0.0, 1.0);
            for (i, k) in (lo..=hi).enumerate() {
                self.lane_set(out, track, k, v - i as f32 * ((t - 1.0 + v) / span));
            }
        } else {
            let t = ((top - y).round() / six).clamp(0.0, 1.0);
            for (i, k) in (lo..=hi).enumerate() {
                self.lane_set(out, track, k, i as f32 * ((v - (1.0 - t)) / span) + (1.0 - t));
            }
        }
        v
    }
}

/// [re:45e9a3] resize listener (skin lambda
/// [re:0570da]): PatternNameBox is 100 px wide under a 1101 px toolbar, else
/// 150; against the swing panel (SwingKnob's parent) PatternNameBox and
/// the Quantize panel show only when left of it without overlap, DragEdit
/// only when its x ≤ the panel's and without overlap; OptionsButton shows
/// exactly when DragEdit does not, and the View button's panel the other
/// way round (all via the user enable state).
fn layout_toolbar(app: &mut App, named: &dreist_gui::app::PathCache, toolbar: &dreist_gui::app::PathCache) {
    let Some(tb) = named.path_in(app, &[], "ToolBar") else { return };
    let tw = app.ctrl(&tb).map(|c| c.rect.w).unwrap_or(0);
    let find = |app: &App, n: &'static str| toolbar.path_in(app, &tb, n);
    if let Some(nb) = find(app, "PatternNameBox") {
        let w = if tw < 0x44d { 100 } else { 150 };
        if let Some(c) = app.ctrl_mut(&nb) {
            if c.rect.w != w {
                let r = c.rect;
                c.set_rect(dreist_gui::Rect::new(r.x, r.y, w, r.h));
            }
        }
    }
    let rect = |app: &App, p: &[usize]| -> Option<(i32, i32, i32, i32)> {
        let (x, y) = app.origin(p);
        let c = app.ctrl(p)?;
        Some((x, y, c.rect.w, c.rect.h))
    };
    let parent = |p: Vec<usize>| p[..p.len().saturating_sub(1)].to_vec();
    let Some(swing) = find(app, "SwingKnob").map(parent) else { return };
    let Some(a) = rect(app, &swing) else { return };
    let overlaps = |b: (i32, i32, i32, i32)| {
        let (x0, x1) = (a.0.max(b.0), (a.0 + a.2).min(b.0 + b.2));
        let (y0, y1) = (a.1.max(b.1), (a.1 + a.3).min(b.1 + b.3));
        x1 > x0 && y1 > y0
    };
    let set_vis = |app: &mut App, p: &[usize], on: bool| {
        if let Some(c) = app.ctrl_mut(p) {
            if c.visible != on || c.enabled != on {
                c.visible = on;
                c.enabled = on;
                app.dirty = true;
            }
        }
    };
    if let Some(nb) = find(app, "PatternNameBox") {
        if let Some(b) = rect(app, &nb) {
            set_vis(app, &nb, b.0 <= a.0 && !overlaps(b));
        }
    }
    let drag_fits = find(app, "DragEdit").and_then(|p| rect(app, &p).map(|c| (p, a.0 >= c.0 && !overlaps(c))));
    if let Some((p, fits)) = &drag_fits {
        set_vis(app, p, *fits);
    }
    if let Some(q) = find(app, "QuantizeButton").map(parent) {
        if let Some(d) = rect(app, &q) {
            set_vis(app, &q, d.0 <= a.0 && !overlaps(d));
        }
    }
    let opts = drag_fits.as_ref().map(|(_, f)| !f).unwrap_or(false);
    if let Some(o) = find(app, "OptionsButton") {
        set_vis(app, &o, opts);
    }
    if let Some(v) = find(app, "View").map(parent) {
        set_vis(app, &v, !opts);
    }
}

/// [re:7a5608] (option
/// `PatternEditorShadingMode`, [re:c192c5]: Fixed 0,
/// Adaptive 1, Cyclic 2, Off 3).
static SHADING_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn set_shading_mode(name: &str) {
    let m = crate::modal::SHADING.iter().position(|s| *s == name).unwrap_or(0) as u8;
    SHADING_MODE.store(m, std::sync::atomic::Ordering::Relaxed);
}

/// Repeat time per shading mode [re:83c623]: steps per shading
/// group — Fixed 4, Adaptive the resolution's steps per beat, Cyclic its
/// cycle table; `None` (−1, no alternation) for Off and coarse
/// resolutions.
pub fn shading_group(resolution: usize) -> Option<usize> {
    let g = match SHADING_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        0 => 4,
        1 => dreist_engine::pattern::resolution_beats(resolution),
        2 => dreist_engine::pattern::resolution_cycles(resolution),
        _ => -1,
    };
    (g > 0).then_some(g as usize)
}

/// Whether step `st` lies in a shaded (odd) group.
pub fn shaded(resolution: usize, st: usize) -> bool {
    shading_group(resolution).is_some_and(|g| (st / g) % 2 == 1)
}

pub(crate) fn set_current_pattern(model: &mut Model, e: usize, p: usize) {
    let eng = model.doc.children.iter_mut().find(|c| c.class == "Engine" && c.name.as_deref() == Some(&format!("Engine{e}")));
    if let Some(eng) = eng {
        if let Some(st) = eng.children.iter_mut().find(|c| c.class == "EngineState") {
            st.set("CurrentPattern", p);
        } else {
            let mut st = dreist_formats::Node::new("EngineState", Some("EngineState"));
            st.set("CurrentPattern", p);
            eng.children.push(st);
        }
    }
}

/// [re:d396b9].
pub fn compressed(v: f32, amount: f32, target: f32) -> f32 {
    if (amount - 0.5).abs() < 1e-6 {
        return v;
    }
    if amount > 0.5 {
        return ((target - v) * (amount * 2.0 - 1.0) + v).clamp(0.0, 1.0);
    }
    let k = 1.0 - amount * 2.0;
    if k.abs() < 1e-6 {
        return v;
    }
    if (target - 0.5).abs() < 1e-6 { ((v - target) * k + v).clamp(0.0, 1.0) } else { (((1.0 - v) - target) * k + v).clamp(0.0, 1.0) }
}

/// [re:3a5054]
/// (`r` uniform 0..1): amount quantised to 1 %, 0.5 = none, above adds and
/// below subtracts up to |2·amount − 1|·r.
pub fn randomised(v: f32, amount: f32, r: f32) -> f32 {
    let q = (amount * 100.0).round();
    if (q * 0.01 - 0.5).abs() < 1e-6 {
        return v;
    }
    let d = q * 0.02 - 1.0;
    if d.abs() < 1e-6 {
        return v;
    }
    let x = r * d.abs();
    (if d <= 0.0 { v - x } else { v + x }).clamp(0.0, 1.0)
}

#[cfg(test)]
mod graph_tests {
    #[test]
    fn compress_and_randomise() {
        assert_eq!(super::compressed(0.2, 0.5, 1.0), 0.2);
        assert!((super::compressed(0.2, 1.0, 1.0) - 1.0).abs() < 1e-6, "full compression reaches the target");
        assert!((super::compressed(0.2, 0.75, 1.0) - 0.6).abs() < 1e-6);
        assert_eq!(super::randomised(0.4, 0.5, 0.9), 0.4);
        assert!((super::randomised(0.4, 1.0, 0.5) - 0.9).abs() < 1e-6);
        assert!((super::randomised(0.4, 0.0, 0.5) - 0.0).abs() < 1e-6);
    }
}

#[cfg(test)]
mod insert_tests {
    #[test]
    fn shapes() {
        let mut r = || 0.5;
        assert_eq!(super::insert_shape(0, 4, &mut r), vec![0.0, 0.25, 0.5, 0.75]);
        assert_eq!(super::insert_shape(5, 4, &mut r), vec![1.0, 1.0, 0.0, 0.0]);
        assert_eq!(super::insert_shape(8, 4, &mut r), vec![1.0, 0.0, 1.0, 0.0]);
        assert!((super::insert_shape(2, 4, &mut r)[1] - 1.0).abs() < 1e-6);
    }
}

#[cfg(test)]
mod shading_tests {
    use super::*;

    #[test]
    fn shading_modes() {
        set_shading_mode("Adaptive");
        assert_eq!(shading_group(10), Some(8), "1/32: 8 steps a beat");
        assert_eq!(shading_group(2), None, "1/2 is coarser than a beat");
        set_shading_mode("Cyclic");
        assert_eq!(shading_group(8), Some(16));
        set_shading_mode("Off");
        assert!(!shaded(8, 5));
        set_shading_mode("Fixed");
        assert_eq!(shading_group(10), Some(4));
        assert!(shaded(8, 4) && !shaded(8, 3) && !shaded(8, 8));
    }
}

/// [re:6f3ef7].
const SVF_MODES: [&str; 10] = ["L2", "L4", "H2", "H4", "B2", "B4", "N2", "N4", "P2", "P4"];

/// A filter type step's text for one layer's filter model
/// (`round(FilterAlgorithm · 3)`): SH101, SVF, MS20, else none.
fn filter_type_text(algorithm: Option<f32>, f: f32) -> String {
    let pick = |names: &[&str], n: f32| match ((f * n) as i32 - 1).max(-1) {
        -1 => "As layer setup".to_string(),
        k => names.get(k as usize).map(|s| s.to_string()).unwrap_or_default(),
    };
    match algorithm.map(|a| (a * 3.0).round() as i32) {
        None | Some(1) => pick(&SVF_MODES, 10.0),
        Some(0) => pick(&dreist_dsp::sh101::SH101_MODES, 22.0),
        Some(2) => pick(&dreist_dsp::ms20::MS20_MODES, 4.0),
        _ => match (f * 3.0) as i32 {
            0 => "As layer setup".into(),
            1 => "Positive".into(),
            _ => "Negative".into(),
        },
    }
}

/// The value part of a graph tooltip (lane table of
/// [re:407d1e]).
fn graph_value_text(model: &Model, track: usize, lane: usize, f: f32) -> String {
    use crate::conv::{db_unity, pan_text};
    let pct = |v: f32| format!("{:03}%", (v * 100.0) as i64);
    let bip = |v: f32| format!("{:+03}%", (v * 200.0 - 100.0) as i64);
    match lane {
        0 => format!("{}", (f * 127.0) as i64),
        1 => pan_text(f),
        2 => format!("{} repeats", (f * 7.0 + 1.0).floor() as i64),
        3 => format!("{:+.2} step", 2.0 * f - 1.0),
        4 => if f != 0.0 { "Reversed" } else { "Forward" }.into(),
        5 => format!("{:+} oct", dreist_dsp::convert::octave(f, true) as i64),
        6 => format!("{:+} st", dreist_dsp::convert::semitones(f, true) as i64),
        7 => format!("{:+03} cents", (dreist_dsp::convert::fine_semitones(f) * 100.0) as i64),
        8 => format!("{:+03}%", ((f - 0.5) * 200.0) as i64),
        9 | 12 | 14 | 16 => pct(f),
        10 | 17 | 18 => String::new(),
        11 | 19..=38 => bip(f),
        13 => {
            let e = model.engine;
            let layers: Vec<usize> = (0..8).filter(|&l| model.layer(e, track, l).valid).collect();
            let alg =
                |l: usize| model.layer_node(e, track, l).map(|n| n.f32(dreist_formats::LayerParam::FilterAlgorithm.name()).unwrap_or(dreist_formats::LayerParam::FilterAlgorithm.default_value()));
            match layers.as_slice() {
                [] => filter_type_text(None, f),
                [l] => filter_type_text(alg(*l), f),
                ls => ls.iter().map(|&l| format!("Layer {}: {}", l + 1, filter_type_text(alg(l), f))).collect::<Vec<_>>().join("\n"),
            }
        }
        15 => match (f * 8.0).floor() as i64 - 1 {
            k if k < 0 => "Unforced".into(),
            k => format!("Layer {}", k + 1),
        },
        l if l >= 39 => match (l - 39) % 9 {
            0 => ["As Effect", "Active", "Inactive"][((2.0 * f + 1.0).floor() as usize).clamp(1, 3) - 1].into(),
            1 => pct(f),
            2 => db_unity(f),
            // "\nEffective: " + [re:f50115] of the pad
            // chain effect's value × f ([re:8a45b0]).
            _ => {
                let mut t = pct(f);
                if let Some((slot, param)) = dreist_formats::params::step_lane_fx_param(l) {
                    let e = model.engine;
                    if let Some((_, kind, node)) = crate::fxchain::effects(model, crate::fxchain::Target::Pad(e, track)).into_iter().find(|(s, _, _)| *s == slot) {
                        let key: Option<String> = dreist_formats::params::effect_param_names(kind).get(param).map(|n| n.chars().filter(|c| !c.is_whitespace()).collect());
                        let v = key.and_then(|k| node.f32(&k)).unwrap_or(0.5);
                        let sync = node.f32("Sync").is_none_or(|s| s >= 0.5);
                        if let Some(x) = crate::fxtext::text(kind, param, v * f, sync).filter(|x| !x.is_empty()) {
                            t.push_str("\nEffective: ");
                            t.push_str(&x);
                        }
                    }
                }
                t
            }
        },
        _ => pct(f),
    }
}
