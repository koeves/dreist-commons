//! MIDI input and live pad notes for [re:a71fe5] (the dreist-re live-input notes):
//! `processEvents` → MIDI event handling at the block start, the
//! [re:d662aa] queue dispatched at sub-block starts
//! (immediate handling), the note messages 0x3b–0x3e, note start
//! with note repeat, and [re:8ea21e].

use super::{Dreist, LAYERS, solo_pass};
use crate::binding::{Message, id};
use crate::live::{MapItem, PAD_NOTE_BASE, PadMode, SELECTED, pad_to_velocity, repeat_interval_qn, status_kind};
use crate::pattern::{StepData, TRACKS, lane, randomised};
use crate::sequencer::NoteRequest;
use dreist_dsp::voices::VoiceState;

/// A `VstMidiEvent`: delta frames from the block start and up to 4 data bytes
/// (byte 3 carries the semitone of re-injected note-repeat events).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiEvent {
    pub delta: u32,
    pub data: [u8; 4],
}

/// `processEvents`: at most 511 queued events.
const MAX_HOST_EVENTS: usize = 511;
/// [re:d3df17]: at most 1023 queued events.
const MAX_AUTOMATION: usize = 1023;

impl Dreist {
    /// Queue a host MIDI event for the next `process` call (`processEvents`
    /// [re:f0bff7]; kept stable-sorted by delta).
    pub fn push_midi(&mut self, delta: u32, data: [u8; 3]) {
        if self.midi_log.len() < 256 {
            self.midi_log.push(data);
        }
        if self.midi_in.len() < MAX_HOST_EVENTS {
            // Kept sorted by delta, arrival order among equal deltas.
            let at = self.midi_in.partition_point(|e| e.delta <= delta);
            self.midi_in.insert(at, MidiEvent { delta, data: [data[0], data[1], data[2], 0] });
        }
    }

    /// [re:ea55ac]: the engine "selected" map items and note repeat use.
    pub fn select_engine(&mut self, e: usize) {
        if e < self.engines.len() {
            self.selected_engine = e;
        }
    }

    pub fn selected_engine(&self) -> usize {
        self.selected_engine
    }

    /// `SelectPad`: the engine's current pad (chromatic/velocity modes play it).
    pub fn select_pad(&mut self, e: usize, pad: usize) {
        if let Some(eng) = self.engines.get_mut(e) {
            eng.state.current_pad = pad.min(TRACKS - 1);
        }
    }

    /// Note repeat controls (messages 0x1f / 0x20 / sync enable).
    pub fn set_note_repeat(&mut self, e: usize, enabled: bool, base: u32, synced: bool) {
        if let Some(eng) = self.engines.get_mut(e) {
            eng.state.note_repeat = enabled;
            eng.state.note_repeat_base = base;
            eng.state.note_repeat_synced = synced;
        }
    }

    pub fn set_pad_mode(&mut self, e: usize, mode: PadMode) {
        if let Some(eng) = self.engines.get_mut(e) {
            eng.pad_mode = mode;
        }
    }

    /// A pad hit from the editor/pads panel (message 0x3b).
    pub fn pad_hit(&mut self, e: usize, pad: usize, velocity: u32, delta: u32) {
        self.msg_pad_on(e, pad, velocity, delta as f64, None, true);
    }

    /// One layer of a pad from the editor's layer trigger: a live note on
    /// that layer only (no note repeat, linked pads or recording); the pad
    /// release ends it.
    pub fn layer_hit(&mut self, e: usize, pad: usize, layer: usize, velocity: u32, delta: u32) {
        if e >= self.engines.len() || pad >= TRACKS || layer >= 8 {
            return;
        }
        let pattern = self.engines[e].state.current_pattern;
        let req = NoteRequest { pad, step: -1, pattern, velocity: velocity.min(127), start: delta as f64, duration: -1.0, live: true, semitone: 0.0, record: None, layer: Some(layer) };
        self.note_on(e, &req);
    }

    /// A pad release from the editor (message 0x3d).
    pub fn pad_release(&mut self, e: usize, pad: usize, delta: u32) {
        self.msg_pad_off(e, pad, delta as f64, None);
    }

    /// Host MIDI events for frames 0..n, before the sub-block loop.
    pub(super) fn process_midi_in(&mut self, n: usize) {
        // Taken and put back: the queue keeps its capacity.
        let mut events = std::mem::take(&mut self.midi_in);
        if !events.is_empty() {
            self.meters.midi_received = true;
        }
        self.sub_start = 0;
        for mut ev in events.drain(..) {
            ev.delta = ev.delta.min(n.saturating_sub(1) as u32);
            self.midi_event(ev);
        }
        self.midi_in = events;
    }

    /// [re:6fdd0f].
    pub(super) fn automation_block_end(&self, start: u32, end: u32) -> u32 {
        match self.automation.iter().find(|e| e.delta > start) {
            Some(e) => e.delta.min(end),
            None => end,
        }
    }

    /// [re:97e4c2]: dispatch the events due at
    /// `start` (sorted queue, front first). A remaining event before `end`
    /// is a bad delta and clears the queue.
    pub(super) fn process_automation(&mut self, start: u32, end: u32) {
        self.sub_start = start;
        while self.automation.first().is_some_and(|e| e.delta == start) {
            let ev = self.automation.remove(0);
            self.immediate(ev);
        }
        if self.automation.iter().any(|e| e.delta < end) {
            self.automation.clear();
        }
    }

    /// [re:96931c]: rebase deltas; an
    /// undispatched event inside the block clears the queue.
    pub(super) fn automation_end_of_block(&mut self, n: u32) {
        if self.automation.iter().any(|e| e.delta < n) {
            self.automation.clear();
            return;
        }
        for e in &mut self.automation {
            e.delta -= n;
        }
    }

    fn queue_automation(&mut self, ev: MidiEvent) {
        if self.automation.len() < MAX_AUTOMATION {
            self.automation.push(ev);
            self.automation.sort_by_key(|e| e.delta);
        }
    }

    fn resolve_engine(&self, e: i32) -> Option<usize> {
        let e = if e == -1 || e == SELECTED { self.selected_engine as i32 } else { e };
        (0..self.engines.len() as i32).contains(&e).then_some(e as usize)
    }

    /// MIDI event [re:70eb56].
    fn midi_event(&mut self, ev: MidiEvent) {
        let ch = (ev.data[0] & 0x0f) as usize;
        let note = (ev.data[1] & 0x7f) as usize;
        let vel = ev.data[2] as i8 as i32;
        // [re:16a2b3] sees every event first.
        self.sampler.midi(status_kind(ev.data), ev.delta);
        let handled = match status_kind(ev.data) {
            0x90 => self.midi_note_on(ch, note, vel.max(0) as u32, ev.delta),
            0x80 => self.midi_note_off(ch, note, ev.delta),
            0xa0 | 0xb0 | 0xd0 | 0xe0 => false,
            _ => true,
        };
        if !handled {
            self.queue_automation(ev);
        }
    }

    /// Note-on [re:b7bf6c]; false = not handled here.
    fn midi_note_on(&mut self, ch: usize, note: usize, vel: u32, delta: u32) -> bool {
        let item = self.midi_map.get(ch, note);
        let (item_engine, item_pad, is_pad, is_scene) = match item {
            MapItem::Pad { engine, pad } => (engine, pad, true, false),
            MapItem::Scene { scene } => {
                self.scenes.add(scene, delta, true, Some(&self.scene_clock()));
                return true;
            }
            MapItem::Pattern { engine, .. } | MapItem::Function { engine, .. } => (engine, 0, false, false),
            MapItem::None => (-1, 0, false, false),
        };
        let _ = is_scene;
        let Some(e) = self.resolve_engine(item_engine) else { return false };
        let eng = &self.engines[e];
        let mut vel = vel;
        if eng.state.full_level {
            vel = eng.state.full_level_velocity;
        }
        let current = eng.state.current_pad;
        // [re:fb2eae]: a pad note (or any note of a chromatic engine) ends the
        // wait for MIDI.
        if is_pad || eng.pad_mode == PadMode::Chromatic {
            self.end_midi_wait();
        }
        let eng = &self.engines[e];
        match eng.pad_mode {
            PadMode::Chromatic => {
                // Chromatic engines treat every mapped note as a pad note.
                let semitone = note as f64 - PAD_NOTE_BASE as f64 - current as f64;
                self.msg_pad_on(e, current, vel, delta as f64, Some(semitone), true);
                true
            }
            _ if !is_pad => false,
            PadMode::MuteAndSolo => {
                // Pads 0-7 toggle engine mute, 8-15 engine solo (0x8c/0x8d).
                if item_pad < 16 {
                    let target = if item_pad < 8 { item_pad } else { item_pad - 8 };
                    if target < self.engines.len() {
                        if item_pad < 8 {
                            self.engines[target].mute = !self.engines[target].mute;
                        } else {
                            self.engines[target].solo = !self.engines[target].solo;
                        }
                        self.recompute_audible();
                    }
                }
                true
            }
            mode => {
                let mut pad = item_pad;
                if mode == PadMode::Velocity && !eng.state.full_level {
                    pad = current;
                    vel = pad_to_velocity(item_pad as u32, eng.state.visible_pads);
                }
                self.msg_pad_on(e, pad, vel, delta as f64, None, true);
                true
            }
        }
    }

    /// Note-off [re:61594e].
    fn midi_note_off(&mut self, ch: usize, note: usize, delta: u32) -> bool {
        let item = self.midi_map.get(ch, note);
        let item_engine = match item {
            MapItem::Pad { engine, .. } | MapItem::Pattern { engine, .. } | MapItem::Function { engine, .. } => engine,
            MapItem::Scene { .. } | MapItem::None => -1,
        };
        let Some(e) = self.resolve_engine(item_engine) else { return false };
        let current = self.engines[e].state.current_pad;
        if self.engines[e].pad_mode == PadMode::Chromatic {
            let semitone = note as f64 - PAD_NOTE_BASE as f64 - current as f64;
            self.msg_pad_off(e, current, delta as f64, Some(semitone));
            return true;
        }
        match item {
            MapItem::Scene { scene } => {
                self.scenes.add(scene, delta, false, Some(&self.scene_clock()));
                true
            }
            MapItem::Pad { pad, .. } => {
                let pad = if self.engines[e].pad_mode == PadMode::Velocity { current } else { pad };
                self.msg_pad_off(e, pad, delta as f64, None);
                true
            }
            _ => false,
        }
    }

    /// Immediate MIDI event [re:c14865].
    fn immediate(&mut self, ev: MidiEvent) {
        let ch = (ev.data[0] & 0x0f) as usize;
        let note = (ev.data[1] & 0x7f) as usize;
        match status_kind(ev.data) {
            0x90 => self.note_on_immediate(ch, note, ev.data[3] as i8 as i32),
            0x80 => self.note_off_immediate(ch, note),
            // MIDI CC [re:9147a8]: BindingMap (MIDI learn itself is an
            // editor feature), then CC1 → mod wheel source ([re:c19c30]).
            0xb0 => {
                self.last_cc = (self.last_cc.0.wrapping_add(1), ch as u8, note as u8);
                let mut msgs = std::mem::take(&mut self.messages);
                let from = msgs.len();
                self.bindings.process_midi_cc(ch, note, ev.data[2] as i8 as f32 * 0.007874016, &mut msgs);
                self.messages = msgs;
                self.echo_automation(from);
                if ev.data[1] == 1 {
                    self.host.values[crate::modulation::src::MOD_WHEEL] = ev.data[2] as i8 as f32 * 0.007874016;
                }
            }
            // Pitch bend → pitch wheel = 14-bit/16383.
            0xe0 => {
                let v = ((ev.data[2] & 0x7f) as u32) << 7 | (ev.data[1] & 0x7f) as u32;
                self.host.values[crate::modulation::src::PITCH_WHEEL] = v as f32 * 6.103888e-5;
            }
            // Channel pressure → channel pressure = v/127.
            0xd0 => {
                self.host.values[crate::modulation::src::CHANNEL_PRESS] = ev.data[1] as i8 as f32 * 0.007874016;
            }
            _ => {}
        }
    }

    /// Immediate note-on [re:54d829] (`extra` = data byte 3).
    fn note_on_immediate(&mut self, ch: usize, note: usize, extra: i32) {
        let item = self.midi_map.get(ch, note);
        let engine = match item {
            MapItem::None | MapItem::Scene { .. } => return,
            MapItem::Pad { engine, .. } | MapItem::Pattern { engine, .. } | MapItem::Function { engine, .. } => engine,
        };
        let Some(e) = self.resolve_engine(engine) else { return };
        match item {
            MapItem::Function { function, .. } => self.function(e, function, true),
            MapItem::Pattern { pattern, .. } => {
                self.select_pattern(e, pattern, false);
                self.msg_phase(e, None, false);
            }
            MapItem::Pad { pad, .. } => {
                // Note-repeat retrigger for the selected engine while the pad is held.
                let st = &self.engines[e].state;
                if e != self.selected_engine || !st.note_repeat || !st.held.get(pad).copied().unwrap_or(false) {
                    return;
                }
                let vel = st.last_velocity[pad];
                self.do_start_note(e, pad, vel, self.sub_start as f64, extra as f64, true);
            }
            _ => {}
        }
    }

    /// Immediate note-off [re:b9bf40].
    fn note_off_immediate(&mut self, ch: usize, note: usize) {
        let item = self.midi_map.get(ch, note);
        let engine = match item {
            MapItem::Pattern { engine, .. } | MapItem::Function { engine, .. } => engine,
            _ => return,
        };
        if engine == -1 {
            return;
        }
        let Some(e) = self.resolve_engine(engine) else { return };
        match item {
            MapItem::Function { function, .. } => self.function(e, function, false),
            MapItem::Pattern { .. } => self.msg_stop(e, false),
            _ => {}
        }
    }

    /// Mapped functions (immediate path): 3 note repeat and 4 note erase follow
    /// the note; 1/2/5 are editor-only, 6 play toggle, 7 record toggle and 8
    /// (message 0x123) belong to the transport/recording host.
    fn function(&mut self, e: usize, function: u32, on: bool) {
        match function {
            3 => self.engines[e].state.note_repeat = on,
            4 => self.engines[e].state.note_erase = on,
            // 6 play toggle (0x16) and 7 record toggle (0x14) on the press;
            // 8 momentary 0x123 (live erase on the engine).
            6 if on => {
                let m = Message::new(id::SET_PLAYBACK_STATE, &[], &[if self.is_playing() { 0.0 } else { 1.0 }]);
                self.apply_message(&m);
            }
            7 if on => {
                let m = Message::new(id::TOGGLE_RECORD, &[], &[if self.recording { 0.0 } else { 1.0 }]);
                self.apply_message(&m);
            }
            8 => {
                let m = Message::new(id::SONG_SET_ERASE_STATE, &[e as i32], &[on as i32 as f32]);
                self.apply_message(&m);
            }
            _ => {}
        }
    }

    /// Messages 0x3b/0x3c (transport messages): `semitone` is the 0x3c
    /// value (chromatic MIDI notes); `play` is 0x3b's third value.
    fn msg_pad_on(&mut self, e: usize, pad: usize, vel: u32, delta: f64, semitone: Option<f64>, play: bool) {
        let Some(eng) = self.engines.get(e) else { return };
        let mode = eng.pad_mode;
        let mut pad = pad;
        if mode == PadMode::X0x {
            // Transport message [re:33e7fc]: x0x mode edits the current
            // pad's track at step = pad (within the pattern's steps), cycling
            // off → on (velocity 0.5) → accent (1.0) → off; an on step below
            // 0.5 becomes 0.5. Posted as 0xd2 [vel, 1.0, false] / 0xd3. The
            // current pad then sounds only for 0x3b with its play flag.
            let (track, cur) = (eng.state.current_pad, eng.state.current_pattern);
            let step = eng
                .patterns
                .get(cur)
                .filter(|p| pad < p.steps as usize)
                .map(|p| p.tracks.get(track).map(|t| (t.step(pad as u32).on, t.step(pad as u32).lanes[lane::VELOCITY])).unwrap_or((false, 0.0)));
            if let Some((on, v)) = step {
                let idx = [e as i32, track as i32, -1, cur as i32, track as i32, pad as i32];
                let m = if !on || v < 0.5 {
                    Message::new(id::SET_STEP_ON, &idx, &[0.5, 1.0, 0.0])
                } else if v < 1.0 {
                    Message::new(id::SET_STEP_ON, &idx, &[1.0, 1.0, 0.0])
                } else {
                    Message::new(id::SET_STEP_OFF, &idx, &[0.0, 0.0])
                };
                self.apply_message(&m);
                self.doc_step(e, cur, track, pad);
            }
            pad = self.engines[e].state.current_pad;
            if !(play && semitone.is_none()) {
                return;
            }
        }
        let eng = &self.engines[e];
        let semi = if mode == PadMode::Chromatic { semitone.unwrap_or(pad as f64 - eng.state.current_pad as f64) } else { 0.0 };
        let vel = if eng.state.full_level && mode == PadMode::Normal { eng.state.full_level_velocity } else { vel };
        self.do_start_note(e, pad, vel, delta, semi, false);
    }

    /// Messages 0x3d/0x3e.
    fn msg_pad_off(&mut self, e: usize, pad: usize, delta: f64, semitone: Option<f64>) {
        let Some(eng) = self.engines.get(e) else { return };
        let current = eng.state.current_pad;
        let (pad, semi) = match eng.pad_mode {
            PadMode::Chromatic => (current, semitone.unwrap_or(pad as f64 - current as f64)),
            PadMode::Velocity => (current, 0.0),
            _ => (pad, 0.0),
        };
        self.note_off(e, pad, delta, semi);
        if let Some(h) = self.engines[e].state.held.get_mut(pad) {
            *h = false;
        }
        // [re:f8a9d2](pad + 36, channel e): stop repeats.
        let note = PAD_NOTE_BASE as usize + pad;
        self.automation.retain(|ev| !(matches!(status_kind(ev.data), 0x80 | 0x90) && (ev.data[0] & 0x0f) as usize == e && (ev.data[1] & 0x7f) as usize == note));
    }

    /// Note start [re:0d62e7].
    fn do_start_note(&mut self, e: usize, pad: usize, vel: u32, delay: f64, semitone: f64, immediate: bool) {
        let (sr, bpm, ppq) = (self.sr as f64, self.bpm, self.ppq);
        let sub = self.sub_start as f64;
        let selected = self.selected_engine;
        let eng = &self.engines[e];
        let st = &eng.state;
        let vel = if st.full_level { st.full_level_velocity } else { vel };
        let mode = eng.pad_mode;
        let pad = if matches!(mode, PadMode::Chromatic | PadMode::Velocity) { st.current_pad } else { pad };
        let mut delay = delay;
        if e == selected && st.note_repeat && mode.index() <= 2 {
            let base = repeat_interval_qn(st.note_repeat_base);
            let spb = sr * 60.0 / bpm;
            let t;
            if immediate {
                t = delay;
            } else {
                if st.note_repeat_synced {
                    let q = (((delay / spb + ppq) / base + 0.99999).floor() * base - ppq) * spb;
                    delay = q.max(0.0);
                }
                t = sub + delay;
                if st.note_repeat_synced {
                    delay += sub;
                }
            }
            let at = ((spb * base).ceil() + t) as i64 as u32;
            let data = [0x90 | e as u8, PAD_NOTE_BASE + pad as u8, vel as u8, semitone as i32 as i8 as u8];
            self.queue_automation(MidiEvent { delta: at, data });
        }
        self.start_voice_live(e, pad, vel, delay, semitone);
        if !immediate {
            // [re:fea720].
            let st = &mut self.engines[e].state;
            if pad < TRACKS {
                st.held[pad] = true;
                st.last_velocity[pad] = vel;
                st.last_pitch[pad] = semitone;
            }
            self.follow_note(e, pad);
        }
    }

    /// Note start [re:3c8245]: selection follows a live note while the
    /// sampler is idle or recording for the selected engine: the engine
    /// becomes the selected one (`EngineSelectionFollowsMIDI`), then the
    /// pad the selected engine's current one (`PadSelectionFollowsMIDI`;
    /// not in x0x mode). The editor hears of both.
    fn follow_note(&mut self, e: usize, pad: usize) {
        let sampler_free = self.sampler.state == crate::sampler::State::Idle || self.sampler.target_engine == Some(self.selected_engine);
        if !sampler_free || pad >= TRACKS {
            return;
        }
        if self.engine_follow && e != self.selected_engine {
            self.select_engine(e);
            self.post_editor(Message::new(id::UPDATE_ENGINE_SELECTION_CHANGED, &[e as i32], &[]));
        }
        let eng = &self.engines[e];
        if self.pad_follow && e == self.selected_engine && eng.pad_mode != PadMode::X0x && eng.state.current_pad != pad {
            self.select_pad(e, pad);
            self.post_editor(Message::new(id::UPDATE_PAD_SELECTION_CHANGED, &[e as i32, pad as i32], &[]));
        }
    }

    /// Delayed voice start [re:4e8b59] + [re:927da0]
    /// (live): no mute/solo check, the given velocity, held until note-off.
    /// While playing with pattern power on the step is the recording step
    /// (kept with the current limited step by
    /// step playback) and its Repeats lane gives the repeat count;
    /// otherwise step −1 and the default step's repeats.
    fn start_voice_live(&mut self, e: usize, pad: usize, vel: u32, delay: f64, semitone: f64) {
        if pad >= TRACKS {
            return;
        }
        let (host, bpm) = (&self.host, self.bpm as f32);
        for m in self.mods.iter_mut().flatten() {
            m.note_on(e, pad, host, bpm);
        }
        let record = self.record_phase(e, delay);
        let eng = &self.engines[e];
        let current_pattern = eng.state.current_pattern;
        let rec_step = record.map(|_| eng.state.current_limited_step as i32);
        let repeats = match eng.patterns.get(current_pattern) {
            Some(p) => {
                let track = &p.tracks[pad];
                let mut rand = || self.rng.next_f32();
                let step = match rec_step {
                    Some(k) if (k as usize) < track.steps.len() => track.steps[k as usize].clone(),
                    _ => StepData::off(),
                };
                (randomised(step.lanes[lane::REPEATS], track.rand[lane::REPEATS], &mut rand) * 7.0 + 1.0).floor() as u32
            }
            None => 1,
        };
        if repeats == 0 {
            return;
        }
        // Linked pads as a bit mask (no allocation on the audio thread).
        let links = (0..TRACKS).filter(|&j| j != pad && self.engines[e].links[pad][j]).fold(0u64, |m, j| m | 1 << j);
        for i in 0..repeats {
            let mut start = -(i as f64);
            if i == 0 {
                start += delay.max(0.0);
            }
            let req = |p| NoteRequest { pad: p, step: rec_step.unwrap_or(-1), pattern: current_pattern, velocity: vel, start, duration: -1.0, live: true, semitone, record, layer: None };
            self.note_on(e, &req(pad));
            for j in (0..TRACKS).filter(|&j| links & 1 << j != 0) {
                self.note_on(e, &req(j));
            }
        }
    }

    /// Recording phase (delayed voice start): `x = delay·res·mult/spb + ppq −
    /// trigger`, wrapped into the pattern, `d` = offset inside the step (QN),
    /// phase `2d`, minus 0.5 when `floor(200d) > 25` (kept as in the original).
    fn record_phase(&self, e: usize, delay: f64) -> Option<f64> {
        let eng = &self.engines[e];
        if !self.playing || !eng.state.pattern_power {
            return None;
        }
        let p = eng.patterns.get(eng.state.current_pattern)?;
        let mult = eng.timing.tempo_multiplier;
        let spqn = self.sr as f64 * 60.0 / self.bpm;
        let mut x = delay * p.resolution() * mult / spqn + self.ppq - eng.state.trigger_ppq;
        let len = p.length_16ths() * 0.25;
        if len <= 0.0 {
            return None;
        }
        if x < 0.0 {
            x += (1.0 - (x / len).floor()) * len;
        }
        let d = (x % (mult * len)) % (len / p.steps.max(1) as f64);
        let mut phase = d + d;
        if ((d * 200.0) as f32).floor() > 25.0 {
            phase -= 0.5;
        }
        Some(phase)
    }

    /// [re:524f03] → [re:9f4897]: release the
    /// pad's voices `delta` samples into their note (−1 = now); chromatic
    /// engines match the voice's semitone.
    fn note_off(&mut self, e: usize, pad: usize, delta: f64, semitone: f64) {
        let chromatic = self.engines.get(e).is_some_and(|x| x.pad_mode == PadMode::Chromatic);
        for v in &mut self.voices {
            if !matches!(v.state, VoiceState::Pending | VoiceState::Playing) {
                continue;
            }
            if v.slot.engine as usize != e || v.slot.pad as usize != pad {
                continue;
            }
            if chromatic && (semitone - v.semitone).abs() >= 1e-6 {
                continue;
            }
            if delta == -1.0 {
                if v.state == VoiceState::Playing {
                    v.voice.release();
                } else {
                    v.duration = 0.0;
                }
            } else {
                v.duration = delta;
            }
        }
    }

    /// Mute/solo changes: recompute each layer's audibility as at load.
    pub(super) fn recompute_audible(&mut self) {
        let solo_eng = self.engines.iter().any(|x| x.solo);
        for eng in &mut self.engines {
            let ea = solo_pass(solo_eng, eng.solo, eng.mute);
            let solo_pad = eng.pads.iter().any(|p| p.solo);
            for p in &mut eng.pads {
                let pa = solo_pass(solo_pad, p.solo, p.mute);
                let solo_layer = p.layers.iter().any(|l| l.solo);
                for l in p.layers.iter_mut().take(LAYERS) {
                    l.audible = ea && pa && solo_pass(solo_layer, l.solo, l.mute);
                }
            }
        }
    }

    pub(super) fn scene_clock(&self) -> crate::scene::SceneClock {
        crate::scene::SceneClock { ppq: self.ppq, samples_per_beat: self.sr as f64 * 60.0 / self.bpm, beats_per_bar: self.beats_per_bar() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderOptions;
    use std::path::PathBuf;

    fn demo() -> Option<Dreist> {
        let content = dreist_formats::legacy::content_dir().to_string_lossy().into_owned();
        let p = PathBuf::from(content).join(dreist_formats::legacy::DEMO_FACTORY).join("Presets/Blister.geist");
        let mut g = Dreist::load(&p, 44100.0, RenderOptions::default()).ok()?;
        g.start();
        // Silence the song and patterns so only live notes play.
        g.song.active = false;
        for e in &mut g.engines {
            e.state.pattern_power = false;
            e.state.gate = false;
        }
        Some(g)
    }

    fn first_pad_with_sample(g: &Dreist, e: usize) -> Option<usize> {
        g.engines[e].pads.iter().position(|p| p.layers.iter().any(|l| l.sample.is_some()))
    }

    #[test]
    fn midi_note_starts_and_releases_a_live_voice() {
        let Some(mut g) = demo() else { return };
        let Some(pad) = first_pad_with_sample(&g, 0) else { return };
        g.push_midi(100, [0x90, 36 + pad as u8, 100]);
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        g.process(&mut l, &mut r);
        let live: Vec<_> = g.voices.iter().filter(|v| v.live && v.state != VoiceState::Idle).collect();
        assert!(!live.is_empty());
        assert!(live.iter().all(|v| v.slot.pad as usize == pad && v.duration == -1.0));
        assert!(g.engines[0].state.held[pad]);
        // Started at sample 100 (+ pre-delay): silent before it.
        assert!(l[..100].iter().all(|x| x.abs() < 1e-6), "{:?}", l[..100].iter().fold(0.0f32, |a, x| a.max(x.abs())));
        assert!(l[100..].iter().any(|x| x.abs() > 1e-4));
        g.note_off(0, pad, 10.0, 0.0);
        assert!(g.voices.iter().filter(|v| v.live && v.state == VoiceState::Playing).all(|v| v.duration == 10.0));
        g.push_midi(10, [0x80, 36 + pad as u8, 0]);
        g.process(&mut l, &mut r);
        assert!(!g.engines[0].state.held[pad]);
    }

    #[test]
    fn recording_writes_steps_while_playing() {
        let Some(mut g) = demo() else { return };
        let Some(pad) = first_pad_with_sample(&g, 0) else { return };
        // Engine 0 runs its pattern (song off); clear the target track.
        g.engines[0].state.pattern_power = true;
        g.engines[0].state.gate = true;
        let pat = g.engines[0].state.current_pattern;
        g.engines[0].patterns[pat].tracks[pad].steps.iter_mut().for_each(|s| s.on = false);
        g.set_record(true);
        g.set_quantize(true);
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        // A few blocks in, so the sequencer has a current step.
        for _ in 0..20 {
            g.process(&mut l, &mut r);
        }
        let step = g.engines[0].state.current_limited_step as usize;
        g.push_midi(0, [0x90, 36 + pad as u8, 64]);
        g.process(&mut l, &mut r);
        let s = &g.engines[0].patterns[pat].tracks[pad].steps[step];
        assert!(s.on, "step {step} recorded");
        assert!((s.lanes[0] - 64.0 / 127.0).abs() < 1e-6);
        assert_eq!(s.lanes[3], 0.5);
        // Not armed → nothing new is written.
        g.set_record(false);
        g.engines[0].patterns[pat].tracks[pad].steps.iter_mut().for_each(|s| s.on = false);
        g.push_midi(0, [0x90, 36 + pad as u8, 64]);
        g.process(&mut l, &mut r);
        assert!(g.engines[0].patterns[pat].tracks[pad].steps.iter().all(|s| !s.on));
    }

    #[test]
    fn held_gated_notes_record_duration_and_ties() {
        let Some(mut g) = demo() else { return };
        let Some(pad) = first_pad_with_sample(&g, 0) else { return };
        g.engines[0].state.pattern_power = true;
        g.engines[0].state.gate = true;
        g.engines[0].one_shot[pad] = false;
        let pat = g.engines[0].state.current_pattern;
        g.engines[0].patterns[pat].tracks[pad].steps.iter_mut().for_each(|s| s.on = false);
        g.set_record(true);
        g.set_quantize(true);
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        for _ in 0..20 {
            g.process(&mut l, &mut r);
        }
        let steps = g.engines[0].patterns[pat].steps as usize;
        let start = g.engines[0].state.current_limited_step as usize;
        g.push_midi(0, [0x90, 36 + pad as u8, 100]);
        g.process(&mut l, &mut r);
        let sps = g.dur_recs.first().map(|d| d.sps).expect("duration recording started");
        // Hold for about 1.5 steps, then release.
        let blocks = ((sps * 1.5) / 256.0).ceil() as usize;
        for _ in 0..blocks {
            g.process(&mut l, &mut r);
        }
        g.push_midi(0, [0x80, 36 + pad as u8, 0]);
        g.process(&mut l, &mut r);
        assert!(g.dur_recs.is_empty());
        let track = &g.engines[0].patterns[pat].tracks[pad];
        let next = (start + 1).min(steps - 1);
        assert!(track.steps[start].on);
        assert!(track.steps[next].on && track.steps[next].tied(), "next step tied");
        let d = track.steps[next].lanes[crate::pattern::lane::DURATION];
        assert!(d > 0.0 && d < 1.0, "partial duration {d}");
    }

    #[test]
    fn midi_transmod_sources() {
        let Some(mut g) = demo() else { return };
        g.push_midi(0, [0xb0, 1, 127]);
        g.push_midi(10, [0xe0, 0x7f, 0x7f]);
        g.push_midi(20, [0xd0, 64, 0]);
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        g.process(&mut l, &mut r);
        use crate::modulation::src;
        assert!((g.host.values[src::MOD_WHEEL] - 1.0).abs() < 1e-6);
        assert!((g.host.values[src::PITCH_WHEEL] - 1.0).abs() < 1e-4);
        assert!((g.host.values[src::CHANNEL_PRESS] - 64.0 / 127.0).abs() < 1e-6);
    }

    #[test]
    fn synced_note_repeat_retriggers_on_the_grid() {
        let Some(mut g) = demo() else { return };
        let Some(pad) = first_pad_with_sample(&g, 0) else { return };
        g.select_engine(0);
        g.set_note_repeat(0, true, 8, true);
        let spb = 44100.0 * 60.0 / g.bpm;
        let block = 512usize;
        g.push_midi(0, [0x90, 36 + pad as u8, 90]);
        let mut starts = 0;
        let mut seen = std::collections::HashSet::new();
        let (mut l, mut r) = (vec![0.0; block], vec![0.0; block]);
        let blocks = (spb * 1.0 / block as f64).ceil() as usize; // one beat
        for _ in 0..blocks {
            g.process(&mut l, &mut r);
            for v in g.voices.iter().filter(|v| v.live && v.state != VoiceState::Idle) {
                if seen.insert(v.id) {
                    starts += 1;
                }
            }
        }
        // 1/16 repeats over one beat: the first hit plus 3 retriggers (the 4th
        // lands exactly on the next beat).
        assert!((4..=5).contains(&starts), "starts {starts}");
        assert!(g.automation.iter().any(|e| e.data[1] == 36 + pad as u8));
        g.push_midi(0, [0x80, 36 + pad as u8, 0]);
        g.process(&mut l, &mut r);
        assert!(g.automation.is_empty());
    }
}
