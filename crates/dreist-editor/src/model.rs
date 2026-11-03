//! The editor's view of the project: the `.geist` document tree (the same
//! tree the engine saves), plus GUI-only selection state.

use dreist_formats::Node;

pub const ENGINES: usize = 8;
pub const PADS: usize = 64;
pub const LAYERS: usize = 8;

#[derive(Clone, Debug, Default)]
pub struct LayerFlags {
    pub present: bool,
    pub valid: bool,
    pub mute: bool,
    pub solo: bool,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub doc: Node,
    pub engine: usize,
    /// Selected pad per engine.
    pub pad: [usize; ENGINES],
    /// Visible pad bank: 0..3 = A..D (16 pads), 4 = all 64.
    pub bank: usize,
    pub playing: bool,
    /// Current (limited) step per engine, from the audio thread.
    pub steps: [Option<u32>; ENGINES],
    /// Mini sequencer playing step per transmod slot (−1: none), from the
    /// audio thread ([re:0440ca]).
    pub seq_steps: [i32; 16],
    /// Project name (file stem) for the header.
    pub name: Option<String>,
    /// Selected page of the editor tab panel (`EditorTabButtons` ids:
    /// 0 pattern, 1 scenes, 2 song, 3 layer mix, 4 pad mix, 5 mixer, 6 map).
    pub editor_tab: usize,
    /// Utility tab: 0 browser, 1 transmod.
    pub utility_tab: usize,
    /// Selected layer per engine (pad inspector).
    pub layer: [usize; ENGINES],
    /// The project's `.samples` folder (for waveforms).
    pub samples: Option<std::path::PathBuf>,
    /// Selected transmod source (0 = the parameters themselves).
    pub source: usize,
    /// Sampler state from the audio thread ([re:090c66]).
    pub sampler: SamplerView,
    /// Song position (quarter notes) while playing, from the audio thread.
    pub song_pos: Option<f64>,
    /// The host reports a loop ([re:a26535]).
    pub host_loop: bool,
    /// Option `PatternEditorFollow`: the pattern editor and the song
    /// scroll to keep the playback position in view.
    pub follow_playback: bool,
    /// `e_setModifiers` 0x26 ([re:6f7056] / +[re:80578e], not saved):
    /// actions apply to all layers / all pads.
    pub all_layers: bool,
    pub all_pads: bool,
    /// Engine multi-selection (`e_setEngineSelections` 0x43, [re:b00098]
    /// +[re:1b20b6]; not saved — loading selects only `CurrentEngine`).
    pub engine_sel: u8,
    /// Pattern editor scroll / zoom per engine.
    pub pattern_view: [crate::pattern::PatternView; ENGINES],
    /// Inspector wave display in sampling mode (live / last take).
    pub sampling: Option<Sampling>,
    /// A pattern grew since the commands were last taken: the engine needs
    /// a rebuild for the step storage (see [`Model::apply`]).
    pub(crate) pattern_grown: bool,
}

/// Frames per min/max pair of the live take display.
pub const LIVE_BUCKET: usize = 256;

/// Min/max of a stereo block.
pub fn bucket(l: &[f32], r: &[f32]) -> (f32, f32) {
    l.iter().chain(r.iter()).fold((0.0f32, 0.0f32), |(lo, hi), &x| (lo.min(x), hi.max(x)))
}

/// The pad inspector's wave display in sampling mode
/// ([re:085538], [re:af56a1]
/// [re:dda7e9]): the take being recorded or the last take, as min/max pairs
/// of `LIVE_BUCKET` frames, until the layer view takes over again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sampling {
    pub buckets: Vec<(f32, f32)>,
    pub frames: usize,
    /// (engine, pad, layer) selected when sampling started.
    pub selection: (usize, usize, usize),
}

/// Audio-thread side of the live take display: turns the growing take into
/// new min/max pairs.
#[derive(Clone, Debug, Default)]
pub struct LiveTake {
    frames: usize,
    active: bool,
}

impl LiveTake {
    /// Append pairs for the frames recorded since the last call, within
    /// `out`'s reserved room (the audio thread calls this; the rest follows
    /// once the editor drained it); returns whether a new take started
    /// (the display restarts).
    pub fn poll(&mut self, take: Option<(&[f32], &[f32])>, out: &mut Vec<(f32, f32)>) -> bool {
        let Some((l, r)) = take else {
            self.active = false;
            return false;
        };
        let reset = !self.active || l.len() < self.frames;
        if reset {
            self.frames = 0;
            self.active = true;
        }
        while self.frames + LIVE_BUCKET <= l.len() && out.len() < out.capacity() {
            let (a, b) = (self.frames, self.frames + LIVE_BUCKET);
            out.push(bucket(&l[a..b], r.get(a..b.min(r.len())).unwrap_or(&[])));
            self.frames = b;
        }
        reset
    }
}

/// What the sampler panel shows ([re:c2fc2f]).
#[derive(Clone, Debug, PartialEq)]
pub struct SamplerView {
    /// 0 idle, 1 armed, 2 recording.
    pub state: u8,
    pub start_mode: u8,
    /// 0 stereo, 1 left, 2 right.
    pub channels: u8,
    pub beats_mode: bool,
    pub bars: u32,
    pub beats: u32,
    pub secs: f32,
    pub input_gain: f32,
    pub thru: bool,
    pub start_threshold: f32,
    pub stop_threshold: f32,
    pub meters: (f32, f32),
    pub takes: usize,
    /// Follow (`e_setAutoSamplerMode` 0x100): `sampler::AUTO_MODES` index.
    pub auto_mode: usize,
    /// Preview volume (`e_setPreviewVolume` 0xe8).
    pub preview_volume: f32,
    /// Record source (`recordsource`, `sampler::SOURCE_*`).
    pub source: i32,
}

impl Default for SamplerView {
    fn default() -> Self {
        SamplerView::of(&dreist_engine::sampler::Sampler::default())
    }
}

impl SamplerView {
    pub fn of(s: &dreist_engine::sampler::Sampler) -> SamplerView {
        use dreist_engine::sampler::{Channels, State};
        SamplerView {
            state: match s.state {
                State::Idle => 0,
                State::Armed => 1,
                State::Recording => 2,
            },
            start_mode: s.start_mode as u8,
            channels: match s.channels {
                Channels::Stereo => 0,
                Channels::Left => 1,
                Channels::Right => 2,
            },
            beats_mode: s.beats_mode,
            bars: s.bars,
            beats: s.beats,
            secs: s.secs,
            input_gain: s.input_gain,
            thru: s.thru,
            start_threshold: s.start_threshold,
            stop_threshold: s.stop_threshold,
            meters: s.meters,
            takes: s.takes,
            auto_mode: s.auto_mode,
            preview_volume: 0.793_700_5,
            source: s.source,
        }
    }
}

/// Show a transmod knob for `source`: the base value, or the source's depth
/// (−1..1) as an editable zone around the base (see dreist-gui render).
pub fn bind_knob(c: &mut dreist_gui::Ctrl, source: usize, base: f32, depth: f32) {
    if source == 0 {
        c.value = base;
        c.remove_attr("editingDepth");
        c.remove_attr("modDepth");
    } else {
        c.value = (depth + 1.0) / 2.0;
        c.set_attr("editingDepth", "true".into());
        c.set_attr("baseValue", base.to_string());
        c.set_attr("modDepth", depth.to_string());
    }
}

/// The (source, value) to send for transmod knob `c` moved to `v` while
/// `source` is selected: a primary (centre) drag edits the base.
/// The transmod sources (bit per source 1..=28) with a non-zero depth on
/// parameter `key` of `node` (the objects' `Get…ModulationBitField`).
pub fn modulation_mask(node: Option<&Node>, key: &str) -> u32 {
    let Some(n) = node else { return 0 };
    (1..=28).filter(|&s| dreist_engine::engine::doc::attr_name(key, s).and_then(|k| n.f32(&k)).is_some_and(|d| d != 0.0)).fold(0, |m, s| m | 1 << s)
}

pub fn knob_edit(c: Option<&dreist_gui::Ctrl>, source: usize, v: f32) -> (usize, f32) {
    if c.is_some_and(|c| c.attr("dragPart") == Some("primary")) { (0, v) } else { (source, knob_value(source, v)) }
}

/// The value to send for a knob moved to `v` while editing `source`.
pub fn knob_value(source: usize, v: f32) -> f32 {
    if source == 0 { v } else { v * 2.0 - 1.0 }
}

impl Default for Model {
    fn default() -> Model {
        Model::new(Node::new("root", None))
    }
}

impl Model {
    /// Rebuild the engine from the current document (structural edits).
    pub fn rebuild_command(&self) -> crate::command::Command {
        crate::command::Command::Rebuild { doc: Box::new(self.doc.clone()), samples: self.samples.clone() }
    }

    /// Editable nodes (created on demand): engine, pad, layer.
    pub fn engine_mut(&mut self, e: usize) -> &mut Node {
        self.doc.child_mut_or_insert("Engine", &format!("Engine{e}"))
    }

    pub fn pad_mut(&mut self, e: usize, p: usize) -> &mut Node {
        self.engine_mut(e).child_mut_or_insert("Pad", &format!("Pad{p}"))
    }

    pub fn layer_mut(&mut self, e: usize, p: usize, l: usize) -> &mut Node {
        self.pad_mut(e, p).child_mut_or_insert("Layer", &format!("Layer{l}"))
    }

    pub fn set_pad_name(&mut self, e: usize, p: usize, name: &str) {
        self.pad_mut(e, p).child_mut_or_insert("FixedString", "PadName").set("string", name);
    }

    pub fn set_layer_name(&mut self, e: usize, p: usize, l: usize, name: &str) {
        self.layer_mut(e, p, l).child_mut_or_insert("FixedString", "LayerName").set("string", name);
    }

    pub fn set_pattern_name(&mut self, e: usize, pat: usize, name: &str) {
        let sp = self.engine_mut(e).child_mut_or_insert("StepPattern", &format!("StepPattern{pat}"));
        sp.child_mut_or_insert("VFixedString_64", "PatternName").set("string", name);
    }

    /// Scene `scene`'s name (`VFixedString_64 name`); empty when unset.
    pub fn scene_name(&self, scene: usize) -> String {
        let mgr = self.doc.children.iter().find(|c| c.class == "SceneManager");
        mgr.and_then(|m| m.child("Scene", &format!("Scene{scene}"))).and_then(|s| s.child("VFixedString_64", "name")).and_then(|n| n.attr("string")).unwrap_or("").to_string()
    }

    pub fn set_scene_name(&mut self, scene: usize, name: &str) {
        let mgr = self.doc.child_mut_or_insert("SceneManager", "Scenes");
        let sc = mgr.child_mut_or_insert("Scene", &format!("Scene{scene}"));
        sc.child_mut_or_insert("VFixedString_64", "name").set("string", name);
    }

    pub fn new(doc: Node) -> Model {
        let engine = doc.attr("CurrentEngine").and_then(|v| v.trim().parse().ok()).unwrap_or(0usize).min(ENGINES - 1);
        Model {
            doc,
            engine,
            pad: [0; ENGINES],
            bank: 0,
            playing: false,
            steps: [None; ENGINES],
            seq_steps: [-1; 16],
            name: None,
            editor_tab: 0,
            utility_tab: 0,
            layer: [0; ENGINES],
            samples: None,
            source: 0,
            sampler: SamplerView::default(),
            song_pos: None,
            host_loop: false,
            follow_playback: crate::options::Options::load().bool("PatternEditorFollow", false),
            all_layers: false,
            all_pads: false,
            engine_sel: 0,
            pattern_view: Default::default(),
            sampling: None,
            pattern_grown: false,
        }
    }

    pub fn engine_node(&self, e: usize) -> Option<&Node> {
        self.doc.child("Engine", &format!("Engine{e}"))
    }

    pub fn pad_node(&self, e: usize, p: usize) -> Option<&Node> {
        self.engine_node(e).and_then(|n| n.child("Pad", &format!("Pad{p}")))
    }

    pub fn engine_name(&self, e: usize) -> String {
        self.engine_node(e).and_then(|n| n.attr("Name")).map(str::to_string).unwrap_or_else(|| format!("Engine {}", e + 1))
    }

    pub fn engine_mute(&self, e: usize) -> bool {
        self.engine_node(e).is_some_and(|n| n.truthy("Mute"))
    }

    pub fn engine_solo(&self, e: usize) -> bool {
        self.engine_node(e).is_some_and(|n| n.truthy("Solo"))
    }

    pub fn tempo(&self) -> f64 {
        self.doc.child("TimeInfo", "TimeInfo").and_then(|t| t.attr("Tempo")).and_then(|v| v.trim().parse().ok()).unwrap_or(120.0)
    }

    pub fn pattern_power(&self, e: usize) -> bool {
        self.engine_node(e).and_then(|n| n.child("EngineState", "EngineState")).map(|s| s.attr("PatternPower").map(str::trim) != Some("false")).unwrap_or(true)
    }

    /// Apply an edit to the document as the engine does.
    pub fn apply(&mut self, c: &crate::Command) {
        match c {
            crate::Command::Message(m) => {
                // The audio thread lengthens a pattern only within the step
                // storage it has (it does not allocate).
                if m.kind == dreist_engine::binding::id::SET_PATTERN_STEP_COUNT && m.index[0] >= 0 {
                    let e = m.index[0] as usize;
                    let p = usize::try_from(m.index[3]).unwrap_or_else(|_| self.current_pattern(e));
                    self.pattern_grown |= m.value[0] as u32 > self.pattern_node(e, p).and_then(|n| n.u32("PatternSteps")).unwrap_or(16);
                }
                dreist_engine::engine::doc::mirror_message(&mut self.doc, m)
            }
            crate::Command::SongTracks(t) => dreist_engine::engine::doc::mirror_song_tracks(&mut self.doc, t),
            crate::Command::SwingTemplate(e, n, _) => dreist_engine::engine::doc::mirror_swing_template(&mut self.doc, *e, n),
            _ => {}
        }
    }

    pub fn pad_mode(&self, e: usize) -> dreist_engine::live::PadMode {
        dreist_engine::live::PadMode::from_name(self.engine_node(e).and_then(|n| n.attr("EnginePadMode")).unwrap_or("Normal"))
    }

    /// [re:e78221]; `None` when unset.
    pub fn pad_name(&self, e: usize, p: usize) -> Option<String> {
        let n = self.pad_node(e, p)?;
        let s = n.child("FixedString", "PadName")?.attr("string")?;
        (!s.is_empty()).then(|| s.to_string())
    }

    /// [re:d2541e]: no layer with a sample plays Gated / Looping
    /// (no pad counts as one-shot).
    pub fn pad_one_shot(&self, e: usize, p: usize) -> bool {
        !(0..LAYERS).any(|l| self.layer_node(e, p, l).is_some_and(|n| n.attr("AudioFileName").is_some_and(|f| !f.is_empty()) && matches!(n.attr("PlaybackMode"), Some("Gated" | "Looping"))))
    }

    pub fn pad_mute(&self, e: usize, p: usize) -> bool {
        self.pad_node(e, p).is_some_and(|n| n.truthy("Mute"))
    }

    pub fn pad_solo(&self, e: usize, p: usize) -> bool {
        self.pad_node(e, p).is_some_and(|n| n.truthy("Solo"))
    }

    pub fn layer_node(&self, e: usize, p: usize, l: usize) -> Option<&Node> {
        self.pad_node(e, p).and_then(|n| n.child("Layer", &format!("Layer{l}")))
    }

    /// Normalised layer parameter as [re:87acba] loads it.
    pub fn layer_param(&self, e: usize, p: usize, l: usize, param: dreist_formats::LayerParam) -> f32 {
        self.layer_node(e, p, l).and_then(|n| n.f32(param.name()).or_else(|| n.f32(param.enum_name()))).unwrap_or(param.default_value()).clamp(0.0, 1.0)
    }

    /// [re:4b2956], else the sample file stem.
    pub fn layer_name(&self, e: usize, p: usize, l: usize) -> Option<String> {
        let n = self.layer_node(e, p, l)?;
        if let Some(s) = n.child("FixedString", "LayerName").and_then(|c| c.attr("string")).filter(|s| !s.is_empty()) {
            return Some(s.to_string());
        }
        n.attr("AudioFileName").map(|f| f.replace('\\', "/")).and_then(|f| std::path::Path::new(&f).file_stem().map(|s| s.to_string_lossy().into_owned()))
    }

    /// Absolute path of the layer's sample.
    pub fn layer_sample(&self, e: usize, p: usize, l: usize) -> Option<std::path::PathBuf> {
        let f = self.layer_node(e, p, l)?.attr("AudioFileName")?;
        Some(dreist_formats::model::resolve_sample(self.samples.as_ref()?, f))
    }

    pub fn layer(&self, e: usize, p: usize, l: usize) -> LayerFlags {
        let Some(n) = self.pad_node(e, p).and_then(|n| n.child("Layer", &format!("Layer{l}"))) else { return LayerFlags::default() };
        LayerFlags { present: true, valid: n.attr("AudioFileName").is_some_and(|f| !f.is_empty()), mute: n.truthy("Mute"), solo: n.truthy("Solo") }
    }

    pub fn current_pattern(&self, e: usize) -> usize {
        self.engine_node(e).and_then(|n| n.child("EngineState", "EngineState")).and_then(|s| s.attr("CurrentPattern")).and_then(|v| v.trim().parse().ok()).unwrap_or(0)
    }

    pub fn pattern_node(&self, e: usize, p: usize) -> Option<&Node> {
        self.engine_node(e).and_then(|n| n.child("StepPattern", &format!("StepPattern{p}")))
    }

    /// The pattern's step length (`RESOLUTION_NAMES` index) without parsing
    /// its steps.
    pub fn pattern_resolution(&self, e: usize, p: usize) -> usize {
        self.pattern_node(e, p).and_then(|n| n.attr("Resolution")).map(dreist_engine::pattern::resolution_index).unwrap_or(8)
    }

    /// The pattern as the sequencer reads it (defaults filled in).
    pub fn pattern(&self, e: usize, p: usize) -> dreist_engine::pattern::PatternData {
        self.pattern_node(e, p).map(dreist_engine::pattern::PatternData::from_node).unwrap_or_else(dreist_engine::pattern::PatternData::empty)
    }

    pub fn pattern_name(&self, e: usize, p: usize) -> String {
        self.pattern_node(e, p).and_then(|n| n.children.iter().find(|c| c.name.as_deref() == Some("PatternName"))).and_then(|c| c.attr("string")).unwrap_or("").to_string()
    }

    /// Patterns that hold any step (for the selector keys).
    pub fn pattern_used(&self, e: usize, p: usize) -> bool {
        self.pattern_node(e, p).is_some_and(|n| n.children.iter().any(|t| t.children.iter().any(|s| s.attr("State").map(str::trim) == Some("true"))))
    }

    pub fn pad_gain(&self, e: usize, p: usize) -> f32 {
        self.pad_node(e, p).and_then(|n| n.f32("Gain")).unwrap_or(0.5)
    }

    /// Pads shown by the matrix: (first, count).
    pub fn visible_pads(&self) -> (usize, usize) {
        if self.bank < 4 { (self.bank * 16, 16) } else { (0, 64) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_take_appends_and_restarts() {
        let mut lt = LiveTake::default();
        let mut out = Vec::with_capacity(16);
        let l: Vec<f32> = (0..600).map(|i| if i < 256 { 0.5 } else { -0.25 }).collect();
        assert!(lt.poll(Some((&l[..300], &[])), &mut out), "first take starts the display");
        assert_eq!(out, vec![(0.0, 0.5)]);
        assert!(!lt.poll(Some((&l, &[])), &mut out));
        assert_eq!(out.len(), 2);
        assert_eq!(out[1], (-0.25, 0.0));
        assert!(!lt.poll(None, &mut out));
        assert!(lt.poll(Some((&l[..10], &[])), &mut out), "next take restarts");
    }
}
