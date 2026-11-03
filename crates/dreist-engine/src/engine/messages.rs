//! Synth-side message handling ([re:0152f9] and the
//! per-area handlers) for the messages bindings and hosts send. Messages
//! posted during a block are applied at the start of the next one (the
//! synth message queue). See the dreist-re bindings notes.

use super::{Dreist, ENGINES, LAYERS, Route};
use crate::binding::{Message, id};
use crate::fx::FxTarget;
use crate::modulation::src;
use dreist_dsp::voice::PlaybackMode;
use dreist_dsp::voice::{LAYER_PARAMS, SOURCES};
use dreist_dsp::voices::VoiceState;
use dreist_formats::{EngineParam, PadParam};

fn clamp_param(src: usize, v: f32) -> f32 {
    if src == 0 { v.clamp(0.0, 1.0) } else { v.clamp(-1.0, 1.0) }
}

impl Dreist {
    /// Queue a message for the next block (to the synth).
    pub fn post_message(&mut self, m: Message) {
        self.messages.push(m);
    }

    /// `effSetParameter` → [re:9ff1dc].
    pub fn set_host_parameter(&mut self, index: usize, value: f32) {
        let mut out = std::mem::take(&mut self.messages);
        let from = out.len();
        self.bindings.process_host(index, value, &mut out);
        self.messages = out;
        self.echo_automation(from);
    }

    /// Binding output (MIDI CC / host automation) since `from` in the queue,
    /// for the editor too: the original's editor reads the shared model, so
    /// automated values show on the controls and are saved. Kept to the
    /// latest value per target (`automation_out`, drained by the host).
    pub(super) fn echo_automation(&mut self, from: usize) {
        for i in from..self.messages.len() {
            let m = &self.messages[i];
            if m.editor {
                continue;
            }
            let out = &mut self.automation_out;
            if let Some(o) = out.iter_mut().find(|o| o.same_target(m)) {
                o.value = m.value;
            } else if out.len() < 256 {
                out.push(m.clone());
            }
        }
    }

    /// Apply the queued messages (block start). The queue keeps its
    /// capacity (no allocation on the next post).
    pub(super) fn apply_messages(&mut self) {
        let mut msgs = std::mem::take(&mut self.messages);
        for m in &msgs {
            self.apply_message(m);
        }
        msgs.clear();
        // Anything posted while applying stays queued.
        msgs.append(&mut self.messages);
        self.messages = msgs;
    }

    /// One message (subset relevant to audio; editor-only types are ignored).
    pub fn apply_message(&mut self, m: &Message) {
        // A NaN value means nothing; applied, it would poison the audio.
        if m.value.iter().any(|v| v.is_nan()) {
            return;
        }
        // Editor-only bindings: handed to the editor (not applied).
        if m.editor {
            self.post_editor(m.clone());
            return;
        }
        self.apply_message_inner(m);
        self.doc_apply(m);
    }

    /// A message for the editor.
    pub(super) fn post_editor(&mut self, mut m: Message) {
        if self.editor_messages.len() < 256 {
            m.editor = true;
            self.editor_messages.push(m);
        }
    }

    fn apply_message_inner(&mut self, m: &Message) {
        let i = |k: usize| m.index[k];
        let v = |k: usize| m.value[k];
        let vi = |k: usize| m.value[k] as i32;
        let vb = |k: usize| m.value[k] >= 0.5;
        let e = i(0);
        match m.kind {
            id::SET_MACRO_VALUE => {
                if (0..4).contains(&e) {
                    self.host.values[src::MACRO1 + e as usize] = v(0).clamp(0.0, 1.0);
                }
            }
            id::SET_TEMPO => {
                if v(0) > 0.0 {
                    self.bpm = v(0).clamp(20.0, 999.0) as f64;
                }
            }
            // `e_setPlaybackState` 0x16 (bindings, MIDI function 6): the
            // internal transport (a host transport overrides it next block).
            id::SET_PLAYBACK_STATE => {
                if vb(0) && !self.is_playing() {
                    self.play();
                } else if !vb(0) && self.is_playing() {
                    self.stop();
                }
            }
            // `e_togglePlayback` 0x15: the requested state is the opposite of
            // the current one.
            id::TOGGLE_PLAYBACK => {
                if self.is_playing() {
                    self.stop();
                } else {
                    self.play();
                }
            }
            id::PANIC => self.panic(),
            id::TAP_TEMPO => self.tap_tempo(v(0)),
            id::RETURN_TO_ZERO => self.return_to_zero(),
            // `e_registryOptionsChanged` [ForcedLayersFallThrough,
            // SendActiveSensing] ([re:5e0672] stores the
            // first in [re:b2a80f]; active sensing does nothing in
            // the plugin: its option update is empty).
            id::REGISTRY_OPTIONS_CHANGED => self.forced_fallthrough = vb(0),
            // `e_registryMIDIOptionsChanged` [send notes, send CCs].
            id::REGISTRY_MIDI_OPTIONS_CHANGED => {
                self.send_notes = vb(0);
                self.send_ccs = vb(1);
            }
            id::TOGGLE_RECORD => {
                // Transport message 0x14: with the song active, record
                // arms / disarms song recording.
                self.recording = vb(0);
                if self.song.active {
                    let pos = self.song_position();
                    if vb(0) {
                        self.songrec.arm(self.playing, pos);
                    } else {
                        self.songrec.disarm(pos);
                    }
                }
            }
            id::TOGGLE_QUANTIZE => self.quantize = vb(0),
            id::SET_METRONOME_LEVEL => self.metronome.gain = v(0).clamp(0.0, 1.0),
            // Header Options menu ([re:4139f8] / [re:d718a1]):
            // metronome output (an output location index), pre-roll length
            // 0..3, pre-roll only when recording, wait for MIDI, metronome
            // only when recording ([re:781091]).
            id::SET_METRONOME_OUTPUT_LOCATION => self.metronome.route = Route::from_index(vi(0).max(0) as usize),
            id::SET_PREROLL_MODE => self.preroll_mode = vi(0).clamp(0, 3) as u32,
            id::SET_PREROLL_ONLY_ON_REC => self.preroll_only_rec = vb(0),
            id::SET_REC_WAITS_FOR_MIDI => self.wait_midi = vb(0),
            id::SET_METRO_ONLY_DURING_REC => self.metro_only_rec = vb(0),
            // `e_registryFollowOptionsChanged` [pad, engine] ([re:7de83e]).
            id::REGISTRY_FOLLOW_OPTIONS_CHANGED => {
                self.pad_follow = vb(0);
                self.engine_follow = vb(1);
            }
            id::TOGGLE_METRONOME => self.metronome.muted = !vb(0),
            id::TUNE_LAYER => self.tune_layer(e, i(1), i(2)),
            id::SET_LAYER_PARAMETER => self.set_layer_param(e, i(1), i(2), vi(0), vi(2), v(1)),
            id::SET_PAD_PARAMETER => self.set_pad_param(e, i(1), vi(1), vi(2), v(0)),
            id::SET_ENGINE_PARAMETER => self.set_engine_param(e, vi(1), vi(2), v(0)),
            id::SET_LAYER_SEND_DEPTH => {
                if let Some(l) = self.layer_mut(e, i(1), i(2)) {
                    set_row(l.aux.get_mut(vi(0) as usize), vi(2), v(1));
                }
            }
            id::SET_PAD_SEND_LEVEL => {
                if let Some(p) = self.pad_mut(e, i(1)) {
                    set_row(p.aux.get_mut(vi(0) as usize), vi(2), v(1));
                }
            }
            id::SET_ENGINE_SEND_LEVEL => {
                if let Some(x) = self.engines.get_mut(e as usize) {
                    set_row(x.aux.get_mut(vi(0) as usize), vi(2), v(1));
                }
            }
            // Output routing (mixer OutputCombo): v0 = output location index.
            id::SET_PAD_OUTPUT | id::SET_LAYER_OUTPUT | id::SET_ENGINE_OUTPUT | id::SET_GLOBAL_AUX_OUTPUT => {
                let r = super::Route::from_index(vi(0).max(0) as usize);
                match m.kind {
                    id::SET_PAD_OUTPUT => {
                        if let Some(x) = self.pad_mut(e, i(1)) {
                            x.route = r;
                        }
                    }
                    id::SET_LAYER_OUTPUT => {
                        if let Some(x) = self.layer_mut(e, i(1), i(2)) {
                            x.route = r;
                        }
                    }
                    id::SET_ENGINE_OUTPUT => {
                        if let Some(x) = usize::try_from(e).ok().and_then(|e| self.engines.get_mut(e)) {
                            x.route = r;
                        }
                    }
                    _ => {
                        if let Some(x) = usize::try_from(e).ok().and_then(|e| self.aux.get_mut(e)) {
                            x.route = r;
                        }
                    }
                }
            }
            // Global aux message 0xa3: [re:892cf2] (`IsPreFade`).
            id::SET_GLOBAL_AUX_PRE_POST => {
                if let Some(a) = usize::try_from(e).ok().and_then(|e| self.aux.get_mut(e)) {
                    a.pre_fade = vb(0);
                }
            }
            id::SET_GLOBAL_AUX_GAIN => {
                if let Some(a) = self.aux.get_mut(e as usize) {
                    let pan = a.mix.pan;
                    a.mix.set(v(0), pan);
                }
            }
            id::SET_MASTER_CHANNEL_GAIN => {
                let pan = self.master.pan;
                self.master.set(v(0), pan);
            }
            // [re:6df259] lambdas [re:7b3363] / [re:a6489a] / [re:522167]:
            // values [slot, on]; indices [engine, pad] / none / [engine].
            id::SET_MODULATION_SOURCE_TRIGGER | id::SET_MODULATION_SOURCE_TRANSPORT_START | id::SET_MODULATION_SOURCE_PATTERN_CHANGE => {
                if let Some(Some(md)) = self.mods.get_mut(vi(0).max(0) as usize) {
                    if vi(0) >= 0 {
                        match m.kind {
                            id::SET_MODULATION_SOURCE_TRIGGER => {
                                if let (Ok(en), Ok(p)) = (usize::try_from(e), usize::try_from(i(1))) {
                                    if en < 8 && p < 64 {
                                        md.pad_trig[en][p] = vb(1);
                                    }
                                }
                            }
                            id::SET_MODULATION_SOURCE_TRANSPORT_START => md.reset_at_start = vb(1),
                            _ => {
                                if let Some(t) = usize::try_from(e).ok().and_then(|en| md.eng_trig.get_mut(en)) {
                                    *t = vb(1);
                                }
                            }
                        }
                    }
                }
            }
            id::SET_MODULATION_SOURCE_PARAMETER => {
                // Modulation message 0x30: I0 slot; v1 param, value index 3
                // ([re:61f280] with 4 values), v2 source.
                if let Some(Some(md)) = self.mods.get_mut(e.max(0) as usize) {
                    if e >= 0 {
                        md.set_param(vi(1).max(0) as usize, vi(2).max(0) as usize, v(3));
                    }
                }
            }
            id::SET_MASTER_CHANNEL_TUNE => self.master_pitch = v(0).clamp(0.0, 1.0),
            id::MUTE_ENGINE | id::SOLO_ENGINE => {
                if let Some(x) = self.engines.get_mut(e as usize) {
                    if m.kind == id::MUTE_ENGINE {
                        x.mute = vb(0)
                    } else {
                        x.solo = vb(0)
                    }
                    self.recompute_audible();
                }
            }
            id::MUTE_PAD | id::SOLO_PAD => {
                if let Some(p) = self.pad_mut(e, i(1)) {
                    if m.kind == id::MUTE_PAD {
                        p.mute = vb(0)
                    } else {
                        p.solo = vb(0)
                    }
                    self.recompute_audible();
                }
            }
            // Pad messages 0x51/0x52: the pad layer mode (0x52 = lround(v·4)).
            id::SET_PAD_LAYER_MODE | id::SET_PAD_LAYER_MODE01 => {
                let n = if m.kind == id::SET_PAD_LAYER_MODE { vi(0) } else { (v(0) * 4.0).round() as i32 };
                if let Some(p) = self.pad_mut(e, i(1)) {
                    p.mode = layer_mode_from_index(n);
                }
            }
            // 0x53: the pad's linkage j.
            id::SET_PAD_LINKAGE => {
                let (p, j) = (i(1), vi(0));
                if let (Some(x), Ok(p), Ok(j)) = (self.engines.get_mut(e.max(0) as usize), usize::try_from(p), usize::try_from(j)) {
                    if e >= 0 && p < x.links.len() && j < x.links[p].len() {
                        x.links[p][j] = vb(1);
                    }
                }
            }
            id::AUTO_VELOCITY_SPLIT_PAD | id::AUTO_VELOCITY_CROSS => {
                if let Some(p) = self.pad_mut(e, i(1)) {
                    let n = p.layers.len().min(LAYERS);
                    let valid: [bool; LAYERS] = std::array::from_fn(|k| p.layers.get(k).is_some_and(|l| l.slot.valid));
                    let mut splits: [crate::velsplit::Split; LAYERS] = std::array::from_fn(|k| p.layers.get(k).map_or([0, 0, 127, 127], |l| [l.slot.vel_low, 0, 127, l.slot.vel_high]));
                    if m.kind == id::AUTO_VELOCITY_SPLIT_PAD {
                        crate::velsplit::auto_split(&valid[..n], &mut splits[..n]);
                    } else {
                        crate::velsplit::auto_cross(&valid[..n], &mut splits[..n], v(0));
                    }
                    for (l, s) in p.layers.iter_mut().zip(&splits) {
                        l.slot.vel_low = s[0];
                        l.slot.vel_high = s[3];
                    }
                }
            }
            // [re:c1d5fb] setters (clamped to 127); turnovers only
            // reach the document.
            id::SET_LAYER_VELOCITY_SPLIT_MIN | id::SET_LAYER_VELOCITY_SPLIT_MAX => {
                if let Some(l) = self.layer_mut(e, i(1), i(2)) {
                    let x = (vi(0).max(0) as u32).min(127);
                    if m.kind == id::SET_LAYER_VELOCITY_SPLIT_MIN { l.slot.vel_low = x } else { l.slot.vel_high = x }
                }
            }
            // Pattern message 0xd9 ([re:cc1ad3]): with the engine
            // gate open and record armed, the bound lane of track index 4
            // takes the value at the engine's current step (through the
            // linkage, as 0xd6), and the graph record queue keeps it for the
            // next step the track reaches.
            id::SET_INSTO_MIDI_GRAPH_VALUE => {
                let track = i(4);
                let Some(eng) = self.engines.get_mut(e.max(0) as usize).filter(|_| e >= 0) else { return };
                if track < 0 {
                    return;
                }
                if !(self.recording && self.playing && eng.state.gate && eng.state.pattern_power) {
                    // [re:2c9461]: with the pattern off the value goes into the
                    // pad's dummy step ([re:24a1b0]
                    // [re:fd754d]), which live notes read.
                    if !eng.state.pattern_power {
                        let lane = vi(1).max(0) as usize;
                        if let Some(d) = eng.dummy.get_mut(track as usize) {
                            let lanes = d.get_or_insert_with(|| crate::pattern::StepData::off().lanes);
                            if let Some(x) = lanes.get_mut(lane) {
                                *x = v(0);
                            }
                        }
                    }
                    return;
                }
                let (pat, step) = (eng.state.current_pattern as i32, eng.state.current_limited_step as i32);
                let lane = vi(1).max(0);
                let g = crate::binding::Message::new(id::SET_STEP_GRAPH_STATE, &[e, track, -1, pat, track, step], &[lane as f32, v(0).clamp(0.0, 1.0), 1.0]);
                self.apply_message(&g);
                // [re:d4e1c3]: queue value·127 for the next step reached.
                let cc = (v(0) * 127.0) as i32;
                self.engines[e as usize].state.set_graph_record(track as usize, lane as usize, cc);
                self.doc_step(e as usize, pat as usize, track as usize, step as usize);
            }
            // Pattern message 0xd8 ([re:4734a6]..[re:96b124]): with the
            // gate open and record armed, the current pad's displayed lane
            // takes the value at the current step and the graph record queue
            // queues it for the next step. Port: [e, pad, −1, pattern]
            // [value, lane]; the editor writes the current step itself (0xd6),
            // so this only queues.
            id::SET_INSTO_GRAPH_VALUE => {
                let pad = i(1);
                let Some(eng) = self.engines.get_mut(e.max(0) as usize).filter(|_| e >= 0) else { return };
                if !(self.recording && self.playing && eng.state.gate && eng.state.pattern_power) || pad < 0 {
                    return;
                }
                eng.state.set_graph_record(pad as usize, vi(1).max(0) as usize, (v(0).clamp(0.0, 1.0) * 127.0) as i32);
            }
            id::SET_LAYER_MUTE | id::SET_LAYER_SOLO => {
                if let Some(l) = self.layer_mut(e, i(1), i(2)) {
                    if m.kind == id::SET_LAYER_MUTE {
                        l.mute = vb(0)
                    } else {
                        l.solo = vb(0)
                    }
                    self.recompute_audible();
                }
            }
            id::MUTE_GLOBAL_AUX => {
                if let Some(a) = self.aux.get_mut(e as usize) {
                    a.muted = vb(0);
                }
            }
            id::SET_PATTERN_POWER_STATE => {
                if (0..self.engines.len() as i32).contains(&e) {
                    self.msg_power(e as usize, vb(0));
                }
            }
            id::SONG_SET_ACTIVE => {
                // Song active [re:733eb8]: off stops song recording, on resumes it
                // while record is on.
                let pos = self.song_position();
                if !vb(0) {
                    self.songrec.song_deactivated(pos);
                }
                self.song.active = vb(0);
                if vb(0) && self.recording {
                    self.songrec.arm(self.playing, pos);
                }
            }
            // [re:84504f] (song timeline clicks).
            id::SET_CURRENT_TIME => self.set_current_time(v(0) as f64),
            id::SONG_SET_ERASE_STATE => {
                let Ok(t) = usize::try_from(e) else { return };
                let pos = self.song_position();
                let restart = (!vb(0) && !self.song.active)
                    .then(|| {
                        let eng = self.engines.get(t)?;
                        let p = eng.state.current_pattern;
                        let phase = eng.patterns.get(p).filter(|pd| !pd.sync.restarts()).map(|pd| {
                            let cycle = pd.length_16ths() * 0.25 * eng.timing.tempo_multiplier;
                            if cycle > 0.0 { self.ppq.rem_euclid(cycle) } else { 0.0 }
                        });
                        Some((p, phase.unwrap_or(0.0)))
                    })
                    .flatten();
                self.songrec.erase(t, vb(0), pos, restart);
            }
            id::SONG_CLEAR_ALL => self.song.tracks.iter_mut().for_each(Vec::clear),
            id::SONG_ADD_PART => {
                let part = crate::song::SongPart { start: v(0) as f64, length: (v(1) as f64).max(0.25), pattern: vi(2).max(0) as usize, phase: 0.0, mute: false };
                self.song.add_part(i(0).max(0) as usize, part);
            }
            id::SONG_DELETE_PART => self.song.delete_part_at(i(0).max(0) as usize, v(0) as f64),
            // `e_songSetInternalLoopActive` / `e_songSetInternalLoopRange` [start, end].
            id::SONG_SET_INTERNAL_LOOP_ACTIVE => {
                let (s, e) = (self.song.loop_start, self.song.loop_end);
                self.set_song_loop(vb(0), s, e);
            }
            id::SONG_SET_INTERNAL_LOOP_RANGE => {
                let on = self.song.loop_enabled;
                self.set_song_loop(on, v(0) as f64, v(1) as f64);
            }
            // Scene messages: v0 = scene.
            id::SCENE_TRIGGER => {
                let sc = vi(0).clamp(0, 95) as usize;
                self.trigger_scene(sc, 0, vb(1));
            }
            id::SCENE_TRIGGER_NEXT | id::SCENE_TRIGGER_PREV => {
                let clock = self.scene_clock();
                self.scenes.trigger_step(m.kind == id::SCENE_TRIGGER_NEXT, vb(0), Some(&clock));
            }
            id::SCENE_CUT | id::SCENE_COPY => self.scenes.copy(vi(0).clamp(0, 95) as usize, m.kind == id::SCENE_CUT),
            id::SCENE_PASTE => self.scenes.paste(vi(0).clamp(0, 95) as usize),
            id::SCENE_SNAPSHOT | id::SCENE_CLEAR | id::SCENE_SET_START_MODE => {
                let sc = vi(0).clamp(0, 95) as usize;
                let states: [(usize, bool); ENGINES] = std::array::from_fn(|k| self.engines.get(k).map_or((0, true), |x| (x.state.current_pattern, x.state.pattern_power)));
                let Some(scene) = self.scenes.scenes.get_mut(sc) else { return };
                match m.kind {
                    id::SCENE_SNAPSHOT => {
                        scene.used = true;
                        for (se, (pat, power)) in scene.engines.iter_mut().zip(states) {
                            se.pattern = pat;
                            se.power = power;
                        }
                    }
                    id::SCENE_CLEAR => *scene = crate::scene::Scene::default(),
                    _ => scene.start = [crate::scene::StartMode::Immediate, crate::scene::StartMode::Beat, crate::scene::StartMode::Bar][(vi(1).clamp(0, 2)) as usize],
                }
            }
            // `e_setLayerAudioFile{Start,End,LoopStart,LoopEnd}Sample` 0x76..0x79
            // [e, p, l] = frame: the layer's markers. The `…Sample01`
            // variants 0x7a..0x7d (MIDI-learnable SP / EP / LS / LE buttons)
            // carry 0..1: frame = round(v·(frames − 1)) (layer message,
            // ±0.5 from [re:70089a]).
            id::SET_LAYER_START_SAMPLE
            | id::SET_LAYER_END_SAMPLE
            | id::SET_LAYER_LOOP_START_SAMPLE
            | id::SET_LAYER_LOOP_END_SAMPLE
            | id::SET_LAYER_START_SAMPLE01
            | id::SET_LAYER_END_SAMPLE01
            | id::SET_LAYER_LOOP_START_SAMPLE01
            | id::SET_LAYER_LOOP_END_SAMPLE01 => {
                let Some(l) = self.layer_mut(i(0), i(1), i(2)) else { return };
                let frames = l.sample.as_ref().map(|s| s.left.len() as u32).unwrap_or(0);
                let kind = if m.kind >= id::SET_LAYER_START_SAMPLE01 { m.kind - 4 } else { m.kind };
                let f = if m.kind >= id::SET_LAYER_START_SAMPLE01 {
                    let x = v(0) * (frames as f32 - 1.0);
                    (x + if x < 0.0 { -0.5 } else { 0.5 }).max(0.0) as u32
                } else {
                    v(0).max(0.0) as u32
                };
                if let Some(t) = l.template.as_mut() {
                    match kind {
                        id::SET_LAYER_START_SAMPLE => t.start = f.min(t.end.saturating_sub(1)),
                        id::SET_LAYER_END_SAMPLE => t.end = f.max(t.start + 1),
                        id::SET_LAYER_LOOP_START_SAMPLE => t.loop_start = f.min(t.loop_end.saturating_sub(1)),
                        _ => t.loop_end = f.max(t.loop_start + 1),
                    }
                    t.loop_len = t.loop_end.saturating_sub(t.loop_start);
                }
            }
            // Layer message: the value is the mode index (automation
            // sends 0..1, scaled by 15 / 3).
            id::SET_LAYER_CUT_MODE | id::SET_LAYER_PLAY_MODE => {
                let (e, p, l) = (i(0), i(1), i(2));
                if e < 0 || p < 0 {
                    return;
                }
                let v = vi(0).max(0) as u32;
                if let Some(pad) = self.engines.get_mut(e as usize).and_then(|x| x.pads.get_mut(p as usize)) {
                    for (li, layer) in pad.layers.iter_mut().enumerate() {
                        if l >= 0 && li != l as usize {
                            continue;
                        }
                        if m.kind == id::SET_LAYER_CUT_MODE {
                            layer.cut = dreist_dsp::voices::CutMode::from_index(v.min(15));
                        } else if let Some(t) = &mut layer.template {
                            t.playback = [PlaybackMode::OneShot, PlaybackMode::OneShotLengthBound, PlaybackMode::Gated, PlaybackMode::Looping][v.min(3) as usize];
                        }
                    }
                }
            }
            // Layer messages 0x6b / 0x6e / 0x6f / 0x70 for the target
            // layers: synthesis mode (automation sends 0..1, rounded),
            // reverse, phase position (±1) and envelope
            // pitch tracking.
            id::SET_LAYER_SYNTHESIS_MODE | id::SET_LAYER_REVERSE_STATUS | id::SET_LAYER_PHASE_POSITION | id::SET_LAYER_ENVELOPE_TRACKING => {
                let (e, p, l) = (i(0), i(1), i(2));
                if e < 0 || p < 0 {
                    return;
                }
                let on = v(0) >= 0.5;
                if let Some(pad) = self.engines.get_mut(e as usize).and_then(|x| x.pads.get_mut(p as usize)) {
                    for (li, layer) in pad.layers.iter_mut().enumerate() {
                        if l >= 0 && li != l as usize {
                            continue;
                        }
                        let Some(t) = &mut layer.template else { continue };
                        match m.kind {
                            id::SET_LAYER_SYNTHESIS_MODE => t.vintage = v(0).round() as i32 == 1,
                            id::SET_LAYER_REVERSE_STATUS => t.reversed = on,
                            id::SET_LAYER_PHASE_POSITION => t.output_scale = if on { -1.0 } else { 1.0 },
                            _ => t.env_tracks = on,
                        }
                    }
                }
            }
            // [re:9ba40d]: index 3 = pattern, 4 =
            // track (pad), 5 = step.
            id::SET_PATTERN_STEP_LENGTH
            | id::SET_PATTERN_SYNC_MODE
            | id::SET_PATTERN_STEP_COUNT
            | id::SET_TRACK_MUTE_STATUS
            | id::SET_TRACK_SOLO_STATUS
            | id::SET_TRACK_LENGTH
            | id::SET_STEP_ON
            | id::SET_STEP_OFF
            | id::SET_STEP_VELOCITY
            | id::CLEAR_PATTERN
            | id::CLEAR_TRACK
            | id::NUDGE_TRACK_FORWARD
            | id::NUDGE_TRACK_BACKWARDS
            | id::NUDGE_PATTERN_FORWARD
            | id::NUDGE_PATTERN_BACKWARDS
            | id::SET_STEP_GRAPH_STATE
            | id::SET_GRAPH_RANDOMISATION
            | id::CLEAR_TRACK_AUTOMATION
            | id::CLEAR_ALL_TRACK_AUTOMATION
            | id::CLEAR_ALL_PATTERN_AUTOMATION
            | id::QUANTIZE_PATTERN_TIMING
            | id::QUANTIZE_PATTERN_VELOCITY
            | id::SET_PATTERN_MONO_MODE => {
                self.pattern_edit(m);
            }
            // Preview messages [re:355ba9].
            id::SET_PREVIEW_VOLUME => self.preview_volume = v(0).clamp(0.0, 1.0),
            id::STOP_PREVIEW => self.stop_preview(),
            id::SET_PREVIEW_LOOP_MODE => self.set_preview_loop(vb(0)),
            id::SET_PREVIEW_OUTPUT => self.preview_output = (v(0).round() as i32).clamp(1, 32) as usize,
            id::CLEAR_ALL_PATTERNS => {
                if let Some(x) = self.engines.get_mut(e.max(0) as usize) {
                    for p in &mut x.patterns {
                        for t in 0..p.tracks.len() {
                            p.clear_track(t);
                        }
                    }
                }
            }
            id::SELECT_PATTERN => {
                if (0..self.engines.len() as i32).contains(&e) && i(3) >= 0 {
                    self.select_pattern(e as usize, i(3) as usize, vb(0));
                }
            }
            id::SET_NOTE_REPEAT_STATE | id::SET_NOTE_ERASE_STATE | id::SET_FULL_LEVEL_STATE => {
                if let Some(x) = self.engines.get_mut(e as usize) {
                    match m.kind {
                        id::SET_NOTE_REPEAT_STATE => x.state.note_repeat = vb(0),
                        id::SET_NOTE_ERASE_STATE => x.state.note_erase = vb(0),
                        _ => x.state.full_level = vb(0),
                    }
                }
            }
            id::SET_NOTE_REPEAT_BASE => {
                if let Some(x) = self.engines.get_mut(e as usize) {
                    // Transport message 0x20: float values (bindings) → round(v·13).
                    x.state.note_repeat_base = (v(0) * 13.0).round().clamp(0.0, 13.0) as u32;
                }
            }
            id::SET_FULL_LEVEL_LEVEL => {
                if let Some(x) = self.engines.get_mut(e as usize) {
                    x.state.full_level_velocity = (v(0) * 127.0) as u32;
                }
            }
            id::SET_ENGINE_SWING_TEMPLATE_DEPTH => {
                if let Some(x) = self.engines.get_mut(e as usize) {
                    x.timing.swing_depth = v(0);
                }
            }
            // [re:c1ac41] / tempo denominator (1..16).
            id::SET_ENGINE_TEMPO_NUM | id::SET_ENGINE_TEMPO_DENOM => {
                if let Some(x) = self.engines.get_mut(e as usize) {
                    let n = (v(0).round() as i32).clamp(1, 16) as u32;
                    if m.kind == id::SET_ENGINE_TEMPO_NUM {
                        x.timing.tempo_num = n;
                    } else {
                        x.timing.tempo_den = n;
                    }
                    x.timing.tempo_multiplier = x.timing.tempo_num as f64 / x.timing.tempo_den as f64;
                }
            }
            // Effect change message [re:391a17]: chain active /
            // chain mix from value 0.
            id::SET_FX_CHAIN_ACTIVE | id::SET_FX_CHAIN_MIX => {
                let Some(chain) = self.chain_mut(i(0), i(1), i(2)) else { return };
                if m.kind == id::SET_FX_CHAIN_ACTIVE {
                    chain.chain.active = vb(0);
                } else {
                    chain.chain.mix = v(0).clamp(0.0, 1.0);
                }
            }
            id::SET_FX_PARAMETER | id::SET_FX_MIX | id::SET_FX_LEVEL | id::SET_FX_TRIM | id::SET_FX_ACTIVE => {
                // Effect change message [re:391a17]: chain at (engine, pad, layer),
                // slot = Index3 ([re:61f280]<4,4>) else value0.
                let slot = if i(3) >= 0 { i(3) } else { vi(0) };
                let (e, p, l) = (i(0), i(1), i(2));
                let Some(chain) = self.chain_mut(e, p, l) else { return };
                if slot < 0 {
                    return;
                }
                let slot = slot as usize;
                match m.kind {
                    id::SET_FX_PARAMETER => chain.set(slot, FxTarget::Param(vi(0).max(0) as usize), vi(1).max(0) as usize, v(2)),
                    id::SET_FX_MIX => chain.set(slot, FxTarget::Mix, vi(2).max(0) as usize, v(1)),
                    id::SET_FX_LEVEL => chain.set(slot, FxTarget::Level, vi(2).max(0) as usize, v(1)),
                    id::SET_FX_TRIM => chain.set(slot, FxTarget::Trim, vi(2).max(0) as usize, v(1)),
                    _ => chain.set_active(slot, vb(1)),
                }
            }
            id::SAMPLER_TOGGLE_ARMED..=id::SAMPLER_AUTO_MODE => self.sampler_message(m),
            // The playback state and the
            // editor messages are not handled by the port yet.
            _ => {}
        }
    }

    /// [re:f70fac] (auto target 0x101 and the take
    /// export are editor-side and not ported).
    fn sampler_message(&mut self, m: &Message) {
        use crate::sampler::{State, TimedMode};
        let clock = self.sampler_clock();
        let v = |k: usize| m.value[k];
        let selected = self.selected_engine;
        let s = &mut self.sampler;
        // Arming takes the auto-allocation target from the selected engine
        // (kept by the editor; MIDI follow reads
        // its engine).
        if s.state == State::Idle && matches!(m.kind, id::SAMPLER_TOGGLE_ARMED | id::SAMPLER_SET_ARMED) {
            s.target_engine = Some(selected);
        }
        match m.kind {
            id::SAMPLER_TOGGLE_ARMED => match s.state {
                State::Armed => s.disarm(),
                State::Recording => s.stop_recording(),
                State::Idle => s.arm(),
            },
            id::SAMPLER_SET_ARMED => s.arm(),
            id::SAMPLER_SET_NOT_ARMED => s.disarm(),
            id::SAMPLER_TOGGLE_RECORD => match s.state {
                State::Recording => s.stop_recording(),
                State::Armed => s.start_armed(&clock),
                State::Idle => {}
            },
            id::SAMPLER_START_RECORD => {
                if s.state == State::Recording {
                    s.start_now(&clock);
                } else {
                    s.start_armed(&clock);
                }
            }
            id::SAMPLER_STOP_RECORD => s.stop_recording(),
            id::SAMPLER_START_MODE => s.set_start_mode(v(0) as i32),
            id::SAMPLER_TIMED_MODE => {
                s.timed = match v(0) as i32 {
                    1 => TimedMode::Stop,
                    2 => TimedMode::Retake,
                    _ => TimedMode::Off,
                }
            }
            id::SAMPLER_INPUT_GAIN => s.input_gain = v(0),
            id::SAMPLER_OUTPUT_GAIN => s.output_gain = v(0),
            id::SAMPLER_START_THRESHOLD => s.start_threshold = v(0),
            id::SAMPLER_STOP_THRESHOLD => s.stop_threshold = v(0),
            id::SAMPLER_RECORD_SECS => s.secs = v(0).clamp(0.0, 3600.0),
            id::SAMPLER_BARS_BEATS => {
                s.bars = (v(0).max(0.0) as u32).min(16);
                // Beats are capped below the time signature numerator.
                let num = self.time_sig.0.max(1);
                s.beats = (v(1).max(0.0) as u32).min(num - 1);
            }
            id::SAMPLER_BEATS_MODE => s.beats_mode = v(0) >= 0.5,
            id::SAMPLER_CLEAR_TAKE => s.clear_take(v(0).max(0.0) as usize),
            id::SAMPLER_CLEAR_TAKES => s.clear_takes(),
            id::SAMPLER_RECORD_SOURCE => s.source = v(0) as i32,
            id::SAMPLER_MONO => s.set_channels(v(0) as i32),
            id::SAMPLER_THRU => s.thru = v(0) >= 0.5,
            id::SAMPLER_AUTO_MODE => s.auto_mode = (v(0).max(0.0) as usize).min(5),
            _ => {}
        }
    }

    fn pad_mut(&mut self, e: i32, p: i32) -> Option<&mut super::PadRt> {
        self.engines.get_mut(usize::try_from(e).ok()?)?.pads.get_mut(usize::try_from(p).ok()?)
    }

    fn layer_mut(&mut self, e: i32, p: i32, l: i32) -> Option<&mut super::LayerRt> {
        self.pad_mut(e, p)?.layers.get_mut(usize::try_from(l).ok()?)
    }

    /// [re:7d62e7]: −1 selects the
    /// enclosing level; engine −4 is global aux `p`, −3 the master.
    fn chain_mut(&mut self, e: i32, p: i32, l: i32) -> Option<&mut crate::fx::FxChain> {
        match e {
            -4 => return Some(&mut self.aux.get_mut(usize::try_from(p).ok()?)?.fx),
            -3 => return Some(&mut self.master_fx),
            _ => {}
        }
        if e < 0 {
            return None;
        }
        if p < 0 {
            return Some(&mut self.engines.get_mut(e as usize)?.fx);
        }
        if l < 0 {
            return Some(&mut self.pad_mut(e, p)?.fx);
        }
        Some(&mut self.layer_mut(e, p, l)?.fx)
    }

    /// Layer message 0x66 → [re:34ea12].
    /// `e_tuneLayer` 0x88 (layer message [re:db5a60] →
    /// [re:f8581c]): coarse / fine tune so the sample's BPM
    /// (metadata, else the length guess) plays at the tempo: semitones =
    /// 12·log2(tempo/BPM), coarse = semitones/24 + 0.5 (unquantised), fine
    /// = the remainder past the nearest semitone ·0.5 + 0.5, taken from the
    /// unclamped coarse value (beyond ±12 semitones fine keeps the
    /// fraction, as the original's routine does). The editor
    /// hears of both (0x1c8; port: `automation_out`).
    fn tune_layer(&mut self, e: i32, p: i32, l: i32) {
        let Some(layer) = self.layer_mut(e, p, l) else { return };
        let Some(src) = layer.template.as_ref().map(|t| t.file_bpm).filter(|b| *b > 0.0) else { return };
        let tempo = self.bpm as f32;
        if tempo <= 0.0 || (tempo - src).abs() < 1e-6 {
            return;
        }
        let ratio = (tempo / src).abs();
        if ratio == 0.0 {
            return;
        }
        // In double, as the original: semitones = 12·log2(ratio); the
        // normalised pitch (−12 − x)/(−12 − 12) in float; fine from the
        // unclamped coarse value. Both are clamped only when stored.
        let semis = (ratio as f64).ln() * 17.312339006067457;
        let coarse_u = (-12.0f32 - semis as f32) / (-12.0f32 - 12.0);
        let s = (coarse_u as f64 - 0.5) * 24.0;
        let r = (s + if s < 0.0 { -0.5 } else { 0.5 }) as i32 as f64;
        let fine = (((s - r) * 0.5 + 0.5) as f32).clamp(0.0, 1.0);
        let coarse = coarse_u.clamp(0.0, 1.0);
        for (par, v) in [(dreist_formats::LayerParam::CoarseTune, coarse), (dreist_formats::LayerParam::FineTune, fine)] {
            self.set_layer_param(e, p, l, par as i32, 0, v);
            let m = Message::new(id::SET_LAYER_PARAMETER, &[e, p, l], &[par as u8 as f32, v, 0.0]);
            if let Some(o) = self.automation_out.iter_mut().find(|o| o.same_target(&m)) {
                o.value = m.value;
            } else if self.automation_out.len() < 256 {
                self.automation_out.push(m);
            }
        }
    }

    fn set_layer_param(&mut self, e: i32, p: i32, l: i32, param: i32, source: i32, value: f32) {
        let (Ok(param), Ok(source)) = (usize::try_from(param), usize::try_from(source)) else { return };
        if param >= LAYER_PARAMS || source >= SOURCES {
            return;
        }
        let value = clamp_param(source, value);
        let Some(layer) = self.layer_mut(e, p, l) else { return };
        if let Some(t) = layer.template.as_mut() {
            t.layer[param][source] = value;
        }
        if param == dreist_formats::LayerParam::Gain as usize {
            layer.gain[source] = value;
        } else if param == dreist_formats::LayerParam::Pan as usize {
            layer.pan[source] = value;
        }
        for v in &mut self.voices {
            if matches!(v.state, VoiceState::Pending | VoiceState::Playing) && v.slot.engine as i32 == e && v.slot.pad as i32 == p && v.slot.layer as i32 == l {
                v.voice.set_layer_param(param, source, value);
                if let Some((_, s)) = v.pending.as_mut() {
                    s.layer[param][source] = value;
                }
            }
        }
    }

    /// Pad message 0x4e → [re:fc38b1].
    fn set_pad_param(&mut self, e: i32, p: i32, param: i32, source: i32, value: f32) {
        let Ok(source) = usize::try_from(source) else { return };
        if source >= SOURCES {
            return;
        }
        let value = clamp_param(source, value);
        let Some(pad) = self.pad_mut(e, p) else { return };
        match PadParam::ALL.get(param.max(0) as usize) {
            Some(PadParam::Gain) => pad.gain[source] = value,
            Some(PadParam::Pan) => pad.pan[source] = value,
            Some(PadParam::V2A) if source == 0 => pad.v2a = value,
            Some(PadParam::Pitch) => pad.pitch[source] = value,
            _ => {}
        }
    }

    /// Engine message 0x90 → [re:a4cbaf].
    fn set_engine_param(&mut self, e: i32, param: i32, source: i32, value: f32) {
        let Ok(source) = usize::try_from(source) else { return };
        if source >= SOURCES {
            return;
        }
        let value = clamp_param(source, value);
        let Some(x) = self.engines.get_mut(e.max(0) as usize) else { return };
        if e < 0 {
            return;
        }
        match EngineParam::ALL.get(param.max(0) as usize) {
            Some(EngineParam::Gain) => x.gain[source] = value,
            Some(EngineParam::Pan) => x.pan[source] = value,
            Some(EngineParam::Pitch) => x.pitch[source] = value,
            _ => {}
        }
    }

    /// Step/track/pattern edits from the pattern editor.
    fn pattern_edit(&mut self, m: &Message) {
        use crate::pattern::{StepData, lane};
        let (e, p, t, st) = (m.index[0], m.index[3], m.index[4], m.index[5]);
        let Some(eng) = (e >= 0).then(|| self.engines.get_mut(e as usize)).flatten() else { return };
        let p = if p < 0 { eng.state.current_pattern } else { p as usize };
        let one_shot = (t >= 0).then(|| eng.one_shot.get(t as usize).copied()).flatten().unwrap_or(true);
        let Some(pat) = eng.patterns.get_mut(p) else { return };
        let v = m.value;
        if m.kind == id::SET_PATTERN_STEP_LENGTH {
            pat.resolution = (v[0] as i32).clamp(0, 13) as usize;
            return;
        }
        if m.kind == id::SET_PATTERN_SYNC_MODE {
            pat.sync = crate::pattern::SyncMode::from_index(v[0].max(0.0) as u32);
            return;
        }
        match m.kind {
            // Pattern messages 0xbf/0xc0/0xc1: lane (v0) of the track,
            // every lane of the track, every lane of the pattern.
            id::CLEAR_TRACK_AUTOMATION if t >= 0 => {
                pat.revert_lanes(Some(t as usize), Some(v[0].max(0.0) as usize));
                return;
            }
            id::CLEAR_ALL_TRACK_AUTOMATION if t >= 0 => {
                pat.revert_lanes(Some(t as usize), None);
                return;
            }
            id::CLEAR_ALL_PATTERN_AUTOMATION => {
                pat.revert_lanes(None, None);
                return;
            }
            // [re:c10fda] / step velocity quantize:
            // Shift / Velocity back to their defaults (selected-only is an
            // editor selection in the port).
            id::QUANTIZE_PATTERN_TIMING => {
                pat.revert_lanes(None, Some(lane::SHIFT));
                return;
            }
            id::QUANTIZE_PATTERN_VELOCITY => {
                pat.revert_lanes(None, Some(lane::VELOCITY));
                return;
            }
            id::SET_PATTERN_MONO_MODE => {
                pat.mono = v[0] >= 0.5;
                return;
            }
            id::CLEAR_PATTERN => {
                for t in 0..pat.tracks.len() {
                    pat.clear_track(t);
                }
                return;
            }
            id::NUDGE_PATTERN_FORWARD | id::NUDGE_PATTERN_BACKWARDS => {
                pat.nudge(m.kind == id::NUDGE_PATTERN_FORWARD);
                return;
            }
            id::CLEAR_TRACK if t >= 0 => {
                pat.clear_track(t as usize);
                return;
            }
            id::NUDGE_TRACK_FORWARD | id::NUDGE_TRACK_BACKWARDS if t >= 0 => {
                pat.nudge_track(t as usize, m.kind == id::NUDGE_TRACK_FORWARD);
                return;
            }
            _ => {}
        }
        if m.kind == id::SET_PATTERN_STEP_COUNT {
            // [re:b4b5d4](n, true). The audio thread
            // resizes within the storage a track has; steps past it read as
            // off until the editor's rebuild brings the longer pattern.
            let n = (v[0] as i32).clamp(1, 1024) as u32;
            let old = pat.steps;
            pat.steps = n;
            for tr in &mut pat.tracks {
                let keep = (n as usize).min(tr.steps.capacity());
                tr.steps.resize(keep, StepData::off());
                if tr.length == old || tr.length > n {
                    tr.length = n;
                }
            }
            return;
        }
        let Some(tr) = (t >= 0).then(|| pat.tracks.get_mut(t as usize)).flatten() else { return };
        match m.kind {
            // [re:29e8ec].
            id::SET_GRAPH_RANDOMISATION => {
                if let Some(r) = tr.rand.get_mut(v[1].max(0.0) as usize) {
                    *r = v[0].clamp(0.0, 1.0);
                }
            }
            id::SET_TRACK_MUTE_STATUS => tr.mute = v[0] >= 0.5,
            id::SET_TRACK_SOLO_STATUS => tr.solo = v[0] >= 0.5,
            id::SET_TRACK_LENGTH => tr.length = (v[0] as i32).max(1) as u32,
            _ => {
                let Some(step) = (st >= 0).then(|| tr.steps.get_mut(st as usize)).flatten() else { return };
                match m.kind {
                    id::SET_STEP_ON => {
                        step.on = true;
                        step.lanes[lane::VELOCITY] = v[0];
                        if !one_shot {
                            step.lanes[lane::DURATION] = v[1];
                            step.lanes[lane::LINK] = if v[2] >= 0.5 { 1.0 } else { 0.0 };
                        } else {
                            step.lanes[lane::LINK] = 0.0;
                        }
                        pat.monoize(st as usize, t as usize);
                    }
                    id::SET_STEP_OFF => {
                        step.on = false;
                        step.lanes[lane::LINK] = 0.0;
                    }
                    // e_setStepGraphState [lane, value, selected]: the
                    // selection-relative paths are editor-only here.
                    id::SET_STEP_GRAPH_STATE => crate::pattern::set_lane(step, (v[0].max(0.0)) as usize, v[1]),
                    _ => step.lanes[lane::VELOCITY] = v[0],
                }
            }
        }
    }
}

fn set_row(row: Option<&mut super::Row>, source: i32, value: f32) {
    if let (Some(r), Ok(s)) = (row, usize::try_from(source)) {
        if s < SOURCES {
            r[s] = clamp_param(s, value);
        }
    }
}

/// Pad layer mode ([re:b9e7b0] order).
fn layer_mode_from_index(n: i32) -> dreist_dsp::voices::LayerMode {
    use dreist_dsp::voices::LayerMode::*;
    match n {
        1 => VelocitySplit,
        2 => RoundRobin,
        3 => RoundRobinReset,
        4 => Random,
        _ => AllLayers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RenderOptions;
    use std::path::PathBuf;

    fn hollow() -> Option<Dreist> {
        let content = dreist_formats::legacy::content_dir().to_string_lossy().into_owned();
        Dreist::load(&PathBuf::from(content).join(dreist_formats::legacy::DEMO_FACTORY).join("Presets/Hollow.geist"), 44100.0, RenderOptions::default()).ok()
    }

    #[test]
    fn saved_cc_bindings_drive_macros() {
        let Some(mut g) = hollow() else { return };
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        g.push_midi(0, [0xb0, 10, 127]);
        g.push_midi(0, [0xb0, 11, 127]);
        let mut prev = -1.0;
        for _ in 0..40 {
            g.process(&mut l, &mut r);
            let m1 = g.host.values[src::MACRO1];
            assert!(m1 >= prev - 1e-6, "macro 1 rises smoothly");
            prev = m1;
        }
        assert!(prev > 0.99, "macro 1 reached {prev}");
        // CC11 is bound with Min 1 / Max 0: full CC → macro 2 = 0.
        assert!(g.host.values[src::MACRO1 + 1] < 0.01);
        // The editor hears of the latest value per target.
        let macros: Vec<_> = g.automation_out.iter().filter(|m| m.kind == id::SET_MACRO_VALUE).collect();
        assert_eq!(macros.len(), 2);
        assert!(macros.iter().any(|m| m.index[0] == 0 && m.value[0] > 0.99));
    }

    #[test]
    fn layer_parameter_message_updates_template() {
        let Some(mut g) = hollow() else { return };
        let (e, p, l) = (0usize, 0usize, 0usize);
        let gain = dreist_formats::LayerParam::Gain as i32;
        g.post_message(Message::new(id::SET_LAYER_PARAMETER, &[e as i32, p as i32, l as i32], &[gain as f32, 0.25, 0.0]));
        let (mut a, mut b) = (vec![0.0; 64], vec![0.0; 64]);
        g.process(&mut a, &mut b);
        let layer = &g.engines[e].pads[p].layers[l];
        assert_eq!(layer.gain[0], 0.25);
        if let Some(t) = &layer.template {
            assert_eq!(t.layer[gain as usize][0], 0.25);
        }
    }

    #[test]
    fn sampler_messages_arm_record_and_configure() {
        use crate::sampler::{State, TimedMode};
        let Some(mut g) = hollow() else { return };
        g.apply_message(&Message::new(id::SAMPLER_START_MODE, &[], &[3.0]));
        assert_eq!(g.sampler.timed, TimedMode::Off);
        g.apply_message(&Message::new(id::SAMPLER_START_MODE, &[], &[0.0]));
        assert_eq!(g.sampler.timed, TimedMode::Stop);
        g.apply_message(&Message::new(id::SAMPLER_TOGGLE_ARMED, &[], &[0.0]));
        assert_eq!(g.sampler.state, State::Armed);
        g.apply_message(&Message::new(id::SAMPLER_TOGGLE_RECORD, &[], &[0.0]));
        assert_eq!(g.sampler.state, State::Recording);
        g.apply_message(&Message::new(id::SAMPLER_TOGGLE_ARMED, &[], &[0.0]));
        assert_eq!(g.sampler.state, State::Idle);
        g.apply_message(&Message::new(id::SAMPLER_BARS_BEATS, &[], &[40.0, 9.0]));
        assert_eq!((g.sampler.bars, g.sampler.beats), (16, 3));
        g.apply_message(&Message::new(id::SAMPLER_INPUT_GAIN, &[], &[2.0]));
        g.apply_message(&Message::new(id::SAMPLER_MONO, &[], &[2.0]));
        assert_eq!(g.sampler.input_gain, 2.0);
        assert_eq!(g.sampler.channels, crate::sampler::Channels::Right);
    }

    #[test]
    fn sampler_records_the_master_output_and_persists_its_source() {
        use crate::sampler::{SOURCE_EXTERNAL, SOURCE_MASTER, State, source_from_name, source_name};
        assert_eq!(source_name(SOURCE_MASTER), "Master");
        assert_eq!(source_name(3), "Sub output 2");
        assert_eq!(source_from_name("Sub output 2"), 3);
        assert_eq!(source_from_name("External"), SOURCE_EXTERNAL);
        assert_eq!(source_from_name("MITM"), SOURCE_EXTERNAL);
        let Some(mut g) = hollow() else { return };
        g.start();
        // Hollow stores recordsource="Master".
        assert_eq!(g.sampler.source, SOURCE_MASTER);
        g.apply_message(&Message::new(id::SAMPLER_START_MODE, &[], &[0.0]));
        g.apply_message(&Message::new(id::SAMPLER_TOGGLE_ARMED, &[], &[0.0]));
        g.apply_message(&Message::new(id::SAMPLER_TOGGLE_RECORD, &[], &[0.0]));
        assert_eq!(g.sampler.state, State::Recording);
        // No host input at all: the master still records.
        let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
        for _ in 0..200 {
            g.process_with_input(&mut l, &mut r, &mut [], None);
        }
        let take = g.sampler.current_take().expect("take");
        assert!(take.left.iter().any(|x| x.abs() > 1e-4), "the playing project reached the take");
        // The source follows the message and the document.
        g.apply_message(&Message::new(id::SAMPLER_RECORD_SOURCE, &[], &[SOURCE_EXTERNAL as f32]));
        assert_eq!(g.sampler.source, SOURCE_EXTERNAL);
        let sm = g.doc.children.iter().find(|c| c.class == "Sampler").unwrap();
        assert_eq!(sm.attr("recordsource"), Some("External"));
    }

    #[test]
    fn editor_only_binding_messages_go_to_the_editor() {
        let Some(mut g) = hollow() else { return };
        let n = dreist_formats::Node::parse(
            r#"<Binding name="MIDI0Binding0" EditorOnly="true" Min="0" Max="1">
            <VString name="MessageStuffer" string="Binding::StuffToValue0" />
            <TMessage_I6_V3 name="MessagePrototype" Index0="-1" Index1="-1" Index2="-1" Index3="-1" Index4="-1" Index5="-1">
              <VString name="Type" string="e_updateEditAllPads" />
            </TMessage_I6_V3>
            <VString name="Type" string="Continuous" />
          </Binding>"#,
        )
        .unwrap();
        let b = crate::binding::Binding::from_node(&n);
        assert!(b.editor_only);
        let mut m = b.prototype.clone();
        m.editor = true;
        m.value[0] = 1.0;
        let tempo = g.tempo();
        g.apply_message(&m);
        assert_eq!(g.editor_messages.len(), 1);
        assert_eq!(g.editor_messages[0].kind, 0x1c6);
        assert_eq!(g.tempo(), tempo, "not applied to the engine");
    }

    #[test]
    fn pattern_ops_match_the_mirrored_document() {
        use crate::pattern::PatternData;
        let Some(mut g) = hollow() else { return };
        let pat = g.engines[0].state.current_pattern;
        let step = |t, s| Message::new(id::SET_STEP_GRAPH_STATE, &[0, -1, -1, pat as i32, t, s], &[3.0, 0.8, 0.0]);
        for m in [
            Message::new(id::NUDGE_TRACK_FORWARD, &[0, -1, -1, pat as i32, 0, -1], &[0.0]),
            Message::new(id::NUDGE_PATTERN_BACKWARDS, &[0, -1, -1, pat as i32, -1, -1], &[0.0]),
            step(1, 2),
            Message::new(id::CLEAR_TRACK, &[0, -1, -1, pat as i32, 2, -1], &[0.0]),
        ] {
            g.apply_message(&m);
        }
        let node =
            g.document().children.iter().find(|c| c.class == "Engine" && c.name.as_deref() == Some("Engine0")).and_then(|e| e.child("StepPattern", &format!("StepPattern{pat}"))).cloned().unwrap();
        let from_doc = PatternData::from_node(&node);
        let live = &g.engines[0].patterns[pat];
        for t in 0..4 {
            for s in 0..live.steps as usize {
                assert_eq!(from_doc.tracks[t].steps[s].on, live.tracks[t].steps[s].on, "track {t} step {s}");
            }
        }
        assert!((live.tracks[1].steps[2].lanes[3] - 0.8).abs() < 1e-6);
        assert!((from_doc.tracks[1].steps[2].lanes[3] - 0.8).abs() < 1e-4);
        assert!(live.tracks[2].steps.iter().all(|s| !s.on));
        for m in [Message::new(id::QUANTIZE_PATTERN_TIMING, &[0, -1, -1, pat as i32, -1, -1], &[0.0]), Message::new(id::SET_PATTERN_MONO_MODE, &[0, -1, -1, pat as i32, -1, -1], &[1.0])] {
            g.apply_message(&m);
        }
        let node =
            g.document().children.iter().find(|c| c.class == "Engine" && c.name.as_deref() == Some("Engine0")).and_then(|e| e.child("StepPattern", &format!("StepPattern{pat}"))).cloned().unwrap();
        let from_doc = PatternData::from_node(&node);
        let live = &g.engines[0].patterns[pat];
        let d = dreist_formats::params::step_lane_default(3);
        assert!((live.tracks[1].steps[2].lanes[3] - d).abs() < 1e-6);
        assert!((from_doc.tracks[1].steps[2].lanes[3] - d).abs() < 1e-4);
        assert!(live.mono && from_doc.mono);
    }

    #[test]
    fn tempo_modifier_and_swing_depth() {
        let Some(mut g) = hollow() else { return };
        for m in [Message::new(id::SET_ENGINE_TEMPO_NUM, &[0], &[3.0]), Message::new(id::SET_ENGINE_TEMPO_DENOM, &[0], &[4.0]), Message::new(id::SET_ENGINE_SWING_TEMPLATE_DEPTH, &[0], &[0.25])] {
            g.apply_message(&m);
        }
        assert!((g.engines[0].timing.tempo_multiplier - 0.75).abs() < 1e-9);
        let eng = g.document().children.iter().find(|c| c.class == "Engine" && c.name.as_deref() == Some("Engine0")).cloned().unwrap();
        assert_eq!(eng.u32("TempoNumerator"), Some(3));
        assert_eq!(eng.u32("TempoDenominator"), Some(4));
        assert!((eng.f32("SwingTemplateDepth").unwrap() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn layer_marker_messages() {
        let Some(mut g) = hollow() else { return };
        let (e, p, l) = (0usize, 0usize, 0usize);
        if g.engines[e].pads[p].layers[l].template.is_none() {
            return;
        }
        g.apply_message(&Message::new(id::SET_LAYER_START_SAMPLE, &[0, 0, 0], &[100.0]));
        g.apply_message(&Message::new(id::SET_LAYER_LOOP_END_SAMPLE, &[0, 0, 0], &[500.0]));
        let t = g.engines[e].pads[p].layers[l].template.as_ref().unwrap();
        assert_eq!(t.start, 100);
        assert_eq!(t.loop_end, 500);
        let doc = g.document();
        let layer = doc.children.iter().find(|c| c.class == "Engine" && c.name.as_deref() == Some("Engine0")).and_then(|n| n.child("Pad", "Pad0")).and_then(|n| n.child("Layer", "Layer0")).unwrap();
        assert_eq!(layer.child("AudioFileMarkers", "StartEndMarkers").and_then(|m| m.u32("StartSample")), Some(100));
        assert_eq!(layer.child("AudioFileMarkers", "LoopMarkers").and_then(|m| m.u32("EndSample")), Some(500));
        // 0..1 variants: round(v·(frames − 1)).
        let frames = g.engines[e].pads[p].layers[l].sample.as_ref().unwrap().left.len() as f32;
        g.apply_message(&Message::new(id::SET_LAYER_END_SAMPLE01, &[0, 0, 0], &[0.5]));
        let t = g.engines[e].pads[p].layers[l].template.as_ref().unwrap();
        assert_eq!(t.end, (0.5 * (frames - 1.0) + 0.5) as u32);
    }

    #[test]
    fn preview_plays_then_stops() {
        let mut g = Dreist::empty(44100.0, RenderOptions::default());
        let data = std::sync::Arc::new(dreist_dsp::voice::SampleData { left: vec![0.5; 1000], right: None, sample_rate: 44100.0 });
        g.start_preview(Some(data));
        let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
        g.process(&mut l, &mut r);
        assert!(l.iter().any(|x| x.abs() > 0.1));
        for _ in 0..3 {
            g.process(&mut l, &mut r);
        }
        assert!(!g.previewing());
    }

    #[test]
    fn preview_region_loops_until_stopped() {
        let mut g = Dreist::empty(44100.0, RenderOptions::default());
        let data = std::sync::Arc::new(dreist_dsp::voice::SampleData { left: vec![0.5; 1000], right: None, sample_rate: 44100.0 });
        g.start_preview_region(data, 100, 300, true);
        let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
        for _ in 0..4 {
            g.process(&mut l, &mut r);
        }
        assert!(g.previewing());
        g.post_message(Message::new(id::SET_PREVIEW_LOOP_MODE, &[], &[0.0]));
        g.process(&mut l, &mut r);
        assert!(!g.previewing(), "loop off: the region finishes");
        g.start_preview(None);
        let data = std::sync::Arc::new(dreist_dsp::voice::SampleData { left: vec![0.5; 1000], right: None, sample_rate: 44100.0 });
        g.start_preview_region(data, 0, 1000, true);
        g.post_message(Message::new(id::STOP_PREVIEW, &[], &[]));
        g.process(&mut l, &mut r);
        assert!(!g.previewing());
    }

    #[test]
    fn song_recording_reports_user_pattern_changes() {
        let p = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(dreist_formats::legacy::CONTENT_DIR).join(dreist_formats::legacy::DEMO_FACTORY).join("Presets/Hollow.geist");
        let Ok(proj) = dreist_formats::Project::load(&p) else { return };
        let mut cache = crate::audio::SampleCache::default();
        let Some(mut g) = Dreist::from_document(&proj.raw, &dreist_formats::model::samples_dir(&p), 44100.0, RenderOptions::default(), &mut cache) else { return };
        g.post_message(Message::new(id::SONG_SET_ACTIVE, &[], &[1.0]));
        g.post_message(Message::new(id::TOGGLE_RECORD, &[], &[1.0]));
        g.start();
        let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
        g.process(&mut l, &mut r);
        assert!(g.songrec.events.contains(&crate::songrec::SongEvent::State(2)), "{:?}", g.songrec.events);
        g.songrec.events.clear();
        g.post_message(Message::new(id::SET_PATTERN_POWER_STATE, &[0, -1, -1, 0], &[1.0]));
        g.post_message(Message::new(id::SELECT_PATTERN, &[0, -1, -1, 1], &[0.0]));
        for _ in 0..8 {
            g.process(&mut l, &mut r);
        }
        let started = g.songrec.events.iter().any(|e| matches!(e, crate::songrec::SongEvent::Started { track: 0, recording: true, .. }));
        assert!(started, "{:?}", g.songrec.events);
    }

    #[test]
    fn midi_graph_value_writes_the_current_step_while_recording() {
        let Some(mut g) = hollow() else { return };
        let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
        // Not recording: ignored.
        g.start();
        g.process(&mut l, &mut r);
        let m = |v: f32| Message::new(id::SET_INSTO_MIDI_GRAPH_VALUE, &[0, 0, -1, 0, 0], &[v, crate::pattern::lane::PROBABILITY as f32]);
        let pan = |g: &Dreist| {
            let s = &g.engines[0].state;
            g.engines[0].patterns[s.current_pattern].tracks[0].steps[s.current_limited_step as usize].lanes[crate::pattern::lane::PROBABILITY]
        };
        g.post_message(m(0.9));
        g.process(&mut l, &mut r);
        assert!((pan(&g) - 0.9).abs() > 1e-4);
        g.post_message(Message::new(id::TOGGLE_RECORD, &[], &[1.0]));
        g.process(&mut l, &mut r);
        assert!(g.engines[0].state.gate, "pattern gate open while playing");
        g.post_message(m(0.9));
        g.process(&mut l, &mut r);
        let s = &g.engines[0].state;
        let (cp, st) = (s.current_pattern, s.current_limited_step);
        let lanes = &g.engines[0].patterns[cp].tracks[0].steps;
        assert!(lanes.iter().any(|x| (x.lanes[crate::pattern::lane::PROBABILITY] - 0.9).abs() < 1e-4), "pan written near step {st}");
    }

    #[test]
    fn midi_graph_value_sets_the_dummy_step_with_the_pattern_off() {
        let Some(mut g) = hollow() else { return };
        let lane = crate::pattern::lane::PROBABILITY;
        g.engines[0].state.pattern_power = false;
        g.apply_message(&Message::new(id::SET_INSTO_MIDI_GRAPH_VALUE, &[0, 2, -1, 0, 2], &[0.3, lane as f32]));
        assert_eq!(g.engines[0].dummy[2].as_ref().map(|d| d[lane]), Some(0.3));
        // Pattern on and not recording: nothing.
        g.engines[0].state.pattern_power = true;
        g.apply_message(&Message::new(id::SET_INSTO_MIDI_GRAPH_VALUE, &[0, 4, -1, 0, 4], &[0.3, lane as f32]));
        assert!(g.engines[0].dummy[4].is_none());
    }

    #[test]
    fn midi_out_sends_sequenced_notes() {
        let Some(mut g) = hollow() else { return };
        let (mut l, mut r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
        g.apply_message(&Message::new(id::REGISTRY_MIDI_OPTIONS_CHANGED, &[], &[1.0, 0.0]));
        assert!(g.send_notes && !g.send_ccs);
        g.start();
        let mut ons = Vec::new();
        let mut offs = 0;
        for _ in 0..400 {
            g.process(&mut l, &mut r);
            for &(t, d) in &g.midi_out {
                assert!(t < 512);
                match d[0] & 0xf0 {
                    0x90 => ons.push(d),
                    0x80 => offs += 1,
                    _ => {}
                }
            }
        }
        assert!(!ons.is_empty(), "sequenced steps send note ons");
        assert!(ons.iter().all(|d| d[1] >= 36 && d[2] > 0), "{ons:?}");
        assert!(offs > 0, "and note offs a step later");
        // Off again: nothing new.
        g.apply_message(&Message::new(id::REGISTRY_MIDI_OPTIONS_CHANGED, &[], &[0.0, 0.0]));
        for _ in 0..50 {
            g.process(&mut l, &mut r);
        }
        g.process(&mut l, &mut r);
        assert!(g.midi_out.iter().all(|e| e.1[0] & 0xf0 != 0x90));
    }

    #[test]
    fn playback_state_message_starts_and_stops() {
        let Some(mut g) = hollow() else { return };
        g.apply_message(&Message::new(id::SET_PLAYBACK_STATE, &[], &[1.0]));
        assert!(g.is_playing());
        g.apply_message(&Message::new(id::SET_PLAYBACK_STATE, &[], &[0.0]));
        assert!(!g.is_playing());
    }

    #[test]
    fn tune_layer_matches_the_tempo() {
        let Some(mut g) = hollow() else { return };
        let (e, p, l) = (0..8)
            .flat_map(|e| (0..64).flat_map(move |p| (0..8).map(move |l| (e, p, l))))
            .find(|&(e, p, l)| g.engines[e].pads[p].layers.get(l).and_then(|x| x.template.as_ref()).is_some_and(|t| t.file_bpm > 0.0))
            .unwrap();
        let src = g.engines[e].pads[p].layers[l].template.as_ref().unwrap().file_bpm;
        g.automation_out.clear();
        g.apply_message(&Message::new(id::TUNE_LAYER, &[e as i32, p as i32, l as i32], &[0.0; 3]));
        let tempo = g.tempo() as f32;
        if (tempo - src).abs() < 1e-3 {
            return;
        }
        let coarse = ((tempo / src).log2() * 12.0 / 24.0 + 0.5).clamp(0.0, 1.0);
        let c = dreist_formats::LayerParam::CoarseTune as u8 as f32;
        let m = g.automation_out.iter().find(|m| m.value[0] == c).expect("coarse echoed");
        assert!((m.value[1] - coarse).abs() < 1e-4, "{} vs {coarse}", m.value[1]);
        assert_eq!(g.automation_out.len(), 2);
    }

    #[test]
    fn song_play_resumes_at_the_last_stop() {
        let Some(mut g) = hollow() else { return };
        assert!(g.song_active());
        g.start();
        let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
        for _ in 0..100 {
            g.process(&mut l, &mut r);
        }
        let at = g.song_position();
        g.stop();
        g.start();
        assert!((g.song_position() - at).abs() < 1e-9 && at > 1.0);
        // Return to zero while stopped, then Play starts at 0.
        g.stop();
        g.return_to_zero();
        g.start();
        assert_eq!(g.song_position(), 0.0);
        // With the song off, Play starts at 0.
        g.apply_message(&Message::new(id::SONG_SET_ACTIVE, &[], &[0.0, 0.0]));
        for _ in 0..10 {
            g.process(&mut l, &mut r);
        }
        g.stop();
        g.start();
        assert_eq!(g.song_position(), 0.0);
    }

    #[test]
    fn transport_toggle_panic_and_return_to_zero() {
        let Some(mut g) = hollow() else { return };
        // Loaded for rendering: already playing.
        g.stop();
        g.apply_message(&Message::new(id::TOGGLE_PLAYBACK, &[], &[]));
        assert!(g.is_playing());
        g.apply_message(&Message::new(id::TOGGLE_PLAYBACK, &[], &[]));
        assert!(!g.is_playing());
        // Panic: voices gone, transport stopped.
        g.apply_message(&Message::new(id::TOGGLE_PLAYBACK, &[], &[]));
        g.pad_hit(0, 0, 127, 0);
        let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
        g.process(&mut l, &mut r);
        g.apply_message(&Message::new(id::PANIC, &[], &[]));
        assert!(!g.is_playing());
        assert!(g.voices.iter().all(|v| v.state == VoiceState::Idle));
        // Return to zero only acts in song mode.
        g.apply_message(&Message::new(id::SONG_SET_ACTIVE, &[], &[0.0]));
        g.apply_message(&Message::new(id::SET_CURRENT_TIME, &[], &[12.0]));
        g.apply_message(&Message::new(id::RETURN_TO_ZERO, &[], &[]));
        assert_eq!(g.ppq, 12.0);
        g.apply_message(&Message::new(id::SONG_SET_ACTIVE, &[], &[1.0]));
        g.apply_message(&Message::new(id::SONG_SET_INTERNAL_LOOP_RANGE, &[], &[4.0, 8.0]));
        g.apply_message(&Message::new(id::SONG_SET_INTERNAL_LOOP_ACTIVE, &[], &[1.0]));
        // Stopped past the loop start → loop start, then → 0.
        g.apply_message(&Message::new(id::SET_CURRENT_TIME, &[], &[6.0]));
        g.apply_message(&Message::new(id::RETURN_TO_ZERO, &[], &[]));
        assert_eq!(g.locate, 4.0);
        g.apply_message(&Message::new(id::RETURN_TO_ZERO, &[], &[]));
        assert_eq!(g.locate, 0.0);
    }

    #[test]
    fn aux_and_master_chains_and_pre_fade() {
        let Some(mut g) = hollow() else { return };
        // Effect chain lookup: engine −4 = aux (pad index), −3 = master.
        g.apply_message(&Message::new(id::SET_FX_CHAIN_ACTIVE, &[-4, 1, -1], &[0.0]));
        assert!(!g.aux[1].fx.chain.active);
        g.apply_message(&Message::new(id::SET_FX_CHAIN_MIX, &[-3, -1, -1], &[0.25]));
        assert_eq!(g.master_fx.chain.mix, 0.25);
        g.apply_message(&Message::new(id::SET_GLOBAL_AUX_PRE_POST, &[2], &[1.0]));
        assert!(g.aux[2].pre_fade);
    }

    #[test]
    fn x0x_pad_hits_cycle_the_current_track_steps() {
        let Some(mut g) = hollow() else { return };
        g.set_pad_mode(0, crate::live::PadMode::X0x);
        let (track, cur) = (g.engines[0].state.current_pad, g.engines[0].state.current_pattern);
        let step = |g: &Dreist| {
            let s = g.engines[0].patterns[cur].tracks[track].step(5);
            (s.on, s.lanes[crate::pattern::lane::VELOCITY])
        };
        // Start from off.
        if step(&g).0 {
            g.apply_message(&Message::new(id::SET_STEP_OFF, &[0, track as i32, -1, cur as i32, track as i32, 5], &[0.0, 0.0]));
        }
        g.pad_hit(0, 5, 100, 0);
        assert_eq!(step(&g), (true, 0.5));
        g.pad_hit(0, 5, 100, 0);
        assert_eq!(step(&g), (true, 1.0));
        g.pad_hit(0, 5, 100, 0);
        assert!(!step(&g).0);
        assert!(g.step_writes.iter().any(|w| w.track == track && w.step == 5));
    }

    #[test]
    fn graph_record_queue_writes_the_next_step() {
        let Some(mut g) = hollow() else { return };
        let (mut l, mut r) = (vec![0.0f32; 256], vec![0.0f32; 256]);
        g.start();
        g.post_message(Message::new(id::TOGGLE_RECORD, &[], &[1.0]));
        g.process(&mut l, &mut r);
        let lane = crate::pattern::lane::PROBABILITY;
        g.post_message(Message::new(id::SET_INSTO_GRAPH_VALUE, &[0, 3, -1, 0], &[0.5, lane as f32]));
        g.process(&mut l, &mut r);
        assert!(g.engines[0].state.graph_rec_dirty[3]);
        let first = g.engines[0].state.current_limited_step;
        // Run until the step changes: the queue lands there and empties.
        for _ in 0..2000 {
            g.process(&mut l, &mut r);
            if g.engines[0].state.current_limited_step != first {
                break;
            }
        }
        let s = &g.engines[0].state;
        assert!(!s.graph_rec_dirty[3]);
        let st = s.current_step[3] as usize;
        let v = g.engines[0].patterns[s.current_pattern].tracks[3].steps[st].lanes[lane];
        assert!((v - 63.0 / 127.0).abs() < 1e-4, "{v}"); // 0.5·127 truncated (cvttss2si)
        assert!(g.step_writes.iter().any(|w| w.track == 3 && w.step == st));
    }
}
