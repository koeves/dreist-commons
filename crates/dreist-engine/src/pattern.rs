//! Step patterns as the sequencer reads them (`StepPattern`, `Track`, `Step`,
//! [re:9f001e]). See the dreist-re sequencer notes.

use dreist_formats::Node;
use dreist_formats::params::{STEP_LANES, step_lane_default, step_lane_index};

/// Number of step graph lanes (`Step::StepGraphItems`).
pub const LANES: usize = 93;
/// Tracks per pattern (= pads per engine).
pub const TRACKS: usize = 64;
/// Patterns per engine.
pub const PATTERNS: usize = 24;

/// Step lanes read by the sequencer.
pub mod lane {
    pub const VELOCITY: usize = 0;
    pub const REPEATS: usize = 2;
    pub const SHIFT: usize = 3;
    pub const DURATION: usize = 9;
    pub const LINK: usize = 10;
    pub const FORCE_LAYER: usize = 15;
    pub const PROBABILITY: usize = 16;
}

/// `StepPattern::Resolution`: names registered by `Start()` ([re:58734c],
/// `ReplaceResolution`), steps per 16th from table [re:660cc6].
pub const RESOLUTION_NAMES: [&str; 14] = ["1", "1T", "1/2", "1/2T", "1/4", "1/4T", "1/8", "1/8T", "1/16", "1/16T", "1/32", "1/32T", "1/64", "1/64T"];
const STEPS_PER_16TH: [f64; 14] = [0.0625, 0.09375, 0.125, 0.1875, 0.25, 0.375, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0];

/// [re:2f79c0] (table
/// [re:912fe4]): steps per beat for the pattern editor's Adaptive shading;
/// −1 for resolutions coarser than a beat, 4 past the table.
pub fn resolution_beats(resolution: usize) -> i32 {
    [-1, -1, -1, -1, 1, 1, 2, 3, 4, 6, 8, 12, 16, 24].get(resolution).copied().unwrap_or(4)
}

/// [re:06cfd1] (table
/// [re:e3bf70]): steps per shading group in Cyclic mode.
pub fn resolution_cycles(resolution: usize) -> i32 {
    [-1, -1, 2, 2, 4, 4, 8, 8, 16, 16, 32, 32, 64, 64].get(resolution).copied().unwrap_or(4)
}

/// Index into [`RESOLUTION_NAMES`]; unknown names map to 14, which reads as
/// 1.0 (`GetPatternResolutionFloatingPoint` falls back for indices > 13).
pub fn resolution_index(name: &str) -> usize {
    RESOLUTION_NAMES.iter().position(|n| *n == name).unwrap_or(14)
}

pub fn steps_per_16th(resolution: usize) -> f64 {
    STEPS_PER_16TH.get(resolution).copied().unwrap_or(1.0)
}

/// `StepPattern::SyncMode` (names from `Start()` `ReplaceSyncMode`; the legacy
/// names Sync/Trigger/Gate/Shot are also accepted, [re:33eee9]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    Slave,
    SlaveRestart,
    Gate,
    GateRestart,
}

/// `ReplaceSyncMode` names, by index.
pub const SYNC_MODE_NAMES: [&str; 4] = ["Slave", "Slave restart", "Gate", "Gate restart"];

impl SyncMode {
    pub fn from_index(i: u32) -> SyncMode {
        [SyncMode::Slave, SyncMode::SlaveRestart, SyncMode::Gate, SyncMode::GateRestart][(i as usize).min(3)]
    }

    pub fn from_name(s: &str) -> SyncMode {
        match s {
            "Slave restart" | "Trigger" => SyncMode::SlaveRestart,
            "Gate" => SyncMode::Gate,
            "Gate restart" | "Shot" => SyncMode::GateRestart,
            // "Slave", "Sync", and unknown (StringToSyncMode returns 4, treated as free-running).
            _ => SyncMode::Slave,
        }
    }

    pub fn index(self) -> u32 {
        self as u32
    }

    /// Restart modes (1, 3) resync the pattern to its start (`(mode | 2) == 3`).
    pub fn restarts(self) -> bool {
        self.index() | 2 == 3
    }

    /// Gate modes (2, 3) only run while held.
    pub fn gated(self) -> bool {
        self.index() & !1 == 2
    }
}

#[derive(Debug, Clone)]
pub struct StepData {
    pub on: bool,
    pub lanes: [f32; LANES],
}

impl StepData {
    /// A step that is off with every lane at its default.
    pub fn off() -> StepData {
        Self::off_ref().clone()
    }

    fn off_ref() -> &'static StepData {
        static OFF: std::sync::OnceLock<StepData> = std::sync::OnceLock::new();
        OFF.get_or_init(|| StepData { on: false, lanes: std::array::from_fn(step_lane_default) })
    }

    /// [re:164b7d]: the LinkToPrevious lane is set.
    pub fn tied(&self) -> bool {
        (self.lanes[lane::LINK] - 1.0).abs() < 1e-6
    }
}

#[derive(Debug, Clone)]
pub struct TrackData {
    pub steps: Vec<StepData>,
    /// `TrackLength` (+0x54): wraps the track before the pattern end.
    pub length: u32,
    /// `<Lane>Rand` per-lane randomisation (+0x70), default 0.5 = none.
    pub rand: [f32; LANES],
    pub mute: bool,
    pub solo: bool,
}

impl TrackData {
    pub fn empty(steps: u32) -> TrackData {
        TrackData { steps: vec![StepData::off(); steps as usize], length: steps, rand: [0.5; LANES], mute: false, solo: false }
    }

    /// Step `i`; steps past the stored ones are off (the original's
    /// tracks hold every step up to the maximum pattern length).
    pub fn step(&self, i: u32) -> &StepData {
        self.steps.get(i as usize).unwrap_or_else(|| StepData::off_ref())
    }
}

#[derive(Debug, Clone)]
pub struct PatternData {
    /// `PatternSteps` (+0x2b0).
    pub steps: u32,
    pub resolution: usize,
    pub sync: SyncMode,
    pub tracks: Vec<TrackData>,
    /// `MonoModeActive` (+0x2b4): one step per column (`MonoizeSteps`).
    pub mono: bool,
}

/// Hand `set` every attribute of `n` that `lane` maps to a step lane and
/// that parses as a number (the first attribute of each lane only, as
/// `Node::f32` reads it).
fn for_lanes(n: &Node, lane: impl Fn(&str) -> Option<usize>, mut set: impl FnMut(usize, f32)) {
    let mut seen = [false; STEP_LANES.len()];
    for (k, v) in &n.attrs {
        if let Some(l) = lane(k).filter(|l| !std::mem::replace(&mut seen[*l], true)) {
            if let Ok(v) = v.parse() {
                set(l, v);
            }
        }
    }
}

impl PatternData {
    pub fn empty() -> PatternData {
        PatternData { steps: 16, resolution: 8, sync: SyncMode::Slave, tracks: (0..TRACKS).map(|_| TrackData::empty(16)).collect(), mono: false }
    }

    /// [re:7c1f62] (load).
    pub fn from_node(n: &Node) -> PatternData {
        let steps = n.u32("PatternSteps").unwrap_or(16).max(1);
        let resolution = n.attr("Resolution").map(resolution_index).unwrap_or(8);
        let sync = n.attr("SyncMode").map(SyncMode::from_name).unwrap_or(SyncMode::Slave);
        let mut tracks: Vec<TrackData> = (0..TRACKS).map(|_| TrackData::empty(steps)).collect();
        for t in n.children_of("Track") {
            let Some(i) = t.index().map(|i| i as usize).filter(|i| *i < TRACKS) else { continue };
            let track = &mut tracks[i];
            track.length = t.u32("TrackLength").unwrap_or(steps);
            track.mute = t.bool("Mute").unwrap_or(false);
            track.solo = t.bool("Solo").unwrap_or(false);
            for_lanes(t, |name| name.strip_suffix("Rand").and_then(step_lane_index), |l, v| track.rand[l] = v);
            for s in t.children_of("Step") {
                let Some(k) = s.index().filter(|k| *k < steps) else { continue };
                let step = &mut track.steps[k as usize];
                step.on = s.bool("State").unwrap_or(false);
                for_lanes(s, step_lane_index, |l, v| step.lanes[l] = v);
            }
        }
        PatternData { steps, resolution, sync, tracks, mono: n.bool("MonoModeActive").unwrap_or(false) }
    }

    /// [re:992319]: in mono mode, turn off `step`
    /// on every other track; the new step takes the first one's Shift lane.
    pub fn monoize(&mut self, step: usize, track: usize) {
        if !self.mono {
            return;
        }
        let mut first = true;
        for t in 0..self.tracks.len() {
            if t == track || step >= self.tracks[t].steps.len() || !self.tracks[t].steps[step].on {
                continue;
            }
            if first {
                let shift = self.tracks[t].steps[step].lanes[lane::SHIFT];
                if let Some(s) = self.tracks[track].steps.get_mut(step) {
                    s.lanes[lane::SHIFT] = shift;
                }
                first = false;
            }
            self.tracks[t].steps[step].on = false;
        }
    }

    pub fn resolution(&self) -> f64 {
        steps_per_16th(self.resolution)
    }

    /// `GetPatternLengthIn16ths` [re:960cc4].
    pub fn length_16ths(&self) -> f64 {
        self.steps as f64 / self.resolution()
    }

    /// `GetPatternLengthInQN` [re:7eb86a].
    pub fn length_qn(&self) -> f64 {
        self.length_16ths() * 0.25
    }
}

/// [re:9f001e] (values at +0x18, 128 doubles; `NumberOfSteps` +0x418).
#[derive(Debug, Clone)]
pub struct Swing {
    pub steps: u32,
    pub values: Vec<f64>,
}

impl Default for Swing {
    fn default() -> Swing {
        Swing { steps: 16, values: vec![0.0; 128] }
    }
}

impl Swing {
    pub fn from_node(n: &Node) -> Swing {
        let mut s = Swing { steps: n.u32("NumberOfSteps").unwrap_or(16), ..Swing::default() };
        for (i, v) in s.values.iter_mut().enumerate() {
            if let Some(x) = n.attr(&format!("S{i}")).or_else(|| n.attr(&format!("Step{i}"))).and_then(|x| x.parse().ok()) {
                *v = x;
            }
        }
        s
    }

    /// [re:2a9c3d].
    pub fn value(&self, i: u32) -> f64 {
        self.values.get(i as usize).copied().unwrap_or(0.0)
    }
}

/// [re:d0ded4]: randomise a lane
/// value by the track's amount (`rand` returns 0..1).
pub fn randomised(value: f32, amount: f32, rand: &mut impl FnMut() -> f32) -> f32 {
    let q = (amount * 100.0 + if amount * 100.0 < 0.0 { -0.5 } else { 0.5 }) as i32 as f32;
    if (q * 0.01 - 0.5).abs() < 1e-6 {
        return value;
    }
    let s = q * 0.02 - 1.0;
    if s.abs() < 1e-6 {
        return value;
    }
    let r = rand() * s.abs();
    let v = if s <= 0.0 { value - r } else { r + value };
    v.clamp(0.0, 1.0)
}

/// [re:60d051] table [re:08ca81] (lane → style; 0 for
/// Velocity): 0 unipolar, 1 bipolar, 2 on/off, 3 stepped, 4 pitch.
pub const LANE_STYLE: [u8; 85] = [
    0, 1, 3, 1, 2, 4, 4, 1, 1, 0, 2, 1, 1, 0, 1, 3, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0,
    0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 3,
];

pub fn lane_style(lane: usize) -> u8 {
    LANE_STYLE.get(lane).copied().unwrap_or(0)
}

/// [`set_lane`] over a whole lane array (a voice's randomised copy).
pub fn snap_lanes(lanes: &mut [f32]) {
    for (l, v) in lanes.iter_mut().enumerate() {
        *v = v.clamp(0.0, 1.0);
        if lane_style(l) == 2 {
            *v = if *v < 0.5 { 0.0 } else { 1.0 };
        }
    }
    if lanes.get(lane::REPEATS).is_some_and(|v| *v != 0.0) {
        if let Some(v) = lanes.get_mut(lane::LINK) {
            *v = 0.0;
        }
    }
}

/// [re:85bc45]: clamp to 0..1, on/off lanes snap at
/// 0.5, and a non-zero Repeats clears the link to the previous step.
pub fn set_lane(step: &mut StepData, lane: usize, value: f32) {
    if lane >= LANES {
        return;
    }
    let mut v = value.clamp(0.0, 1.0);
    if lane_style(lane) == 2 {
        v = if v < 0.5 { 0.0 } else { 1.0 };
    }
    step.lanes[lane] = v;
    if lane == lane::REPEATS && v != 0.0 {
        step.lanes[lane::LINK] = 0.0;
    }
}

impl PatternData {
    /// [re:3c597a]: rotate the
    /// track's first `steps` steps by one.
    pub fn nudge_track(&mut self, track: usize, forward: bool) {
        let n = self.steps as usize;
        if let Some(t) = self.tracks.get_mut(track) {
            let n = n.min(t.steps.len());
            if n > 1 {
                if forward {
                    t.steps[..n].rotate_right(1);
                } else {
                    t.steps[..n].rotate_left(1);
                }
            }
        }
    }

    /// [re:40acf3].
    pub fn nudge(&mut self, forward: bool) {
        for t in 0..self.tracks.len() {
            self.nudge_track(t, forward);
        }
    }

    /// [re:9ba5fa] (one lane, or every lane when
    /// `lane` is None); `track` None = every track
    /// ([re:228ebc]).
    pub fn revert_lanes(&mut self, track: Option<usize>, lane: Option<usize>) {
        for (t, tr) in self.tracks.iter_mut().enumerate() {
            if track.is_some_and(|x| x != t) {
                continue;
            }
            for st in &mut tr.steps {
                for (k, v) in st.lanes.iter_mut().enumerate() {
                    if lane.is_none_or(|l| l == k) {
                        *v = step_lane_default(k);
                    }
                }
            }
        }
    }

    /// [re:693d89]: re-time the pattern to
    /// `steps` steps at `resolution`. Each step that is on lands at
    /// `(i + 2·Shift − 1)·oldStepQN` scaled by the new step length; the
    /// integer part picks the step (clamped), the fraction becomes Shift
    /// (`(f + 1) / 2`); a tie of k steps becomes `k·oldStepQN` worth of tied
    /// new steps, the last one's Duration the remaining fraction.
    pub fn convert_timing(&self, steps: u32, resolution: usize) -> PatternData {
        let steps = steps.clamp(1, 1024);
        let mut out = PatternData { steps, resolution, sync: self.sync, tracks: Vec::new(), mono: self.mono };
        let old_len = self.length_qn();
        let new_len = out.length_qn();
        let old_steps = self.steps.max(1) as usize;
        let old_step_qn = old_len / old_steps as f64;
        let ratio = new_len / old_len.max(1e-9);
        let per_qn = steps as f64 / new_len.max(1e-9);
        let n = steps as usize;
        out.tracks = self
            .tracks
            .iter()
            .map(|t| {
                let mut nt = TrackData::empty(steps);
                nt.rand = t.rand;
                nt.mute = t.mute;
                nt.solo = t.solo;
                let mut i = 0;
                while i < old_steps.min(t.steps.len()) {
                    let st = &t.steps[i];
                    if !st.on {
                        i += 1;
                        continue;
                    }
                    let tied = (i + 1..old_steps.min(t.steps.len())).take_while(|&k| t.steps[k].tied()).count();
                    let pos = ((2.0 * st.lanes[lane::SHIFT] as f64 - 1.0) * old_step_qn + i as f64 * old_step_qn) * ratio * per_qn;
                    let (int, frac) = (pos.trunc(), pos.fract());
                    let at = (int as i64).clamp(0, n as i64 - 1) as usize;
                    let mut ns = st.clone();
                    ns.on = true;
                    ns.lanes[lane::SHIFT] = ((frac + 1.0) * 0.5) as f32;
                    ns.lanes[lane::LINK] = 0.0;
                    nt.steps[at] = ns;
                    if tied > 0 {
                        let span = tied as f64 * ratio * old_step_qn * per_qn;
                        let (whole, part) = (span.trunc() as usize, span.fract());
                        let end = at + whole;
                        for k in at + 1..=end.min(n - 1) {
                            let mut ts = st.clone();
                            ts.on = true;
                            ts.lanes[lane::LINK] = 1.0;
                            ts.lanes[lane::DURATION] = if k < end { 1.0 } else { part as f32 };
                            nt.steps[k] = ts;
                        }
                        i += tied;
                    }
                    i += 1;
                }
                nt
            })
            .collect();
        out
    }

    /// [re:9e6de5].
    pub fn clear_track(&mut self, track: usize) {
        if let Some(t) = self.tracks.get_mut(track) {
            t.steps.iter_mut().for_each(|s| *s = StepData::off());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lane values and their `…Rand` amounts parse by name; unknown names,
    /// bad numbers and repeated attributes (the first one counts) are
    /// handled as `Node::f32` reads them.
    #[test]
    fn from_node_reads_lanes_by_name() {
        let attr = |n: &mut Node, k: &str, v: &str| n.attrs.push((k.into(), v.into()));
        let mut pat = Node::new("StepPattern", None);
        attr(&mut pat, "PatternSteps", "8");
        let mut t = Node::new("Track", Some("Track2"));
        attr(&mut t, "PanRand", "0.25");
        attr(&mut t, "VelocityRand", "x");
        attr(&mut t, "Rand", "1");
        let mut st = Node::new("Step", Some("Step3"));
        attr(&mut st, "State", "true");
        attr(&mut st, "Pan", "0.75");
        attr(&mut st, "Pan", "0.1");
        attr(&mut st, "Velocity", "bad");
        attr(&mut st, "Velocity", "0.2");
        attr(&mut st, "Nonsense", "0.5");
        t.children.push(st);
        pat.children.push(t);
        let p = PatternData::from_node(&pat);
        let tr = &p.tracks[2];
        let def = TrackData::empty(8);
        assert_eq!(tr.rand[1], 0.25);
        assert_eq!(tr.rand[0], def.rand[0]);
        let step = &tr.steps[3];
        assert!(step.on);
        assert_eq!(step.lanes[1], 0.75);
        assert_eq!(step.lanes[0], def.steps[3].lanes[0]);
        assert_eq!(&step.lanes[2..], &def.steps[3].lanes[2..]);
    }

    #[test]
    fn convert_timing_halves_and_keeps_offsets() {
        let mut p = PatternData::empty();
        p.steps = 16;
        p.resolution = resolution_index("1/16");
        for t in p.tracks.iter_mut() {
            *t = TrackData::empty(16);
        }
        p.tracks[0].steps[4].on = true;
        p.tracks[0].steps[6].on = true;
        p.tracks[0].steps[6].lanes[lane::SHIFT] = 0.75;
        let q = p.convert_timing(8, resolution_index("1/8"));
        assert_eq!(q.steps, 8);
        assert!(q.tracks[0].steps[2].on);
        assert!((q.tracks[0].steps[2].lanes[lane::SHIFT] - 0.5).abs() < 1e-6);
        // Step 6 a quarter step late → new step 3, a quarter of the way.
        assert!(q.tracks[0].steps[3].on);
        assert!((q.tracks[0].steps[3].lanes[lane::SHIFT] - 0.625).abs() < 1e-6, "{}", q.tracks[0].steps[3].lanes[lane::SHIFT]);
    }

    #[test]
    fn resolutions() {
        assert_eq!(steps_per_16th(resolution_index("1/16")), 1.0);
        assert_eq!(steps_per_16th(resolution_index("1/8T")), 0.75);
        assert_eq!(steps_per_16th(resolution_index("bogus")), 1.0);
        assert!(SyncMode::from_name("Slave restart").restarts());
        assert!(SyncMode::from_name("Gate").gated());
        assert!(!SyncMode::Slave.restarts() && !SyncMode::Slave.gated());
    }

    #[test]
    fn randomisation_is_identity_at_half() {
        let mut r = || 0.7;
        assert_eq!(randomised(0.3, 0.5, &mut r), 0.3);
        assert!((randomised(0.3, 1.0, &mut r) - 1.0).abs() < 1e-6);
        assert!((randomised(0.3, 0.0, &mut r) - 0.0).abs() < 1e-6);
        assert!((randomised(0.3, 0.75, &mut r) - 0.65).abs() < 1e-6);
    }

    #[test]
    fn snapped_lanes_follow_set_graph_status() {
        let mut l = [0.0f32; 93];
        l[4] = 0.62;
        l[lane::LINK] = 1.0;
        l[lane::REPEATS] = 0.3;
        l[lane::VELOCITY] = 1.4;
        snap_lanes(&mut l);
        assert_eq!(l[4], 1.0);
        assert_eq!(l[lane::LINK], 0.0);
        assert_eq!(l[lane::VELOCITY], 1.0);
        let mut l = [0.0f32; 93];
        l[4] = 0.49;
        snap_lanes(&mut l);
        assert_eq!(l[4], 0.0);
    }
}
