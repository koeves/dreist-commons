//! Event dispatch: an event goes to the panels (`route`), its commands
//! update the model and the undo history, then the panels resync.

use crate::*;

/// Custom drawing for the code-drawn panels.
struct Drawer<'a> {
    model: &'a Model,
    browser: Option<&'a browser::Browser>,
    song: Option<&'a song::SongPanel>,
    pattern: Option<&'a pattern::PatternPanel>,
    inspector: &'a inspector::Inspector,
    midimap: Option<&'a midimap::MidiMap>,
    slicer: &'a slicer::Slicer,
    filefind: &'a filefind::FileFind,
    devices: &'a devices::Devices,
    transmod: &'a transmod::Transmod,
    midiconsole: &'a midiconsole::MidiConsole,
    scenes: Option<&'a scenes::Scenes>,
    modals: &'a modal::Modals,
    learn: &'a learn::Learn,
}

impl dreist_gui::render::CustomDraw for Drawer<'_> {
    fn draw(&self, r: &dreist_gui::render::Renderer, p: &mut dreist_gui::paint::Painter, c: &Ctrl) -> bool {
        self.pattern.is_some_and(|pp| pp.draw(self.model, r, p, c))
            || self.inspector.draw(self.model, r, p, c)
            || self.song.is_some_and(|s| s.draw(self.model, r, p, c))
            || self.browser.is_some_and(|b| b.draw(r, p, c))
            || self.midimap.is_some_and(|m| m.draw(self.model, r, p, c))
            || self.slicer.draw(r, p, c)
            || self.filefind.draw(r, p, c)
            || self.devices.draw(r, p, c)
            || self.transmod.draw(self.model, r, p, c)
            || self.scenes.is_some_and(|sc| sc.draw(self.model, r, p, c))
            || self.midiconsole.draw(r, p, c)
            || self.modals.draw(r, p, c)
            || (c.overload == "LearnOverlay" && draw_learn(self.learn, r, p))
    }

    fn draw_over(&self, r: &dreist_gui::render::Renderer, p: &mut dreist_gui::paint::Painter, c: &Ctrl) {
        if c.overload == "PatternSelector" {
            pattern::draw_selector(r, p, c);
        } else if c.overload == "EditorPad" {
            pads::draw_flash(r, p, c);
        }
    }
}

/// [re:b05593] learn overlay: corner brackets a quarter of each
/// learnable control's size, binding text, a 2 px outline on the target.
fn draw_learn(l: &learn::Learn, r: &dreist_gui::render::Renderer, p: &mut dreist_gui::paint::Painter) -> bool {
    use dreist_gui::paint::RectF;
    let target = r.styles.colours.get("LearnTargetColour").copied().unwrap_or(dreist_gui::Colour::rgba(255, 160, 0, 255));
    let text = r.styles.colours.get("LearnTextColour").copied().unwrap_or(dreist_gui::Colour::WHITE);
    let font = r.styles.font("ApplicationStyle");
    for (rc, label, sel) in &l.rects {
        let (x, y, w, h) = (rc.x as f32 + 0.5, rc.y as f32 + 0.5, rc.w as f32 - 1.0, rc.h as f32 - 1.0);
        let (qw, qh) = ((rc.w / 4) as f32, (rc.h / 4) as f32);
        for (x0, y0, x1, y1) in [
            (x, y, x + qw, y),
            (x, y, x, y + qh),
            (x + w - qw, y, x + w, y),
            (x + w, y, x + w, y + qh),
            (x, y + h, x + qw, y + h),
            (x, y + h - qh, x, y + h),
            (x + w - qw, y + h, x + w, y + h),
            (x + w, y + h - qh, x + w, y + h),
        ] {
            p.line(x0, y0, x1, y1, target, 1.0);
        }
        if let Some(t) = label {
            p.text(RectF::new(rc.x as f32, rc.y as f32, rc.w as f32, rc.h as f32), t, text, &font, dreist_gui::style::HAlign::Center, dreist_gui::style::VAlign::Center);
        }
        if *sel {
            p.stroke_rect(RectF::new(rc.x as f32, rc.y as f32, rc.w as f32, rc.h as f32), target, 2.0);
        }
    }
    true
}

impl Editor {
    /// Collapse panel notifications.
    /// - Effect panels ([re:375f19] collapse start /
    ///   expand end): hide / show ActiveButton, NextPresetButton,
    ///   LastPresetButton and the knobs' labeller (the OutputKnob's parent).
    /// - [re:ebb236] collapse end / expand
    ///   end: hide / show every child of the panel.
    pub(crate) fn collapse_notification(&mut self, path: &[usize], iconic: bool, end: bool) {
        let parent_is = |app: &App, n: &str| path.split_last().and_then(|(_, p)| app.ctrl(p)).is_some_and(|c| c.name == n);
        let layer = parent_is(&self.app, "LayerParams");
        let Some(c) = self.app.ctrl_mut(path) else { return };
        if c.attr("fxSlot").is_some() && (iconic != end) {
            fxchain::fold_controls(c, !iconic);
        } else if layer && end {
            for k in &mut c.children {
                k.visible = !iconic;
                k.enabled = !iconic;
            }
        }
        self.app.dirty = true;
    }

    /// Committed text boxes: tempo, step count and the names kept in the
    /// document (pattern, pad, layer, engine, aux).
    pub(crate) fn text_commit(&mut self, ev: &UiEvent) -> bool {
        let UiEvent::TextCommitted { path, name, text } = ev else { return false };
        let e = self.model.engine;
        let cur = self.model.current_pattern(e);
        let t = text.trim();
        match name.as_str() {
            "BPMTextBox" => {
                if let Some(bpm) = crate::conv::number(t) {
                    self.outbox.push(command::msg(dreist_engine::binding::id::SET_TEMPO, &[], &[bpm.clamp(20.0, 999.0)]));
                }
            }
            "LengthBox" => {
                if let Ok(n) = t.parse::<u32>() {
                    self.outbox.push(command::msg(dreist_engine::binding::id::SET_PATTERN_STEP_COUNT, &[e as i32, -1, -1, cur as i32], &[n.clamp(1, 1024) as f32]));
                }
            }
            "PatternNameBox" => self.model.set_pattern_name(e, cur, t),
            "PadName" => self.model.set_pad_name(e, self.model.pad[e], t),
            "SampleName" => {
                let layer = (0..=path.len()).rev().find_map(|n| self.app.ctrl(&path[..n])?.attr("layer")?.parse::<usize>().ok());
                if let Some(l) = layer {
                    let p = self.model.pad[e];
                    self.model.set_layer_name(e, p, l, t);
                }
            }
            "NameLabel" => {
                // Mixer strips: the channel tag set by the mixer.
                let ch = (0..=path.len()).rev().find_map(|n| self.app.ctrl(&path[..n])?.attr("channel").map(str::to_string));
                let Some(ch) = ch else { return false };
                let num = |s: &str| s.trim_matches(|c: char| !c.is_ascii_digit()).parse::<usize>().ok();
                if let Some(k) = ch.strip_prefix("Engine").and_then(num) {
                    self.model.engine_mut(k).set("Name", t);
                } else if let Some(k) = ch.strip_prefix("Pad").and_then(num) {
                    self.model.set_pad_name(e, k, t);
                } else if let Some(k) = ch.strip_prefix("Layer").and_then(num) {
                    let p = self.model.pad[e];
                    self.model.set_layer_name(e, p, k, t);
                } else if let Some(k) = ch.strip_prefix("Aux").and_then(num) {
                    let n = self.model.doc.children.iter_mut().find(|c| c.class == "GlobalAux" && c.name.as_deref() == Some(&format!("GlobalAux{k}")));
                    if let Some(n) = n {
                        n.set("AuxName", t);
                    }
                } else {
                    return false;
                }
            }
            _ => return false,
        }
        true
    }

    /// Run an event through the panels (modal panels first).
    pub(crate) fn route(&mut self, ev: &UiEvent) -> bool {
        // The slicer is modal: events aimed at it go nowhere else.
        let (ff_hit, ff_rebuild) = self.filefind.handle(&mut self.app, &mut self.model, ev);
        if ff_rebuild {
            self.outbox.push(self.model.rebuild_command());
        }
        let mc_hit = self.midiconsole.handle(&self.app, ev)
            || self.trigger.handle(&mut self.app, ev, &mut self.outbox)
            || self.metadata.handle(&mut self.app, &self.model, ev, &mut self.outbox)
            || self.convertor.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
        let dev_hit = self.devices.handle(&mut self.app, ev);
        if std::mem::take(&mut self.devices.restart_requested) {
            if let Some(h) = &self.devices.host {
                h.restart(&self.model.doc, self.model.samples.as_deref());
            }
        }
        let slicer_hit = dev_hit || ff_hit || mc_hit || self.modals.handle(&mut self.app, ev) || self.slicer.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
        if let Some(theme) = self.modals.style_changed.take() {
            self.app.set_theme(&theme);
        }
        let mut changed = slicer_hit;
        if !slicer_hit {
            changed |= self.header.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
            changed |= self.dropped(ev);
            changed |= self.drag_out(ev);
            changed |= self.pad_menu(ev);
            changed |= self.engine_menu(ev);
            self.patmenu.selection = self.pattern.as_ref().map(|p| p.selection.clone()).unwrap_or_default();
            changed |= self.patmenu.handle(&mut self.app, &mut self.model, &mut self.clipboard, ev, &mut self.outbox);
            if std::mem::take(&mut self.patmenu.selection_cut) {
                if let Some(p) = self.pattern.as_mut() {
                    p.selection.clear();
                }
            }
            if let Some(p) = self.patmenu.convert.take() {
                self.convertor.show(p);
                changed = true;
            }
            // Pattern › Export as audio ([re:6f2709]):
            // stop playback, render the selected engine's pattern with the
            // export options to "<pattern name>.wav", then show the file.
            if std::mem::take(&mut self.patmenu.export) {
                let e = self.model.engine;
                let p = self.model.current_pattern(e);
                let name = self.model.pattern_name(e, p);
                let name = if name.trim().is_empty() { format!("Pattern {}", p + 1) } else { name };
                let safe: String = name.chars().map(|c| if c.is_alphanumeric() || " -_".contains(c) { c } else { '_' }).collect();
                if let Some(dir) = options::settings_dir().map(|d| d.join("Exports")) {
                    let _ = std::fs::create_dir_all(&dir);
                    let mut options = modal::export_options(&self.modals.options, e);
                    options.source = dreist_engine::export::Source::Engine(e);
                    options.files = dreist_engine::export::Files::Mixdown;
                    self.outbox.push(Command::Play(false));
                    self.outbox.push(Command::ExportAudio {
                        doc: Box::new(self.model.doc.clone()),
                        samples: self.model.samples.clone(),
                        path: dir.join(format!("{}.wav", safe.trim())),
                        options,
                        reveal: true,
                        status: None,
                    });
                }
                changed = true;
            }
            changed |= self.pads.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
            changed |= self.pads.handle_controls(&mut self.app, &mut self.model, ev, &mut self.outbox);
            changed |= self.sampler.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
            if let Some(id) = self.editor_tabs.as_ref().and_then(|t| t.clicked(&self.app, ev)) {
                self.model.editor_tab = id;
                changed = true;
            }
            if let Some(id) = self.utility_tabs.as_ref().and_then(|t| t.clicked(&self.app, ev)) {
                self.model.utility_tab = id;
                changed = true;
            }
            if let Some(p) = &mut self.pattern {
                changed |= p.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
            }
            changed |= self.inspector.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
            for m in &mut self.mixers {
                changed |= m.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
            }
            if let Some(sc) = &mut self.scenes {
                changed |= sc.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
            }
            if let Some(mm) = &mut self.midimap {
                changed |= mm.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
            }
            if let Some(sg) = &mut self.song {
                changed |= sg.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
                // Header Retro (`e_grabRetroRecording` 0x23 → the song
                // panel's retro record onto its tracks).
                // With the song off it grabs the pattern retro buffer.
                if let UiEvent::Clicked { name, .. } = ev {
                    if name == "RetroButton" {
                        self.outbox.push(command::msg(dreist_engine::binding::id::GRAB_RETRO_RECORDING, &[], &[]));
                        let song_active = self.model.doc.children.iter().find(|c| c.class == "SongSequencer").and_then(|n| n.bool("SongActive")).unwrap_or(false);
                        changed |= if song_active { sg.apply_retro(&self.model, &mut self.outbox) } else { self.retro.apply(&self.model, &mut self.outbox) };
                    }
                }
            }
            if let Some(b) = &mut self.browser {
                changed |= b.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
            }
            changed |= self.transmod.handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
            if let Some((slot, anchor)) = self.transmod.trigger_request.take() {
                self.trigger.show(&mut self.app, &self.model, slot, &anchor);
                changed = true;
            }
            changed |= self.file_menu(ev);
            changed |= self.tools_help_menu(ev);
            changed |= self.text_commit(ev);
            if let Some(f) = self.browser.as_mut().and_then(|b| b.slice_request.take()) {
                changed |= self.slicer.start(&self.model, &f);
            }
            if let Some((name, fav)) = self.browser.as_mut().and_then(|b| b.export_request.take()) {
                if let Some(dir) = self.dialog(Dialog::ExportFolder) {
                    if let Some(b) = self.browser.as_mut() {
                        let _ = b.export_shortlist(&name, &dir, fav);
                    }
                    changed = true;
                }
            }
        }
        changed |= popups::handle(&mut self.app, &mut self.model, ev, &mut self.outbox);
        changed
    }

    /// Bring every control in line with the model.
    pub fn sync(&mut self) {
        // Only the controls whose state the panels change are repainted.
        let before = self.app.marks();
        // A new pad / layer selection shows that layer again.
        let e = self.model.engine;
        if self.model.sampling.as_ref().is_some_and(|s| s.selection != (e, self.model.pad[e], self.model.layer[e])) && self.model.sampler.state != 2 {
            self.model.sampling = None;
        }
        self.filefind.sync(&mut self.app);
        self.devices.sync(&mut self.app);
        self.midiconsole.sync(&mut self.app);
        self.trigger.sync(&mut self.app, &self.model);
        self.metadata.sync(&mut self.app);
        self.convertor.sync(&mut self.app);
        self.header.sync(&mut self.app, &self.model, self.learn.active);
        if self.sampler.sync(&mut self.app, &self.model) {
            self.pads.invalidate();
        }
        self.pads.sync(&mut self.app, &self.model);
        if let Some(t) = &self.editor_tabs {
            t.sync(&mut self.app, self.model.editor_tab);
        }
        if let Some(t) = &self.utility_tabs {
            t.sync(&mut self.app, self.model.utility_tab);
        }
        if let Some(p) = &mut self.pattern {
            p.sync(&mut self.app, &self.model);
        }
        self.inspector.sync(&mut self.app, &self.model);
        self.inspector.learn_buttons(&mut self.app, self.learn.active);
        for m in &mut self.mixers {
            m.sync(&mut self.app, &self.model);
        }
        if let Some(sc) = &mut self.scenes {
            sc.sync(&mut self.app, &self.model);
        }
        if let Some(mm) = &mut self.midimap {
            mm.sync(&mut self.app, &self.model);
        }
        self.slicer.sync(&mut self.app);
        self.modals.sync(&mut self.app);
        // Preferences › UI scaling / Help › Reset UI scale apply at once
        // ([re:695294] 0x153 → [re:2fbdeb] and
        // a host window resize).
        let ui = self.modals.options.int("LastEditorScaleFactor", 100).clamp(50, 300) as f32 / 100.0;
        if (ui - self.app.ui_scale).abs() > 1e-3 {
            self.app.ui_scale = ui;
            self.app.resize_request = true;
            self.app.dirty = true;
        }
        if self.learn.active {
            self.learn.collect(&self.app.root);
        } else {
            self.learn.rects.clear();
        }
        if let Some(b) = self.app.root.find_mut("LearnButton") {
            b.on = self.learn.active;
        }
        let active = self.learn.active;
        if let Some(o) = self.app.root.children.iter_mut().find(|c| c.name == "LearnOverlay") {
            o.visible = active;
        }
        if let Some(sg) = &mut self.song {
            sg.sync(&mut self.app, &self.model);
        }
        if let Some(b) = &mut self.browser {
            b.sync(&mut self.app, &self.model);
        }
        self.transmod.sync(&mut self.app, &self.model);
        popups::sync(&mut self.app, &self.model, &self.popup_paths);
        self.app.invalidate_changes(&before);
    }

    /// Render a frame (with the code-drawn panels).
    /// A complete fresh frame.
    pub fn render(&mut self, scale: f32) -> dreist_gui::tiny_skia::Pixmap {
        self.app.dirty = true;
        self.render_frame(scale).clone()
    }

    /// The window's frame: only damaged parts are repainted unless the
    /// whole window is dirty (`App::render_frame_with`).
    pub fn render_frame(&mut self, scale: f32) -> &dreist_gui::tiny_skia::Pixmap {
        if let Some(pp) = &self.pattern {
            pp.begin_frame();
        }
        let d = Drawer {
            model: &self.model,
            browser: self.browser.as_ref(),
            song: self.song.as_ref(),
            pattern: self.pattern.as_ref(),
            inspector: &self.inspector,
            midimap: self.midimap.as_ref(),
            slicer: &self.slicer,
            filefind: &self.filefind,
            devices: &self.devices,
            transmod: &self.transmod,
            midiconsole: &self.midiconsole,
            scenes: self.scenes.as_ref(),
            modals: &self.modals,
            learn: &self.learn,
        };
        self.app.render_frame_with(scale, Some(&d))
    }

    pub fn handle(&mut self, ev: &UiEvent) {
        if let UiEvent::ToolTip { path } = ev {
            self.update_tooltip(path);
            return;
        }
        if self.double_click_reset(ev) {
            return;
        }
        if self.knob_right_click(ev) {
            return;
        }
        // Pointer feedback (cursors, drop-target highlights) changes no
        // document state: no undo bookkeeping.
        // [re:ed3c05] (enter / leave a layer
        // knob): `e_highlightTransmodSource` 0x8 with the parameter's
        // modulation bit field, 0 on leave ([re:d0938a]
        //  [re:3be554] outlines the modulating Src buttons).
        if let UiEvent::HoverChanged { path } = ev {
            let mask = path
                .as_ref()
                .and_then(|p| self.inspector.modulation_mask(&self.app, &self.model, p).or_else(|| self.mixers.iter().find_map(|m| m.modulation_mask(&self.app, &self.model, p))))
                .unwrap_or(0);
            if self.transmod.highlight_sources(&mut self.app, mask) {
                self.app.dirty = true;
            }
            return;
        }
        if matches!(ev, UiEvent::Hover { .. } | UiEvent::DragOver { .. }) {
            if self.pointer_feedback(ev) {
                self.app.dirty = true;
            }
            return;
        }
        // Finder drags: feedback while moving, a drop like a browser file's.
        if let UiEvent::Files { to, files, x, y, dropped } = ev {
            self.external_drag = Some(files.clone());
            if !*dropped {
                self.app.cursor = self.drag_cursor(&[], to);
                self.pad_drag_flash(&[], &to.clone(), *x, *y);
                self.drop_highlights(&[], &to.clone());
                self.app.dirty = true;
                return;
            }
            let drop = UiEvent::Dropped { from: Vec::new(), to: to.clone(), x: *x, y: *y };
            self.handle(&drop);
            self.external_drag = None;
            return;
        }
        if let UiEvent::Collapse { path, iconic, end } = ev {
            self.collapse_notification(path, *iconic, *end);
            return;
        }
        // Splitter moves are layout only; the finished position is saved
        // under the splitter's `strAutoSaveName`.
        if let UiEvent::Split { path, pos, end } = ev {
            if *end {
                if let Some(key) = self.app.ctrl(path).and_then(|c| c.attr("strAutoSaveName")).filter(|k| !k.is_empty()) {
                    save_split(key, *pos);
                }
            }
            return;
        }
        // The drag is over: drag exit / drag end end the pad flash loops.
        if matches!(ev, UiEvent::Dropped { .. } | UiEvent::Mouse { kind: dreist_gui::app::MouseKind::Up, .. }) {
            self.pads.end_drag_flash();
            self.set_drop_highlights(None, None);
        }
        if let UiEvent::Shortcut { key, cmd, shift, alt, ctrl } = ev {
            if self.accelerator(key, accel::Mods { shift: *shift, cmd: *cmd, alt: *alt, ctrl: *ctrl }) {
                self.sync();
                self.app.dirty = true;
                return;
            }
        }
        if self.undo_redo(ev) {
            self.sync();
            return;
        }
        if self.learn_event(ev) {
            self.sync();
            return;
        }
        // Knob drags (value changes) and mouse presses with their drags are
        // gestures: one undo step each, no snapshot once it is recorded.
        let gesture =
            matches!(ev, UiEvent::ValueChanged { .. } | UiEvent::Mouse { kind: dreist_gui::app::MouseKind::Down | dreist_gui::app::MouseKind::RightDown | dreist_gui::app::MouseKind::Drag, .. });
        let before = (!self.history.mid_gesture(gesture)).then(|| self.model.doc.clone());
        let n = self.outbox.len();
        let sel_before = (self.model.engine, self.model.pad[self.model.engine], self.model.layer[self.model.engine]);
        let mut changed = self.route(ev);
        // "Pads track browser location": the browser follows the selected
        // pad's sample folder.
        let sel = (self.model.engine, self.model.pad[self.model.engine], self.model.layer[self.model.engine]);
        if sel.0 != sel_before.0 {
            self.retro.clear();
        }
        if sel != sel_before {
            if let Some(b) = self.browser.as_mut().filter(|b| b.pads_track) {
                let (e, p, l) = sel;
                let f = (0..8).map(|k| (l + k) % 8).find_map(|k| self.model.layer_sample(e, p, k)).and_then(|f| f.parent().map(std::path::Path::to_path_buf));
                if let Some(d) = f.filter(|d| d.is_dir()) {
                    b.follow(d);
                }
            }
        }
        // What the panels asked for while handling it (dialogs, files,
        // projects).
        for r in self.take_requests(n) {
            changed |= self.run_request(r);
        }
        // Pad edits apply to every selected pad.
        let extra = multipad::fan_out(&self.model, &self.outbox[n..]);
        self.outbox.extend(extra);
        for c in &self.outbox[n..] {
            self.model.apply(c);
        }
        // Record an undo step when the document changed (selection and
        // metronome changes are not steps).
        let before = before.map(|mut b| {
            multipad::carry_selection(&self.model.doc, &mut b);
            header::carry_metronome(&self.model.doc, &mut b);
            b
        });
        let doc_changed = before.as_ref().is_some_and(|b| *b != self.model.doc);
        let label = if doc_changed { self.undo_label(ev) } else { String::new() };
        let limit = self.undo_limit();
        self.history.after_event(gesture, before, doc_changed, label, limit);
        if changed || self.outbox.len() > n {
            self.sync();
        }
    }
}
