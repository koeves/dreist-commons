//! What the audio thread leaves for the plugin's editor ([`UiState`], under
//! a lock) and the editor's copy of it ([`Inbox`]), taken under the lock and
//! handled after releasing it.

/// Engine state the GUI displays, refreshed by the audio thread.
#[derive(Clone, Debug, Default)]
pub struct UiState {
    pub playing: bool,
    pub tempo: f64,
    pub steps: [Option<u32>; 8],
    /// Mini sequencer playing step per transmod slot (−1: none).
    pub seq_steps: Option<[i32; 16]>,
    /// Retriggered transmod sources (bit mask) for the source flashes.
    pub mods_triggered: u32,
    /// Song position (QN) while playing.
    pub song_pos: Option<f64>,
    /// The host reports a loop this block.
    pub host_loop: bool,
    /// Messages from editor-only bindings.
    pub editor_messages: Vec<dreist_engine::binding::Message>,
    /// Bound parameters moved by MIDI CCs / host automation (latest per
    /// target), for the editor's document.
    pub automation: Vec<dreist_engine::binding::Message>,
    pub sampler: crate::sampling::SamplerView,
    /// Last MIDI CC (count, channel, controller) for MIDI learn.
    pub last_cc: (u32, u8, u8),
    /// Peak levels since the editor's last meter tick.
    pub meters: dreist_engine::Meters,
    /// Finished sampler takes and their sample rate, for the editor to save.
    pub takes: Vec<(dreist_engine::sampler::Take, f32)>,
    /// Incoming MIDI for the MIDI console.
    pub midi_log: Vec<[u8; 3]>,
    /// Live take display: a new take started, and new min/max pairs.
    pub live_reset: bool,
    pub live: Vec<(f32, f32)>,
    /// Song recorder messages.
    pub song_events: Vec<dreist_engine::songrec::SongEvent>,
    /// Pattern retro recording input.
    pub retro_events: Vec<dreist_engine::songrec::RetroEvent>,
    /// Steps the engine wrote (recording).
    pub step_writes: Vec<dreist_engine::StepWrite>,
    /// Scene notifications.
    pub scene_events: Vec<dreist_engine::scene::SceneEvent>,
    /// A reserved buffer for the sampler's next take (see
    /// [re:23fa47]), kept ready by the rebuild worker.
    pub spare_take: Option<dreist_engine::sampler::Take>,
}

/// The GUI's copy of what the audio thread left in [`UiState`], taken under
/// the lock and handled after releasing it.
#[derive(Default)]
pub struct Inbox {
    pub playing: bool,
    pub tempo: f64,
    pub steps: [Option<u32>; 8],
    pub seq_steps: Option<[i32; 16]>,
    pub mods_triggered: u32,
    pub song_pos: Option<f64>,
    pub host_loop: bool,
    pub editor_messages: Vec<dreist_engine::binding::Message>,
    pub automation: Vec<dreist_engine::binding::Message>,
    pub sampler: Option<crate::sampling::SamplerView>,
    pub last_cc: (u32, u8, u8),
    pub meters: dreist_engine::Meters,
    pub takes: Vec<(dreist_engine::sampler::Take, f32)>,
    pub midi_log: Vec<[u8; 3]>,
    pub live_reset: bool,
    pub live: Vec<(f32, f32)>,
    pub song_events: Vec<dreist_engine::songrec::SongEvent>,
    pub retro_events: Vec<dreist_engine::songrec::RetroEvent>,
    pub step_writes: Vec<dreist_engine::StepWrite>,
    pub scene_events: Vec<dreist_engine::scene::SceneEvent>,
}

impl Inbox {
    /// Move the queues out of `s` (draining keeps the audio thread's
    /// reserved capacity) and copy the display state.
    pub fn collect(&mut self, s: &mut UiState) {
        self.playing = s.playing;
        self.tempo = s.tempo;
        self.steps = s.steps;
        self.seq_steps = s.seq_steps.take();
        self.mods_triggered |= std::mem::take(&mut s.mods_triggered);
        self.song_pos = s.song_pos;
        self.host_loop = s.host_loop;
        self.editor_messages.append(&mut s.editor_messages);
        self.automation.append(&mut s.automation);
        self.sampler = Some(s.sampler.clone());
        self.last_cc = s.last_cc;
        self.meters.merge(&s.meters);
        s.meters.clear();
        s.sampler.meters = (0.0, 0.0);
        self.takes.append(&mut s.takes);
        self.midi_log.append(&mut s.midi_log);
        // A new take: the previous take's pairs still here go too.
        if std::mem::take(&mut s.live_reset) {
            self.live_reset = true;
            self.live.clear();
        }
        self.live.append(&mut s.live);
        self.song_events.append(&mut s.song_events);
        self.retro_events.append(&mut s.retro_events);
        self.step_writes.append(&mut s.step_writes);
        self.scene_events.append(&mut s.scene_events);
    }

    /// Handled: empty the queues, keeping their capacity.
    pub fn clear(&mut self) {
        self.editor_messages.clear();
        self.automation.clear();
        self.meters.clear();
        self.midi_log.clear();
        self.live_reset = false;
        self.live.clear();
        self.song_events.clear();
        self.retro_events.clear();
        self.step_writes.clear();
        self.scene_events.clear();
    }
}
