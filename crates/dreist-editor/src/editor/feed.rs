//! What the audio thread and the host feed the editor: meters, playback,
//! load progress, sampler takes, scene / retro / song-recording events, MIDI
//! in, automated parameters and binding messages.

use crate::*;

impl Editor {
    /// One meter tick from the engine's peak levels (the caller clears
    /// them afterwards, as the original clears all meters).
    pub fn update_meters(&mut self, levels: &dreist_engine::Meters) {
        // Only the changed meters are repainted.
        for r in meters::update(&mut self.app.root, &self.model, levels) {
            self.app.invalidate_rect(r);
        }
        if self.header.tick_indicators(levels.midi_received, levels.voice_stolen) {
            let before = self.app.marks();
            self.header.sync(&mut self.app, &self.model, self.learn.active);
            self.app.invalidate_changes(&before);
        }
    }

    /// Transport state from the engine. A play / stop changes the header
    /// (full redraw); a new step or song position only moves the pattern
    /// grid's and song body's playheads, so only those repaint.
    pub fn set_playback(&mut self, playing: bool, steps: [Option<u32>; 8], song_pos: Option<f64>) {
        if self.model.playing != playing {
            self.model.playing = playing;
            self.app.dirty = true;
        }
        let grid = self.model.steps != steps;
        let song = self.model.song_pos != song_pos;
        let (old_pos, new_pos) = (self.model.song_pos, song_pos);
        self.model.steps = steps;
        self.model.song_pos = song_pos;
        if !(grid || song) {
            return;
        }
        // Option `PatternEditorFollow`: keep the position in view.
        if playing && self.model.follow_playback {
            let mut scrolled = grid && self.pattern.as_ref().is_some_and(|p| p.follow(&self.app, &mut self.model));
            // The song panel only shows the position with the song active.
            let song_active = self.model.doc.children.iter().find(|c| c.class == "SongSequencer").and_then(|n| n.bool("SongActive")).unwrap_or(false);
            if let (true, true, Some(pos), Some(sg)) = (song, song_active, song_pos, self.song.as_mut()) {
                scrolled |= sg.follow(pos);
            }
            if scrolled {
                self.sync();
                self.app.dirty = true;
                return;
            }
        }
        // The song body: the old and new position lines only.
        let lines: Option<Vec<f32>> = self.song.as_ref().and_then(|sg| [old_pos, new_pos].iter().flatten().map(|&p| sg.playhead_x(p)).collect());
        let mut rects = Vec::new();
        fn walk(c: &Ctrl, ox: i32, oy: i32, grid: bool, song: bool, lines: &Option<Vec<f32>>, out: &mut Vec<dreist_gui::Rect>) {
            if !c.visible {
                return;
            }
            let (x, y) = (ox + c.rect.x, oy + c.rect.y);
            if grid && c.overload == "PatternGrid" {
                out.push(dreist_gui::Rect::new(x, y, c.rect.w, c.rect.h));
            }
            if song && c.overload == "SongBody" {
                match lines {
                    Some(ls) => out.extend(ls.iter().map(|lx| dreist_gui::Rect::new(x + lx.floor() as i32 - 2, y, 5, c.rect.h))),
                    None => out.push(dreist_gui::Rect::new(x, y, c.rect.w, c.rect.h)),
                }
            }
            for k in &c.children {
                walk(k, x, y, grid, song, lines, out);
            }
        }
        walk(&self.app.root, -self.app.root.rect.x, -self.app.root.rect.y, grid, song, &lines, &mut rects);
        for r in rects {
            self.app.invalidate_rect(r);
        }
    }

    /// Loader progress of the current rebuild (fraction, 1.5 = done) for
    /// the browser's loading bar ([re:0a1fc4]
    ///  [re:91a0d5] → 0x143).
    pub fn load_progress(&mut self, f: f32) {
        if self.browser.as_mut().is_some_and(|b| b.load_progress(f)) {
            self.app.dirty = true;
        }
    }

    /// The tempo the engine runs at (the host's, tap tempo): into the
    /// document, whose [re:5cc709] is the engine's own state in the original
    /// (the header shows it, saves and rebuilds keep it); not an undo step.
    pub fn engine_tempo(&mut self, bpm: f64) {
        let bpm = bpm.clamp(20.0, 999.0);
        if (bpm - self.model.tempo()).abs() > 1e-9 {
            self.model.doc.child_mut_or_insert("TimeInfo", "TimeInfo").set("Tempo", bpm);
            self.header.sync(&mut self.app, &self.model, self.learn.active);
        }
    }

    /// Whether the host reports a loop (the header's Loop button is
    /// disabled meanwhile).
    pub fn set_host_loop(&mut self, on: bool) {
        if self.model.host_loop != on {
            self.model.host_loop = on;
            self.header.sync(&mut self.app, &self.model, self.learn.active);
            self.app.dirty = true;
        }
    }

    /// Modulators retriggered by pad notes (bit = transmod source index):
    /// flash their source buttons.
    pub fn modulators_triggered(&mut self, mask: u32) {
        for s in 0..32 {
            if mask & (1 << s) != 0 {
                self.transmod.flash(s);
            }
        }
    }

    /// A sampler take finished: save it and allocate it by the Follow
    /// mode (Slice opens the slicer).
    pub fn sampler_take(&mut self, left: &[f32], right: &[f32], sr: f32) -> Option<sampler::TakeAction> {
        let mut out = Vec::new();
        // Take stop [re:e602d2]: the display keeps the finished take.
        let sel = (self.model.engine, self.model.pad[self.model.engine], self.model.layer[self.model.engine]);
        let buckets =
            left.chunks(model::LIVE_BUCKET).enumerate().map(|(i, l)| model::bucket(l, right.get(i * model::LIVE_BUCKET..(i * model::LIVE_BUCKET + l.len()).min(right.len())).unwrap_or(&[]))).collect();
        self.model.sampling = Some(model::Sampling { buckets, frames: left.len(), selection: sel });
        let r = self.sampler.take_finished(&mut self.model, &self.modals.options, left, right, sr, &mut out);
        // Auto allocation loads the take into a layer, whose view
        // ([re:a29974]) leaves sampling mode.
        if matches!(r, Some(sampler::TakeAction::Loaded)) {
            self.model.sampling = None;
        }
        self.outbox.extend(out);
        if let Some(sampler::TakeAction::Slice(f)) = &r {
            let f = f.clone();
            self.open_slicer(&f);
        }
        self.sync();
        r
    }

    /// Live take display from the audio thread ([re:94c103] /
    /// [re:eb0601]): `reset` starts a new take.
    pub fn sampler_live(&mut self, reset: bool, buckets: &[(f32, f32)]) {
        if !reset && buckets.is_empty() {
            return;
        }
        let e = self.model.engine;
        let sel = (e, self.model.pad[e], self.model.layer[e]);
        if reset || self.model.sampling.is_none() {
            self.model.sampling = Some(model::Sampling { buckets: Vec::new(), frames: 0, selection: sel });
        }
        if let Some(s) = &mut self.model.sampling {
            s.buckets.extend_from_slice(buckets);
            s.frames += buckets.len() * model::LIVE_BUCKET;
        }
        self.app.dirty = true;
    }

    /// Scene notifications from the engine (activity animations).
    pub fn scene_events(&mut self, events: &[dreist_engine::scene::SceneEvent]) {
        if let Some(sc) = &mut self.scenes {
            for e in events {
                sc.event(*e);
            }
        }
        self.animate();
    }

    /// Pattern retro input from the engine.
    pub fn retro_events(&mut self, events: &[dreist_engine::songrec::RetroEvent]) {
        for ev in events {
            self.retro.event(ev, self.model.engine);
        }
    }

    /// Steps the engine wrote while recording: copy them into the editor's
    /// document (the original's engine posts the step changes to the editor).
    pub fn step_writes(&mut self, writes: &[dreist_engine::StepWrite]) {
        if writes.is_empty() {
            return;
        }
        for w in writes {
            dreist_engine::engine::doc::write_step(&mut self.model.doc, w.engine, w.pattern, w.track, w.step, &w.data);
        }
        self.sync();
    }

    /// Song recorder messages from the engine.
    pub fn song_events(&mut self, events: &[dreist_engine::songrec::SongEvent]) {
        if events.is_empty() {
            return;
        }
        let limit = self.undo_limit();
        let Some(sg) = &mut self.song else { return };
        let mut out = Vec::new();
        for ev in events {
            if sg.song_event(&self.model, ev, &mut out) {
                // "Record Song Parts" undo point.
                self.history.push("Record Song Parts".into(), self.model.doc.clone(), limit);
            }
            for c in out.drain(..) {
                self.model.apply(&c);
                self.outbox.push(c);
            }
        }
        self.sync();
    }

    /// Incoming MIDI for the MIDI console.
    pub fn midi_in(&mut self, msgs: &[[u8; 3]]) {
        let mode = self.modals.options.int("MIDINumbering", 1).clamp(0, 2) as u8;
        if self.midiconsole.push(msgs, mode) {
            self.app.dirty = true;
        }
    }

    pub fn take_commands(&mut self) -> Vec<Command> {
        if std::mem::take(&mut self.model.pattern_grown) {
            let r = self.model.rebuild_command();
            self.outbox.push(r);
        }
        std::mem::take(&mut self.outbox)
    }

    /// Messages from editor-only bindings (`EditorOnly`; the panels'
    /// message handlers, see `editbind`).
    /// Bound parameters the engine moved (MIDI CC / host automation): into
    /// the document and onto the controls (the original's editor reads the
    /// shared model), not undo steps.
    pub fn automation(&mut self, msgs: &[dreist_engine::binding::Message]) {
        for m in msgs {
            self.model.apply(&Command::Message(m.clone()));
        }
        self.sync();
    }

    pub fn binding_messages(&mut self, msgs: &[dreist_engine::binding::Message]) {
        for m in msgs {
            self.binding_message(m);
        }
        self.sync();
    }

    /// A MIDI CC arrived (count, channel, controller): in learn mode with a
    /// target, bind it.
    pub fn midi_cc(&mut self, count: u32, ch: u8, cc: u8) {
        if count == self.learn.last_cc {
            return;
        }
        self.learn.last_cc = count;
        if let Some(m) = self.midimap.as_mut() {
            m.incoming(&self.app, &self.model, ch as usize, cc as usize);
        }
        if self.learn.active && self.learn.target.is_some() {
            self.learn_bind(true, ch as usize, cc as usize);
            self.learn.target = None;
        }
    }
}
