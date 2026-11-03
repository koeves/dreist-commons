//! [re:61255b] ([re:18b132] ..): turns the current pattern of each engine into
//! note-on requests, sample-accurate within a sub-block. See
//! the dreist-re sequencer notes.

use crate::pattern::{LANES, PatternData, Swing, TRACKS, lane, randomised};

/// `EngineState` fields the sequencer and song/scene logic use.
#[derive(Debug, Clone)]
pub struct EngineState {
    pub current_pattern: usize,
    pub pattern_power: bool,
    /// `IsGateEnabled` (+0x20): the pattern is running.
    pub gate: bool,
    pub needs_resync: bool,
    /// Pattern phase to resync to, in 16ths; -999 = none (+0x18).
    pub phase_resync: f64,
    /// Offset added to the song position, in 16ths.
    pub process_offset: f64,
    pub last_play_position: f64,
    pub last_16ths: i32,
    pub current_limited_step: u32,
    pub current_step: [u32; TRACKS],
    /// Track has wrapped at its own length (+[re:de14a7]).
    pub track_wrapped: [bool; TRACKS],
    pub pad_suppression: [i32; TRACKS],
    pub song_triggered: bool,
    /// Set when the pattern wrapped this block ([re:644196]).
    pub wrapped: bool,
    /// Live-play state ([re:7fedc7]; the dreist-re live-input notes).
    pub full_level: bool,
    pub full_level_velocity: u32,
    pub note_repeat: bool,
    pub note_repeat_synced: bool,
    /// `NoteRepeatBase` (+0x50, not saved; default 8 = 1/16).
    pub note_repeat_base: u32,
    pub note_erase: bool,
    pub visible_pads: u32,
    /// `padSelAnchor` (+[re:4bd108]): the current pad.
    pub current_pad: usize,
    /// [re:b98fa6] note held / last velocity / last pitch.
    pub held: [bool; TRACKS],
    pub last_velocity: [u32; TRACKS],
    pub last_pitch: [f64; TRACKS],
    /// `PatternTriggerPPQPos`: where the current pattern started (QN).
    pub trigger_ppq: f64,
    /// [re:b98fa6] graph record queue (+0x34, 93 ints, −1 = empty; dirty
    /// flag +0x1a8): the last recorded value·127 per lane, written into the
    /// next step the track reaches (`ReadAndPlayStep` [re:9496b8]).
    pub graph_rec: Vec<[i32; LANES]>,
    pub graph_rec_dirty: [bool; TRACKS],
    /// Queued values that reached a step this block: (track, step, lane,
    /// value), applied by the engine.
    pub graph_writes: Vec<(usize, u32, usize, f32)>,
}

pub const NO_PHASE: f64 = -999.0;

impl Default for EngineState {
    fn default() -> EngineState {
        EngineState {
            current_pattern: 0,
            pattern_power: true,
            gate: false,
            needs_resync: false,
            phase_resync: NO_PHASE,
            process_offset: 0.0,
            last_play_position: -1.0,
            last_16ths: -1,
            current_limited_step: 0,
            current_step: [0; TRACKS],
            track_wrapped: [false; TRACKS],
            pad_suppression: [0; TRACKS],
            song_triggered: false,
            wrapped: false,
            full_level: false,
            full_level_velocity: 127,
            note_repeat: false,
            note_repeat_synced: true,
            note_repeat_base: 8,
            note_erase: false,
            visible_pads: 0,
            current_pad: 0,
            held: [false; TRACKS],
            last_velocity: [0; TRACKS],
            last_pitch: [0.0; TRACKS],
            trigger_ppq: 0.0,
            graph_rec: vec![[-1; LANES]; TRACKS],
            graph_rec_dirty: [false; TRACKS],
            graph_writes: Vec::with_capacity(64),
        }
    }
}

impl EngineState {
    pub fn needs_phase_resync(&self) -> bool {
        self.phase_resync != NO_PHASE
    }

    /// [re:26ffc6].
    pub fn set_graph_record(&mut self, pad: usize, lane: usize, cc: i32) {
        let (Some(q), Some(d)) = (self.graph_rec.get_mut(pad), self.graph_rec_dirty.get_mut(pad)) else { return };
        if let Some(v) = q.get_mut(lane) {
            *v = cc;
        }
        *d = q.iter().any(|&v| v != -1);
    }

    /// [re:232cf6].
    pub fn clear_graph_record(&mut self) {
        self.graph_rec.fill([-1; LANES]);
        self.graph_rec_dirty = [false; TRACKS];
    }

    /// [re:231524] state reset.
    pub fn reset_playback(&mut self) {
        self.clear_graph_record();
        self.process_offset = 0.0;
        self.last_play_position = -1.0;
        self.last_16ths = -1;
        self.current_limited_step = 0;
        self.current_step = [0; TRACKS];
        self.track_wrapped = [false; TRACKS];
    }

    /// Restart the pattern from its first step (sync-restart handling).
    pub fn restart_steps(&mut self) {
        self.current_limited_step = 0;
        self.current_step = [0; TRACKS];
        self.track_wrapped = [false; TRACKS];
    }
}

/// Per-engine values the sequencer reads from `Engine`.
#[derive(Debug, Clone)]
pub struct EngineTiming {
    /// `TempoNumerator / TempoDenominator` (`GetTempoMultiplierFloat`).
    pub tempo_multiplier: f64,
    /// `TempoNumerator` / `TempoDenominator` (1..16).
    pub tempo_num: u32,
    pub tempo_den: u32,
    pub swing: Swing,
    /// `SwingTemplateDepth` (+0xab8).
    pub swing_depth: f32,
}

impl Default for EngineTiming {
    fn default() -> EngineTiming {
        EngineTiming { tempo_multiplier: 1.0, tempo_num: 1, tempo_den: 1, swing: Swing::default(), swing_depth: 0.0 }
    }
}

/// Host clock ([re:5cc709] at [re:c23aaa]).
#[derive(Debug, Clone, Copy)]
pub struct Clock {
    pub bpm: f64,
    pub sample_rate: f32,
    /// Song position at the start of the host block, in quarter notes (+0x30).
    pub ppq_block: f64,
    /// Song position at the start of the current sub-block (+0x38).
    pub ppq_now: f64,
}

impl Clock {
    pub fn samples_per_beat(&self) -> f64 {
        self.sample_rate as f64 * 60.0 / self.bpm
    }
}

/// One note the sequencer starts ([re:6eade4] arguments).
#[derive(Debug, Clone, PartialEq)]
pub struct NoteRequest {
    pub pad: usize,
    pub step: i32,
    pub pattern: usize,
    /// 0..=127, used for layer selection and the velocity transmod source.
    pub velocity: u32,
    /// Start delay from the sub-block start, samples.
    pub start: f64,
    /// Note length counted from the note start, samples (-1 = until note-off).
    pub duration: f64,
    /// Live/MIDI note ([re:6eade4] live flag): the step lanes come
    /// from the default step and `velocity` is the MIDI velocity.
    pub live: bool,
    /// Pitch offset in semitones (chromatic pad mode).
    pub semitone: f64,
    /// Live-recording timing offset (`StartVoiceDelayed`), when recording applies.
    pub record: Option<f64>,
}

/// Is the track audible given mute/solo (`GetTrackPlaybackStatusFromMuteSolo` [re:60b812]).
pub fn track_plays(pattern: &PatternData, track: usize) -> bool {
    let any_solo = pattern.tracks.iter().any(|t| t.solo);
    let t = &pattern.tracks[track];
    if any_solo { t.solo } else { !t.mute }
}

pub struct PadInfo<'a> {
    /// [re:d2541e]: no valid layer with a gated/looping playback mode.
    pub one_shot: &'a [bool; TRACKS],
    /// `Pad + 0x380`: pads linked to each pad.
    pub links: &'a [[bool; TRACKS]; TRACKS],
}

/// Random sources: `rand_white` is `Randomiser::GetWhite(0, 1)` (step probability),
/// `rand_lane` feeds `Convertor` randomisation ([re:5a24f7]).
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng { state: seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1 }
    }

    pub fn next_f32(&mut self) -> f32 {
        // xorshift64*
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let v = self.state.wrapping_mul(0x2545_f491_4f6c_dd1d);
        ((v >> 40) as f32 / (1u64 << 24) as f32).clamp(0.0, 1.0)
    }
}

fn round_half_away(x: f64) -> i32 {
    (x + if x < 0.0 { -0.5 } else { 0.5 }) as i32
}

/// [re:459579] for one engine and one sub-block
/// (`start`, `len` in samples from the host block start).
pub fn process_engine(
    state: &mut EngineState,
    pattern: Option<&PatternData>,
    timing: &EngineTiming,
    clock: &Clock,
    start: f64,
    len: f64,
    pads: &PadInfo,
    rng: &mut Rng,
    out: &mut Vec<NoteRequest>,
    triggers: &mut Vec<usize>,
    metronome: Option<&mut Option<(i32, i32)>>,
) {
    let mult_bpm = timing.tempo_multiplier * clock.bpm;
    let to_16ths = mult_bpm * 4.0;
    let spb = clock.sample_rate as f64 / (mult_bpm * 0.06666666666666667);
    let p0 = start * (1.0 / spb) + clock.ppq_block * to_16ths * (1.0 / clock.bpm);
    let p1 = (len - 1.0) * (1.0 / spb) + p0;
    let sixteenth = round_half_away(p0);
    // [re:7f14b3] for the metronome (selected) engine on a new
    // 16th, `−spb·(p0 − n16)` samples ahead ([re:3ab244] / [re:82b42a]).
    if let Some(click) = metronome {
        if state.last_16ths != sixteenth {
            *click = Some((sixteenth, (-(spb * (p0 - sixteenth as f64))) as i32));
        }
    }

    if !state.pattern_power {
        state.last_16ths = sixteenth;
        state.last_play_position = p1;
        return;
    }
    if state.last_16ths != sixteenth {
        state.last_16ths = sixteenth;
        for s in state.pad_suppression.iter_mut() {
            if *s > 0 {
                *s -= 1;
            }
        }
    }
    let last = state.last_play_position;
    let mut wstart = if last < 0.0 { p0 } else { last };
    if p1 < wstart {
        wstart = p0;
    }
    if state.needs_phase_resync() {
        state.process_offset = match pattern {
            None => 0.0,
            Some(p) => state.phase_resync - (to_16ths * clock.ppq_now * (1.0 / clock.bpm)).rem_euclid_c(p.length_16ths()),
        };
        state.phase_resync = NO_PHASE;
        state.needs_resync = false;
    }
    if state.needs_resync {
        let restarts = pattern.is_some_and(|p| p.sync.restarts());
        state.process_offset = if restarts { -wstart } else { 0.0 };
        state.needs_resync = false;
    }
    read_and_play_step(state, pattern, timing, spb, wstart, p1, pads, rng, out, triggers);
    state.last_play_position = p1;
}

trait CFmod {
    fn rem_euclid_c(self, m: f64) -> f64;
}

impl CFmod for f64 {
    /// C `fmod` (sign of the dividend).
    fn rem_euclid_c(self, m: f64) -> f64 {
        self % m
    }
}

/// [re:45c2fa].
#[allow(clippy::too_many_arguments)]
fn read_and_play_step(
    state: &mut EngineState,
    pattern: Option<&PatternData>,
    timing: &EngineTiming,
    spb: f64,
    wstart: f64,
    wend: f64,
    pads: &PadInfo,
    rng: &mut Rng,
    out: &mut Vec<NoteRequest>,
    triggers: &mut Vec<usize>,
) {
    let Some(pattern) = pattern else { return };
    state.wrapped = false;
    let r = pattern.resolution();
    let offset = state.process_offset;
    let pos = (offset + wstart) * r;
    let i0 = round_half_away(pos);
    let end_rel = (wend + offset) * r - i0 as f64;
    let n = pattern.steps as i32;
    let samples_per_step = spb / r;
    let frac = (wstart * r - i0 as f64) + offset * r;
    let swing_steps = timing.swing.steps.max(1);

    for k in -1..=1 {
        // Absolute step index (not wrapped by the pattern length) and pattern step.
        // 32-bit wrapping as in the binary (extreme positions saturate `i0`).
        let mut abs = i0.wrapping_add(k - 1).wrapping_sub(n);
        if abs < 0 {
            // The binary adds `n` until non-negative.
            abs = abs.rem_euclid(n);
        }
        let pstep = i0.wrapping_add(k).rem_euclid(n) as u32;
        let mut moved = false;
        if k == 0 && state.current_limited_step != pstep {
            moved = true;
            if (pstep as i32) < state.current_limited_step as i32 {
                state.wrapped = true;
                // [re:39400a]: the graph record queue does not carry over a wrap.
                state.clear_graph_record();
            }
            state.current_limited_step = pstep;
            for t in 0..TRACKS {
                let tr = &pattern.tracks[t];
                state.current_step[t] = if tr.length < pattern.steps && tr.length > 0 {
                    let len = tr.length as i32;
                    if state.track_wrapped[t] || (len <= abs && len <= pstep as i32) { abs.rem_euclid(len) as u32 } else { pstep }
                } else {
                    pstep
                };
            }
        }
        let swing = timing.swing.value(pstep % swing_steps);
        for t in 0..TRACKS {
            let track = &pattern.tracks[t];
            let mut sidx = pstep;
            if track.length < pattern.steps && track.length > 0 {
                let len = track.length as i32;
                if !state.track_wrapped[t] {
                    if len <= abs && len <= pstep as i32 {
                        state.track_wrapped[t] = true;
                        sidx = abs.rem_euclid(len) as u32;
                    }
                } else {
                    sidx = abs.rem_euclid(len) as u32;
                }
            }
            if moved && state.graph_rec_dirty[t] {
                for l in 0..LANES {
                    let cc = state.graph_rec[t][l];
                    if cc != -1 {
                        state.graph_writes.push((t, sidx, l, cc as f32 * (1.0 / 127.0)));
                    }
                }
                state.graph_rec[t] = [-1; LANES];
                state.graph_rec_dirty[t] = false;
            }
            let step = track.step(sidx);
            let at = (step.lanes[lane::SHIFT] + step.lanes[lane::SHIFT] - 1.0) as f64 + swing * timing.swing_depth as f64 + k as f64;
            let tied_continuation = sidx > 0 && step.tied() && track.step(sidx - 1).on && !pads.one_shot[t];
            if at < end_rel && frac <= at && !tied_continuation && step.on && state.pad_suppression[t] < 1 && state.gate {
                let p = step.lanes[lane::PROBABILITY];
                let w = rng.next_f32().clamp(0.0, 1.0);
                if (p - 1.0).abs() > 1e-6 && p * 0.99 + 0.01 < w {
                    continue;
                }
                let delay = ((at - frac) * samples_per_step).max(0.0);
                start_voice(state, pattern, t, sidx as i32, samples_per_step, delay, pads, rng, out);
                triggers.push(t);
            }
        }
    }
}

/// [re:927da0] for a sequenced step.
#[allow(clippy::too_many_arguments)]
fn start_voice(state: &EngineState, pattern: &PatternData, pad: usize, step_index: i32, samples_per_step: f64, delay: f64, pads: &PadInfo, rng: &mut Rng, out: &mut Vec<NoteRequest>) {
    if !track_plays(pattern, pad) {
        return;
    }
    let track = &pattern.tracks[pad];
    let step = track.step(step_index as u32);
    let mut rand = || rng.next_f32();
    let velocity = (randomised(step.lanes[lane::VELOCITY], track.rand[lane::VELOCITY], &mut rand) * 127.0) as u32;
    let repeats = (randomised(step.lanes[lane::REPEATS], track.rand[lane::REPEATS], &mut rand) * 7.0 + 1.0).floor() as u32;
    if repeats == 0 {
        return;
    }
    let interval = if repeats < 2 { -1.0 } else { samples_per_step / repeats as f64 };
    let chain = repeats == 1;
    for i in 0..repeats {
        let mut start = i as f64 * interval;
        if i == 0 {
            start += delay;
        }
        let mut duration = start + samples_per_step;
        if chain {
            // Follow LinkToPrevious steps after this one.
            let mut last = step_index;
            let n = pattern.steps as i32;
            while last + 1 < n {
                let s = track.step((last + 1) as u32);
                if !(s.on && s.tied()) {
                    break;
                }
                last += 1;
            }
            let extra = last - step_index;
            duration = if extra == 0 { step.lanes[lane::DURATION] as f64 } else { track.step(last as u32).lanes[lane::DURATION] as f64 + extra as f64 } * samples_per_step;
        }
        let req = |pad| NoteRequest { pad, step: step_index, pattern: state.current_pattern, velocity, start, duration, live: false, semitone: 0.0, record: None };
        out.push(req(pad));
        for (j, linked) in pads.links[pad].iter().enumerate() {
            if *linked && j != pad {
                out.push(req(j));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::{PatternData, StepData};

    fn four_on_floor() -> PatternData {
        let mut p = PatternData::empty();
        for s in [0, 4, 8, 12] {
            p.tracks[0].steps[s] = StepData { on: true, ..StepData::off() };
        }
        p
    }

    fn run(p: &PatternData, blocks: usize, block: usize) -> Vec<(usize, f64)> {
        let mut st = EngineState { gate: true, ..EngineState::default() };
        st.needs_resync = true;
        let timing = EngineTiming::default();
        let one_shot = [true; TRACKS];
        let links = [[false; TRACKS]; TRACKS];
        let pads = PadInfo { one_shot: &one_shot, links: &links };
        let mut rng = Rng::new(1);
        let sr = 48000.0;
        let bpm = 120.0;
        let mut hits = Vec::new();
        let mut ppq = 0.0;
        for b in 0..blocks {
            let clock = Clock { bpm, sample_rate: sr, ppq_block: ppq, ppq_now: ppq };
            let mut out = Vec::new();
            process_engine(&mut st, Some(p), &timing, &clock, 0.0, block as f64, &pads, &mut rng, &mut out, &mut Vec::new(), None);
            for n in out {
                hits.push((n.pad, (b * block) as f64 + n.start));
            }
            ppq += block as f64 * bpm / 60.0 / sr as f64;
        }
        hits
    }

    #[test]
    fn quarter_notes_land_on_beats() {
        // 120 BPM at 48 kHz: one beat = 24000 samples, one 16th = 6000.
        let hits = run(&four_on_floor(), 2000, 32);
        let times: Vec<f64> = hits.iter().map(|h| h.1).collect();
        // After the first block the window starts at the previous block's last
        // sample while delays apply from the block start: notes land 1 sample late.
        let want = [0.0, 24001.0, 48001.0];
        assert_eq!(times.len(), want.len());
        for (t, w) in times.iter().zip(want) {
            assert!((t - w).abs() < 1.01, "{times:?}");
        }
    }

    #[test]
    fn repeats_split_the_step() {
        let mut p = PatternData::empty();
        p.tracks[3].steps[0] = StepData { on: true, ..StepData::off() };
        p.tracks[3].steps[0].lanes[lane::REPEATS] = 3.0 / 7.0; // floor(3 + 1) = 4 hits
        let hits = run(&p, 200, 32);
        assert_eq!(hits.len(), 4);
        assert!((hits[1].1 - hits[0].1 - 1500.0).abs() < 1.01);
    }
}
