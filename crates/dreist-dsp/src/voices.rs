//! Voice allocation, stealing, cut groups and pad layer selection
//! ([re:9442f8] .., [re:bbda4a] .., [re:f32b6d]
//! [re:0b366c], [re:770f4e]).

/// Pool sizes from [re:3048a1] ([re:7f8d51], [re:1a803a]).
pub const NORMAL_VOICES: usize = 128;
pub const TIMESTRETCH_VOICES: usize = 16;

/// Fade length when a voice is cut by another ([re:afd305]).
pub const CUT_FADE: f64 = 250.0;

/// Layer cut mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutMode {
    NotCut,
    BySelf,
    ByPad,
    ByOtherPads,
    ByOtherPadsAndSelf,
    ByType,
    EngineGroup(u8),
    GlobalGroup(u8),
}

impl CutMode {
    pub fn from_index(i: u32) -> CutMode {
        match i {
            1 => CutMode::BySelf,
            2 => CutMode::ByPad,
            3 => CutMode::ByOtherPads,
            4 => CutMode::ByOtherPadsAndSelf,
            5 => CutMode::ByType,
            6 | 7 => CutMode::EngineGroup((i - 6) as u8),
            8..=15 => CutMode::GlobalGroup((i - 8) as u8),
            _ => CutMode::NotCut,
        }
    }

    /// A stored `CutMode`: the display name (the original rewrites
    /// the map [re:87acba] uses, so factory files say "Self + Other",
    /// "Pad type", …); enum names are accepted too, as older port builds
    /// wrote them. Unknown → NotCut.
    pub fn from_name(s: &str) -> CutMode {
        let s = s.trim();
        let i = CUT_MODE_LABELS.iter().position(|n| *n == s).or_else(|| CUT_MODE_NAMES.iter().position(|n| *n == s));
        CutMode::from_index(i.unwrap_or(0) as u32)
    }
}

/// `CutMode` enum names, by index.
pub const CUT_MODE_NAMES: [&str; 16] = [
    "NotCut", "BySelf", "ByPad", "ByOtherPads", "ByOtherPadsAndSelf", "ByType", "ByEngineGroup1", "ByEngineGroup2", "ByGlobalGroup1", "ByGlobalGroup2", "ByGlobalGroup3", "ByGlobalGroup4",
    "ByGlobalGroup5", "ByGlobalGroup6", "ByGlobalGroup7", "ByGlobalGroup8",
];

/// Display names ([re:ceabd7]).
pub const CUT_MODE_LABELS: [&str; 16] = [
    "Not cut", "Self", "Pad", "By other pads", "Self + Other", "Pad type", "Engine group 1", "Engine group 2", "Global group 1", "Global group 2", "Global group 3", "Global group 4",
    "Global group 5", "Global group 6", "Global group 7", "Global group 8",
];

/// Where a voice came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub engine: u32,
    pub pad: u32,
    pub layer: u32,
}

/// Voice cutting: does a new note on `new` (whose layer's cut mode
/// is `new_mode`) cut a playing voice at `old` whose layer's mode is `old_mode`?
pub fn is_cut(old: Slot, old_mode: CutMode, new: Slot, new_mode: CutMode) -> bool {
    let same_engine = old.engine == new.engine;
    match old_mode {
        CutMode::NotCut => false,
        CutMode::BySelf => same_engine && old.pad == new.pad && old.layer == new.layer,
        CutMode::ByPad => same_engine && old.pad == new.pad,
        CutMode::ByOtherPads => same_engine && old.pad != new.pad,
        CutMode::ByOtherPadsAndSelf => same_engine && (old.pad != new.pad || old.layer == new.layer),
        // Pads are grouped in rows of four; pads above 15 are never cut.
        CutMode::ByType => same_engine && old.pad < 16 && new.pad & !3 == old.pad & !3,
        CutMode::EngineGroup(_) => old_mode == new_mode && same_engine,
        CutMode::GlobalGroup(_) => old_mode == new_mode,
    }
}

/// [re:b9e7b0] strings (the `PadMode` attribute).
pub const LAYER_MODE_NAMES: [&str; 5] = ["All layers", "Velocity split", "Round robin", "Round robin reset", "Random"];

/// Pad layer mode ([re:b9e7b0]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerMode {
    AllLayers,
    VelocitySplit,
    RoundRobin,
    RoundRobinReset,
    Random,
}

/// A pad layer as [re:f32b6d] sees it.
#[derive(Debug, Clone, Copy)]
pub struct LayerSlot {
    /// Has a valid waveform.
    pub valid: bool,
    /// [re:30df67] lowest and highest velocity (inclusive, 0..=127).
    pub vel_low: u32,
    pub vel_high: u32,
}

/// The layers a note triggers (at most 8; no allocation on the audio
/// thread).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Picked {
    layers: [usize; 8],
    len: usize,
}

impl Picked {
    fn one(i: usize) -> Picked {
        let mut p = Picked::default();
        p.push(i);
        p
    }

    fn push(&mut self, i: usize) {
        if self.len < 8 {
            self.layers[self.len] = i;
            self.len += 1;
        }
    }
}

impl std::ops::Deref for Picked {
    type Target = [usize];
    fn deref(&self) -> &[usize] {
        &self.layers[..self.len]
    }
}

impl PartialEq<Vec<usize>> for Picked {
    fn eq(&self, o: &Vec<usize>) -> bool {
        **self == o[..]
    }
}

impl FromIterator<usize> for Picked {
    fn from_iter<I: IntoIterator<Item = usize>>(it: I) -> Picked {
        let mut p = Picked::default();
        for i in it {
            p.push(i);
        }
        p
    }
}

/// Pad layer selection state (the round-robin cursor).
#[derive(Debug, Clone, Copy, Default)]
pub struct LayerPicker {
    cursor: i32,
}

impl LayerPicker {
    pub fn new() -> LayerPicker {
        LayerPicker { cursor: -1 }
    }

    /// [re:f32b6d]. `forced` is the step's ForceLayer lane converted with
    /// `floor(8v) - 1` (`-1` = none); `random` supplies values in `0..1` for
    /// Random mode. Returns the layers to trigger.
    pub fn pick(&mut self, mode: LayerMode, layers: &[LayerSlot; 8], velocity: u32, forced: i32, allow_fallback: bool, mut random: impl FnMut() -> f32) -> Picked {
        if (0..8).contains(&forced) {
            if layers[forced as usize].valid {
                return Picked::one(forced as usize);
            }
            if !allow_fallback {
                return Picked::default();
            }
        }
        match mode {
            LayerMode::AllLayers => (0..8).filter(|&i| layers[i].valid).collect(),
            LayerMode::VelocitySplit => (0..8).filter(|&i| layers[i].valid && layers[i].vel_low <= velocity && velocity <= layers[i].vel_high).collect(),
            LayerMode::RoundRobin | LayerMode::RoundRobinReset => {
                let from = if self.cursor < 0 { 0 } else { self.cursor as usize + 1 };
                let Some(i) = (from..8).find(|&i| layers[i].valid).or_else(|| (0..from.min(8)).find(|&i| layers[i].valid)) else {
                    return Picked::default();
                };
                // The cursor stays only if a later valid layer exists; otherwise wrap.
                self.cursor = if (i + 1..8).any(|j| layers[j].valid) { i as i32 } else { -1 };
                Picked::one(i)
            }
            LayerMode::Random => {
                let mut tried = [false; 8];
                let mut n = 0;
                while n < 8 {
                    let k = ((random() as f64 * 7.99999).floor() as i32).clamp(0, 7) as usize;
                    if tried[k] {
                        continue;
                    }
                    if layers[k].valid {
                        return Picked::one(k);
                    }
                    tried[k] = true;
                    n += 1;
                }
                Picked::default()
            }
        }
    }
}

/// Voice lifecycle ([re:c07cc4]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Idle,
    /// Waiting for its sample-accurate start inside a block.
    Pending,
    Playing,
    /// Note finished; effect tails still running.
    Tail,
}

/// Bookkeeping for stealing: each voice gets an increasing id at note-on.
#[derive(Debug, Clone, Copy)]
pub struct VoiceInfo {
    pub state: VoiceState,
    pub id: u32,
}

/// [re:44749a]: prefer a voice already in its tail,
/// otherwise the oldest (largest `counter - id`, wrapping).
pub fn steal<I>(voices: I, counter: u32) -> Option<usize>
where
    I: IntoIterator<Item = VoiceInfo>,
    I::IntoIter: Clone,
{
    let voices = voices.into_iter();
    if let Some(i) = voices.clone().position(|v| v.state == VoiceState::Tail) {
        return Some(i);
    }
    let mut best = None;
    let mut age = 0u32;
    for (i, v) in voices.enumerate() {
        if v.state == VoiceState::Idle {
            continue;
        }
        let a = counter.wrapping_sub(v.id);
        if best.is_none() || age < a {
            best = Some(i);
            age = a;
        }
    }
    best
}

/// Silence threshold used to end effect tails (kill status [re:b401db]).
pub const TAIL_SILENCE: f32 = 5e-05;

#[cfg(test)]
mod tests {
    #[test]
    fn cut_modes_read_by_display_name() {
        // Factory content stores the display names.
        assert_eq!(CutMode::from_name("Self + Other"), CutMode::ByOtherPadsAndSelf);
        assert_eq!(CutMode::from_name("By other pads"), CutMode::ByOtherPads);
        assert_eq!(CutMode::from_name("Pad type"), CutMode::ByType);
        assert_eq!(CutMode::from_name("Global group 3"), CutMode::GlobalGroup(2));
        assert_eq!(CutMode::from_name("ByPad"), CutMode::ByPad);
        assert_eq!(CutMode::from_name("Not cut"), CutMode::NotCut);
        use crate::voice::PlaybackMode;
        assert_eq!(PlaybackMode::from_name("Length bound"), PlaybackMode::OneShotLengthBound);
        assert_eq!(PlaybackMode::from_name("OneShotLengthBound"), PlaybackMode::OneShotLengthBound);
        assert_eq!(PlaybackMode::from_name("Looping"), PlaybackMode::Looping);
    }

    use super::*;

    const S: fn(u32, u32, u32) -> Slot = |engine, pad, layer| Slot { engine, pad, layer };

    #[test]
    fn cut_modes() {
        let a = S(0, 1, 0);
        assert!(is_cut(a, CutMode::BySelf, S(0, 1, 0), CutMode::NotCut));
        assert!(!is_cut(a, CutMode::BySelf, S(0, 1, 1), CutMode::NotCut));
        assert!(is_cut(a, CutMode::ByPad, S(0, 1, 3), CutMode::NotCut));
        assert!(is_cut(a, CutMode::ByOtherPads, S(0, 2, 0), CutMode::NotCut));
        assert!(!is_cut(a, CutMode::ByOtherPads, S(0, 1, 0), CutMode::NotCut));
        assert!(is_cut(a, CutMode::ByType, S(0, 3, 0), CutMode::NotCut));
        assert!(!is_cut(a, CutMode::ByType, S(0, 4, 0), CutMode::NotCut));
        let g = CutMode::GlobalGroup(2);
        assert!(is_cut(a, g, S(5, 9, 0), g));
        assert!(!is_cut(a, g, S(5, 9, 0), CutMode::GlobalGroup(3)));
        let e = CutMode::EngineGroup(0);
        assert!(!is_cut(a, e, S(1, 9, 0), e));
    }

    #[test]
    fn layer_modes() {
        let l = |valid, lo, hi| LayerSlot { valid, vel_low: lo, vel_high: hi };
        let layers = [l(true, 0, 63), l(true, 64, 127), l(false, 0, 127), l(true, 0, 127), l(false, 0, 0), l(false, 0, 0), l(false, 0, 0), l(false, 0, 0)];
        let mut p = LayerPicker::new();
        assert_eq!(p.pick(LayerMode::AllLayers, &layers, 100, -1, false, || 0.0), vec![0, 1, 3]);
        assert_eq!(p.pick(LayerMode::VelocitySplit, &layers, 100, -1, false, || 0.0), vec![1, 3]);
        let rr: Vec<_> = (0..4).map(|_| p.pick(LayerMode::RoundRobin, &layers, 0, -1, false, || 0.0)[0]).collect();
        assert_eq!(rr, vec![0, 1, 3, 0]);
        assert_eq!(p.pick(LayerMode::AllLayers, &layers, 100, 1, false, || 0.0), vec![1]);
    }

    #[test]
    fn steals_tail_then_oldest() {
        let v = |state, id| VoiceInfo { state, id };
        let voices = [v(VoiceState::Playing, 5), v(VoiceState::Playing, 2), v(VoiceState::Playing, 9)];
        assert_eq!(steal(voices, 10), Some(1));
        let voices = [v(VoiceState::Playing, 1), v(VoiceState::Tail, 8)];
        assert_eq!(steal(voices, 10), Some(1));
    }
}
