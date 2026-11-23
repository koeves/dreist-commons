//! The sampler's view for the editor: the panel's state from the audio
//! thread and the live take display's min/max pairs.

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
    /// (the display restarts). A new take first drops the previous take's
    /// pairs still in `out`, then adds its own.
    pub fn poll(&mut self, take: Option<(&[f32], &[f32])>, out: &mut Vec<(f32, f32)>) -> bool {
        let Some((l, r)) = take else {
            self.active = false;
            return false;
        };
        let reset = !self.active || l.len() < self.frames;
        if reset {
            self.frames = 0;
            self.active = true;
            out.clear();
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
