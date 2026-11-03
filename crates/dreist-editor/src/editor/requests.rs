//! Requests the panels raise while handling an event that need the editor
//! (native dialogs, files, projects, the engine), gathered after routing
//! and run in order.

use std::path::PathBuf;

use crate::*;

#[derive(Clone, Debug)]
pub(crate) enum Request {
    /// Preferences confirmed: the engine reads the options again.
    OptionsConfirmed,
    /// Save an effect chain or effect preset (file dialog).
    SavePreset(fxchain::SaveRequest),
    /// Swing menu › Import Guru shuffle / Import MIDI template.
    ImportGuruSwing,
    ImportMidiTemplate,
    /// Extract groove: pattern `n`'s timing as a swing template.
    ExtractGroove(usize),
    /// Preferences › Browse for the documents folder.
    BrowseDocuments,
    /// Missing files: a folder or file dialog.
    FileFind(filefind::Request),
    /// Export panel: choose the file and start the export job.
    ExportAudio,
    /// Open a project or Guru preset.
    OpenProject(PathBuf),
}

impl Editor {
    /// The requests raised since the outbox held `from` commands, in the
    /// order they are handled.
    pub(crate) fn take_requests(&mut self, from: usize) -> Vec<Request> {
        let mut out = Vec::new();
        if std::mem::take(&mut self.modals.options_confirmed) {
            out.push(Request::OptionsConfirmed);
        }
        out.extend(self.mixers.iter_mut().filter_map(|m| m.take_save_request()).map(Request::SavePreset));
        if let Some(pp) = self.pattern.as_mut() {
            if std::mem::take(&mut pp.import_guru_swing) {
                out.push(Request::ImportGuruSwing);
            }
            if std::mem::take(&mut pp.import_midi_template) {
                out.push(Request::ImportMidiTemplate);
            }
            // Swing menu › Extract timing: as the pattern menu's Extract groove.
            if let Some(n) = pp.groove.take() {
                self.patmenu.groove = Some(n);
            }
        }
        if let Some(n) = self.patmenu.groove.take() {
            out.push(Request::ExtractGroove(n));
        }
        if std::mem::take(&mut self.modals.browse_requested) {
            out.push(Request::BrowseDocuments);
        }
        if let Some(r) = self.filefind.request.take() {
            out.push(Request::FileFind(r));
        }
        if std::mem::take(&mut self.modals.export_requested) {
            out.push(Request::ExportAudio);
        }
        // Project loads come through the outbox (the editor opens them).
        let mut i = from;
        while i < self.outbox.len() {
            if let Command::LoadProject(path) = &self.outbox[i] {
                out.push(Request::OpenProject(path.clone()));
                self.outbox.remove(i);
            } else {
                i += 1;
            }
        }
        out
    }

    /// A file dialog (the host's).
    pub(crate) fn dialog(&mut self, d: Dialog) -> Option<PathBuf> {
        self.dialogs.as_mut().and_then(|f| f(d))
    }

    /// Run one request; whether the editor changed.
    pub(crate) fn run_request(&mut self, r: Request) -> bool {
        match r {
            Request::OptionsConfirmed => {
                let o = &self.modals.options;
                let m = command::msg(
                    dreist_engine::binding::id::REGISTRY_OPTIONS_CHANGED,
                    &[],
                    &[o.bool("ForcedLayersFallThrough", false) as i32 as f32, o.bool("SendActiveSensing", false) as i32 as f32],
                );
                self.outbox.push(m);
                false
            }
            Request::SavePreset(fxchain::SaveRequest::Chain(t)) => {
                if let (Some(p), Some(mut chain)) = (self.dialog(Dialog::SaveChainPreset), fxchain::chain_node(&self.model, t)) {
                    let p = p.with_extension("fxchain");
                    fxchain::set_chain_preset_name(&mut chain, &p.to_string_lossy());
                    if fxchain::save_preset_file(&p, &chain).is_ok() {
                        fxchain::set_chain_preset_name(fxchain::FxPanel::chain_mut(&mut self.model, t), &p.to_string_lossy());
                        self.mixers.iter().for_each(|m| m.forget_presets());
                    }
                }
                true
            }
            Request::SavePreset(fxchain::SaveRequest::Effect(t, slot, kind)) => {
                if let (Some(p), Some(mut n)) = (self.dialog(Dialog::SaveEffectPreset(kind)), fxchain::effect_node(&self.model, t, slot)) {
                    let p = p.with_extension("fxpreset");
                    let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    n.name = Some("Effect".into());
                    fxchain::set_effect_preset_name(&mut n, &name);
                    let _ = fxchain::save_preset_file(&p, &n);
                }
                true
            }
            // [re:00bec6]: the Guru
            // 1 text template into the user swing folder as
            // `<file stem>.swingtemplate`.
            Request::ImportGuruSwing => {
                if let Some(f) = self.dialog(Dialog::OpenGuruSwing) {
                    self.import_guru_swing(&f);
                }
                true
            }
            // The file's pattern (`LoadFromMIDIFile`) as a groove
            // (`ConvertPatternToSwingTemplate`) in the user swing folder.
            Request::ImportMidiTemplate => {
                if let Some(f) = self.dialog(Dialog::OpenMidiTemplate) {
                    self.import_midi_template(&f);
                }
                true
            }
            Request::ExtractGroove(n) => {
                if let Some(p) = self.dialog(Dialog::SaveSwingTemplate) {
                    let p = p.with_extension("swingtemplate");
                    let e = self.model.engine;
                    let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| self.model.pattern_name(e, n));
                    let t = swing::from_pattern(&self.model.pattern(e, n), &name);
                    let _ = swing::save(&p, &t);
                }
                true
            }
            // [re:0d12fb] $_22: the documents folder, written
            // at once.
            Request::BrowseDocuments => {
                let start = PathBuf::from(self.modals.documents_path());
                if let Some(p) = self.dialog(Dialog::ChooseDocuments(start)) {
                    self.modals.write_documents_path(&mut self.app, &p.to_string_lossy());
                }
                true
            }
            Request::FileFind(r) => {
                let d = match r {
                    filefind::Request::AutoFind => Dialog::ChooseFolder,
                    filefind::Request::Locate(_) => Dialog::LocateFile,
                };
                if let Some(p) = self.dialog(d) {
                    match r {
                        filefind::Request::AutoFind => {
                            self.modals.options.set("FileFindLastLocation", p.to_string_lossy());
                            self.filefind.start_auto_find(&mut self.app, &p);
                        }
                        filefind::Request::Locate(i) => {
                            self.filefind.manual_find(&mut self.model, i, &p);
                            self.outbox.push(self.model.rebuild_command());
                        }
                    }
                }
                true
            }
            // [re:c39372] → the export job; the panel stays
            // open with its progress (`PromptAndStartJob` [re:d6b539]).
            Request::ExportAudio => {
                if let Some(p) = self.dialog(Dialog::ExportAudio) {
                    let p = if p.extension().is_none() { p.with_extension("wav") } else { p };
                    let options = modal::export_options(&self.modals.options, self.model.engine);
                    let status = command::ExportProgress::default();
                    self.modals.export_job = Some(status.clone());
                    self.outbox.push(Command::ExportAudio { doc: Box::new(self.model.doc.clone()), samples: self.model.samples.clone(), path: p, options, reveal: false, status: Some(status) });
                } else {
                    self.modals.open = None;
                }
                true
            }
            Request::OpenProject(path) => self.open_project(&path),
        }
    }

    /// Open a project (`DoLoadPresetOnPath`) or a Guru preset
    /// (`DoLoadGuruPresetOnPath`: converted, no samples folder of its own)
    /// and rebuild the engine; whether it opened.
    fn open_project(&mut self, path: &std::path::Path) -> bool {
        if guru::kind(path) == Some(guru::Kind::Preset) {
            match guru::import_preset(path) {
                Ok(doc) => {
                    if !cfg!(test) {
                        add_recent_preset(path);
                    }
                    self.project_path = None;
                    self.open_document(doc, None, path.file_stem().map(|s| s.to_string_lossy().into_owned()));
                    self.outbox.push(self.model.rebuild_command());
                    return true;
                }
                Err(e) => {
                    if let Some(a) = self.app.root.path_of("LoadButton") {
                        self.app.open_menu(&a, vec![e], None);
                    }
                    return false;
                }
            }
        }
        // The profile's reader: a converted document, or one of another
        // extension family, is saved through Save As in the app's own.
        let Some((raw, converted)) = (dreist_formats::profile::get().read_project)(path) else { return false };
        let own = path.extension().is_some_and(|e| e.eq_ignore_ascii_case(dreist_formats::profile::get().save.project));
        self.project_path = (!converted && own).then(|| path.to_path_buf());
        // `DoLoadPresetOnPath` $_14: the preset joins the recent list.
        if !cfg!(test) {
            add_recent_preset(path);
        }
        let mut raw = raw;
        padmenu::carry_locked(&self.model.doc, self.model.samples.as_deref(), &mut raw);
        if !project::restore_outputs() {
            project::keep_outputs(&self.model.doc, &mut raw);
        }
        if options::Options::load().bool("BindingsLocked", false) {
            midimap::carry_bindings(&self.model.doc, &mut raw);
        }
        self.open_document(raw, Some(dreist_formats::model::samples_dir(path)), path.file_stem().map(|s| s.to_string_lossy().into_owned()));
        self.outbox.push(self.model.rebuild_command());
        true
    }
}
