//! Modal panels over the editor: About ([re:c045a2],
//! [re:3c1e07]) and Preferences ([re:00fdfe],
//! [re:0624c1] / [re:397244]).
//!
//! Preferences controls ↔ options (on showing): QuantizeREXButton
//! `AllocatorQuantizeREX`, PreFadeButton `AuxesArePreFade`, MultiButton
//! `PatternEditorRightClickMode`, ForceModeButton `ForcedLayersFallThrough`,
//! ResetLayersButton `AllocatorClearLayersOnLoad`, ActiveSensingButton
//! `SendActiveSensing`, RetardButton `SequencerRetardation`,
//! RestoreOutputsButton `AllocatorRestoreOutputs` (default on),
//! YAxisPadVelocityButton `PadVelocityBehaviour`, KeyboardButton
//! `RespondToKeyboard` (on), ExtendedKeyboardButton
//! `RespondToExtendedKeyboard`, AutoAssign `AutoMIDICC` (on); MIDIModeGroup
//! `MIDINumbering` (child ids, default 1); MetroType `MetronomeSound`
//! (metronome folders, "Default"); ShadingMode `PatternEditorShadingMode`
//! (Fixed, Adaptive, Cyclic, Off — [re:c192c5]);
//! RemMod `RemoveModBehavour` (Prompt, AlwaysClear, NeverClear,
//! ClearOnModifier — [re:d96028]); UIScaling
//! `LastEditorScaleFactor` (100–300 %, [re:ce8c40]);
//! UIStyle (stylesheets); UndoStages `MaxUndoSteps` (1..250, default 20);
//! DocumentsPath `LocalDocumentsPath`. Confirm writes, Cancel discards.
//!
//! The documents path is written at once, not on Confirm (ev_7e0eff81…,
//! ev_b8fdc944…): Browse ([re:448611]) asks for a folder ("Please choose
//! a new the original documents folder", starting at the current one); Reset
//! ([re:9ab38e]) asks "Are you sure you want to reset the Documents path
//! to %s?"; the text box ([re:c24122]) reverts to the stored path unless
//! the typed folder exists. Each write alerts "Changes will be applied the
//! next time you start Dreist" once per panel showing.

use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::render::Renderer;
use dreist_gui::{Colour, Ctrl, Rect, Skin};

use crate::options::Options;
use crate::{EditorError, err};

pub const TOGGLES: [(&str, &str, bool); 12] = [
    ("QuantizeREXButton", "AllocatorQuantizeREX", false),
    ("PreFadeButton", "AuxesArePreFade", false),
    ("MultiButton", "PatternEditorRightClickMode", false),
    ("ForceModeButton", "ForcedLayersFallThrough", false),
    ("ResetLayersButton", "AllocatorClearLayersOnLoad", false),
    ("ActiveSensingButton", "SendActiveSensing", false),
    ("RetardButton", "SequencerRetardation", false),
    ("RestoreOutputsButton", "AllocatorRestoreOutputs", true),
    ("YAxisPadVelocityButton", "PadVelocityBehaviour", false),
    ("KeyboardButton", "RespondToKeyboard", true),
    ("ExtendedKeyboardButton", "RespondToExtendedKeyboard", false),
    ("AutoAssign", "AutoMIDICC", true),
];
pub const SHADING: [&str; 4] = ["Fixed", "Adaptive", "Cyclic", "Off"];
pub const REMOVE_BEHAVIOUR: [&str; 4] = ["Prompt", "AlwaysClear", "NeverClear", "ClearOnModifier"];
pub const SCALINGS: [f32; 11] = [1.0, 1.1, 1.2, 1.25, 1.35, 1.5, 1.75, 2.0, 2.25, 2.5, 3.0];
pub const UNDO_DEFAULT: i64 = 20;

/// The default stylesheet ("v2") as the style menu shows it.
pub const DEFAULT_STYLE_LABEL: &str = "Default";

fn style_label(name: &str) -> &str {
    if name == dreist_gui::skin::default_theme() { DEFAULT_STYLE_LABEL } else { name }
}

/// The About panel's text.
/// The About text (the installed profile's).
pub fn about() -> &'static str {
    dreist_formats::profile::get().about
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    About,
    Preferences,
    Export,
}

/// [re:a3f6c0] normalisation items (`NormalizeComboBox`).
pub const NORMALIZE: [&str; 3] = ["Prompt if clipping detected", "Normalize if clipping detected", "Always Normalize"];

/// Export panel settings, kept in the options file.
pub fn export_options(o: &Options, engine: usize) -> dreist_engine::export::Options {
    use dreist_engine::export::{Files, Normalize, Source};
    dreist_engine::export::Options {
        source: match o.int("ExportSource", 2) {
            0 => Source::AllEngines,
            1 => Source::Engine(engine),
            _ => Source::Song,
        },
        files: match o.int("ExportOutputType", 0) {
            1 => Files::Single(o.int("ExportSingleOutput", 0).clamp(0, dreist_engine::SUB_OUTPUTS as i64) as usize),
            2 => Files::AllOutputs,
            _ => Files::Mixdown,
        },
        normalize: [Normalize::Prompt, Normalize::IfClipping, Normalize::Always][o.int("ExportNormalize", 1).clamp(0, 2) as usize],
        tail: crate::conv::number64(&o.string("ExportTail", "2")).unwrap_or(2.0).max(0.0),
        looped: o.bool("ExportLoop", false),
        auto_crop: o.bool("ExportAutoCrop", false),
        render: crate::options::render_options_from(o),
    }
}

pub struct Modals {
    /// A style chosen in Preferences, for the editor to apply now.
    pub style_changed: Option<String>,
    /// Preferences were confirmed: the editor tells the engine
    /// (`e_registryOptionsChanged`).
    pub options_confirmed: bool,
    about: Option<Vec<usize>>,
    prefs: Option<Vec<usize>>,
    export: Option<Vec<usize>>,
    /// Export panel's Export button was pressed (the editor asks for a path).
    pub export_requested: bool,
    pub open: Option<Kind>,
    /// The panel on screen (sliding in, shown or sliding out).
    shown: Option<Kind>,
    /// `shown` is sliding out.
    closing: bool,
    /// Preferences being edited (written on Confirm).
    staged: Options,
    pub options: Options,
    /// A UI scale tried from Preferences: the old factor and
    /// `LastEditorSize`, when it was applied, whether the prompt is open.
    scale_trial: Option<(i64, String, std::time::Instant, bool)>,
    /// The running export job ([re:a6f883]).
    pub export_job: Option<crate::command::ExportProgress>,
    /// Its prompt or failure alert is open.
    export_alert: bool,
    /// Preferences › Browse was pressed (the editor runs the folder dialog).
    pub browse_requested: bool,
    /// The documents-path alert was shown this time ([re:d8391c]).
    docs_alerted: bool,
    /// The reset question or the restart alert is open.
    docs_prompt: Option<DocsPrompt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DocsPrompt {
    Reset,
    Alert,
}

pub fn docs_restart() -> String {
    format!("Changes will be applied the next time you start {}", crate::product())
}
#[cfg_attr(not(feature = "window"), allow(dead_code))]
pub const DOCS_BROWSE_TITLE: &str = "Please choose a new documents folder";

/// [re:0d12fb]: the question asked 2 s after a new UI
/// scale was applied.
pub const KEEP_SCALE: &str = "Do you want to keep this UI scaling ratio?";

/// Dialog slide tags (`UiEvent::Animated`).
pub(crate) const DIALOG_IN: u32 = 0xd1a0;
pub(crate) const DIALOG_OUT: u32 = 0xd1a1;
/// Slide in / out duration used by every dialog (0x43fa0000).
pub(crate) const DIALOG_MS: f32 = 500.0;

/// [re:56a48f] with the top
/// edge (every caller): the dialog starts above the window, x centred
/// (W/2 − w/2), and slides down to y = 0 (exponential).
pub(crate) fn slide_in(app: &mut App, overlay: &[usize]) {
    let w = app.ctrl(overlay).map(|c| c.rect.w).unwrap_or(0);
    let mut p = overlay.to_vec();
    p.push(0);
    let Some(r) = app.ctrl(&p).map(|c| c.rect) else { return };
    let x = w / 2 - r.w / 2;
    app.animate_rect(&p, Rect::new(x, -r.h, r.w, r.h), Rect::new(x, 0, r.w, r.h), DIALOG_MS, dreist_gui::collapse::exponential, DIALOG_IN);
}

/// Slide out [re:2e49b1] (top edge, logarithmic):
/// back above the window; the shade goes when it finishes.
pub(crate) fn slide_out(app: &mut App, overlay: &[usize]) {
    let mut p = overlay.to_vec();
    p.push(0);
    let Some(r) = app.ctrl(&p).map(|c| c.rect) else { return };
    app.animate_rect(&p, r, Rect::new(r.x, -r.h, r.w, r.h), DIALOG_MS, dreist_gui::collapse::curve, DIALOG_OUT);
}

/// One dialog's slide state (for dialogs other than the `Modals` trio).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Slide {
    shown: bool,
    closing: bool,
}

impl Slide {
    /// Start sliding in / out to follow `open`; whether the overlay shows.
    pub fn sync(&mut self, app: &mut App, overlay: &[usize], open: bool) -> bool {
        if open && !self.shown {
            slide_in(app, overlay);
            self.shown = true;
            self.closing = false;
        } else if !open && self.shown && !self.closing {
            slide_out(app, overlay);
            self.closing = true;
        }
        self.shown
    }

    /// The slide-out finished: hide.
    pub fn animated(&mut self, app: &mut App, overlay: &[usize], ev: &UiEvent) -> bool {
        match ev {
            UiEvent::Animated { path, tag: DIALOG_OUT } if self.closing && path.starts_with(overlay) => {
                self.shown = false;
                self.closing = false;
                if let Some(o) = app.ctrl_mut(overlay) {
                    o.visible = false;
                }
                true
            }
            _ => false,
        }
    }
}

pub(crate) fn overlay(skin: &mut Skin, root: &mut Ctrl, rel: &str, name: &str) -> Result<Option<Vec<usize>>, EditorError> {
    let (w, h) = (root.rect.w, root.rect.h);
    let Some(rel) = skin.find(rel).or_else(|| Some(rel.to_string())) else { return Ok(None) };
    let Some(mut panel) = skin.load_skin(&rel).map_err(err)?.children.into_iter().next() else { return Ok(None) };
    let (pw, ph) = (panel.rect.w.min(w), panel.rect.h.min(h));
    panel.set_rect(Rect::new((w - pw) / 2, (h - ph) / 2, pw, ph));
    let mut o = Ctrl {
        class: "CtrlPanel".into(),
        name: name.into(),
        overload: "ModalOverlay".into(),
        style: "InvisibleStyle".into(),
        rect: Rect::new(0, 0, w, h),
        zorder: 1001,
        visible: false,
        enabled: true,
        ..Ctrl::default()
    };
    o.children.push(panel);
    root.children.push(o);
    Ok(Some(vec![root.children.len() - 1]))
}

fn camel(s: &str) -> String {
    crate::transmod::camel_words(s)
}

impl Modals {
    pub fn build(skin: &mut Skin, root: &mut Ctrl) -> Result<Modals, EditorError> {
        let about = overlay(skin, root, "AboutPanel.skin", "AboutOverlay")?;
        let prefs = overlay(skin, root, "OptionsPanel.skin", "PreferencesOverlay")?;
        let export = overlay(skin, root, "ExportPanel.skin", "ExportOverlay")?;
        // The progress bar shows only while a job runs (timer tick).
        if let Some(c) = export.as_ref().and_then(|p| {
            let mut c = &mut *root;
            for &i in p {
                c = c.children.get_mut(i)?;
            }
            c.find_mut("ProgressBar")
        }) {
            c.visible = false;
        }
        let options = Options::load();
        crate::pattern::set_shading_mode(&options.string("PatternEditorShadingMode", SHADING[0]));
        Ok(Modals {
            style_changed: None,
            options_confirmed: false,
            about,
            prefs,
            export,
            export_requested: false,
            open: None,
            shown: None,
            closing: false,
            staged: options.clone(),
            options,
            scale_trial: None,
            export_job: None,
            export_alert: false,
            browse_requested: false,
            docs_alerted: false,
            docs_prompt: None,
        })
    }

    #[cfg(test)]
    pub fn prefs_path(&self) -> Option<Vec<usize>> {
        self.prefs.clone()
    }

    fn path(&self, k: Kind) -> Option<&Vec<usize>> {
        match k {
            Kind::About => self.about.as_ref(),
            Kind::Preferences => self.prefs.as_ref(),
            Kind::Export => self.export.as_ref(),
        }
    }

    fn metronomes(&self) -> Vec<String> {
        let dir = dreist_engine::metronome::default_metronomes_dir();
        let mut v: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        if v.is_empty() {
            v.push("Default".into());
        }
        v
    }

    fn styles(&self) -> Vec<String> {
        dreist_gui::Skin::themes()
    }

    pub fn show(&mut self, app: &mut App, k: Kind) {
        self.open = Some(k);
        self.staged = self.options.clone();
        self.docs_alerted = false;
        if k == Kind::About {
            if let Some(o) = self.about.clone().and_then(|p| app.ctrl_mut(&p)) {
                o.walk_mut(&mut |c: &mut Ctrl| match c.name.as_str() {
                    "Version" => c.text = format!("Version {}", env!("CARGO_PKG_VERSION")),
                    "DateLabel" => c.text = String::new(),
                    "AboutLabel" => c.text = about().into(),
                    _ => {}
                });
            }
        }
    }

    pub fn sync(&mut self, app: &mut App) {
        // Slide the shown dialog out when it closed or another opened, then
        // the new one in.
        if self.shown.is_some() && self.shown != self.open && !self.closing {
            if let Some(p) = self.shown.and_then(|k| self.path(k).cloned()) {
                slide_out(app, &p);
                self.closing = true;
            } else {
                self.shown = None;
            }
        }
        if self.shown.is_none() {
            if let Some(p) = self.open.and_then(|k| self.path(k).cloned()) {
                slide_in(app, &p);
                self.shown = self.open;
            }
        }
        let open = self.open;
        let shown = self.shown;
        for k in [Kind::About, Kind::Preferences, Kind::Export] {
            if let Some(o) = self.path(k).cloned().and_then(|p| app.ctrl_mut(&p)) {
                o.visible = shown == Some(k);
            }
        }
        if open == Some(Kind::Export) {
            self.sync_export(app);
            return;
        }
        if open != Some(Kind::Preferences) {
            return;
        }
        let s = self.staged.clone();
        let styles = self.styles();
        let Some(p) = self.prefs.clone() else { return };
        let editing = app.is_editing();
        let Some(o) = app.ctrl_mut(&p) else { return };
        o.walk_mut(&mut |c: &mut Ctrl| {
            if let Some((_, key, d)) = TOGGLES.iter().find(|(n, _, _)| *n == c.name) {
                c.on = s.bool(key, *d);
                return;
            }
            match c.name.as_str() {
                "MIDIModeGroup" => {
                    let n = s.int("MIDINumbering", 1);
                    for b in &mut c.children {
                        b.on = b.id as i64 == n;
                    }
                }
                "MetroType" => c.text = s.string("MetronomeSound", "Default"),
                "ShadingMode" => c.text = s.string("PatternEditorShadingMode", SHADING[0]),
                "RemMod" => c.text = camel(&s.string("RemoveModBehavour", REMOVE_BEHAVIOUR[0])),
                "UIScaling" => c.text = format!("{} %", s.int("LastEditorScaleFactor", 100)),
                "UIStyle" => c.text = style_label(&s.string("StyleSheet", styles.iter().find(|n| n.as_str() == dreist_gui::skin::default_theme()).map(String::as_str).unwrap_or(""))).to_string(),
                "UndoStages" => {
                    let v = s.int("MaxUndoSteps", UNDO_DEFAULT).clamp(1, 250);
                    c.set_attr("realMin", "1".into());
                    c.set_attr("realMax", "250".into());
                    c.value = v as f32;
                    c.text = v.to_string();
                }
                "DocumentsPath" if !editing => c.text = s.string("LocalDocumentsPath", &default_documents()),
                _ => {}
            }
        });
    }

    /// [re:f988fc].
    fn sync_export(&mut self, app: &mut App) {
        let o = self.options.clone();
        let editing = app.is_editing();
        let Some(p) = self.export.clone() else { return };
        let Some(panel) = app.ctrl_mut(&p) else { return };
        let (src, ty) = (o.int("ExportSource", 2), o.int("ExportOutputType", 0));
        panel.walk_mut(&mut |c: &mut Ctrl| match c.name.as_str() {
            "SourceButtonGroup" => c.children.iter_mut().enumerate().for_each(|(i, b)| b.on = i as i64 == src),
            "OutputTypeButtonGroup" => c.children.iter_mut().enumerate().for_each(|(i, b)| b.on = i as i64 == ty),
            "SingleOutputComboBox" => {
                c.enabled = ty == 1;
                c.text = dreist_engine::export::output_name(o.int("ExportSingleOutput", 0).max(0) as usize);
            }
            "NormalizeComboBox" => c.text = NORMALIZE[o.int("ExportNormalize", 1).clamp(0, 2) as usize].into(),
            "TailTextBox" if !editing => c.text = o.string("ExportTail", "2"),
            "LoopButton" => c.on = o.bool("ExportLoop", false),
            "AutoCropButton" => c.on = o.bool("ExportAutoCrop", false),
            _ => {}
        });
    }

    fn handle_export(&mut self, app: &mut App, ev: &UiEvent) -> bool {
        match ev {
            UiEvent::Clicked { path, name, on, .. } if self.mine(path) => {
                let parent = app.ctrl(&path[..path.len() - 1]).map(|c| c.name.clone()).unwrap_or_default();
                let i = *path.last().unwrap_or(&0) as i64;
                match (parent.as_str(), name.as_str()) {
                    ("SourceButtonGroup", _) => self.options.set("ExportSource", i),
                    ("OutputTypeButtonGroup", _) => self.options.set("ExportOutputType", i),
                    (_, "SingleOutputComboBox") => app.open_menu(path, (0..=dreist_engine::SUB_OUTPUTS).map(dreist_engine::export::output_name).collect(), None),
                    (_, "NormalizeComboBox") => app.open_menu(path, NORMALIZE.iter().map(|s| s.to_string()).collect(), None),
                    (_, "LoopButton") => self.options.set("ExportLoop", *on),
                    (_, "AutoCropButton") => self.options.set("ExportAutoCrop", *on),
                    // Cancel aborts a running job, else closes.
                    (_, "CancelButton") => match &self.export_job {
                        Some(j) => {
                            if let Ok(mut g) = j.lock() {
                                g.abort = true;
                            }
                        }
                        None => self.open = None,
                    },
                    (_, "StartButton") if self.export_job.is_none() => self.export_requested = true,
                    _ => return true,
                }
                self.options.save();
                true
            }
            UiEvent::MenuSelected { anchor, name, index } if self.mine(anchor) => {
                match name.as_str() {
                    "SingleOutputComboBox" => self.options.set("ExportSingleOutput", *index),
                    "NormalizeComboBox" => self.options.set("ExportNormalize", *index),
                    _ => {}
                }
                self.options.save();
                true
            }
            UiEvent::TextCommitted { path, name, text } if self.mine(path) && name == "TailTextBox" => {
                let v = crate::conv::number64(text).unwrap_or(2.0).clamp(0.0, 60.0);
                self.options.set("ExportTail", v);
                self.options.save();
                true
            }
            UiEvent::Mouse { path, .. } | UiEvent::Wheel { path, .. } | UiEvent::Clicked { path, .. } | UiEvent::ValueChanged { path, .. } => self.mine(path),
            _ => false,
        }
    }

    fn mine(&self, path: &[usize]) -> bool {
        self.open.and_then(|k| self.path(k)).is_some_and(|p| path.starts_with(p))
    }

    /// Menu items for a preferences popup.
    fn items(&self, name: &str) -> Option<Vec<String>> {
        Some(match name {
            "MetroType" => self.metronomes(),
            "ShadingMode" => SHADING.iter().map(|s| s.to_string()).collect(),
            "RemMod" => REMOVE_BEHAVIOUR.iter().map(|s| camel(s)).collect(),
            "UIScaling" => SCALINGS.iter().map(|f| format!("{} %", (f * 100.0).floor() as i32)).collect(),
            "UIStyle" => self.styles().iter().map(|n| style_label(n).to_string()).collect(),
            _ => return None,
        })
    }

    /// [re:6fb3f8] while the job runs: the progress
    /// bar shows; a clipping question opens as an alert (Yes normalises);
    /// at the end a failure shows "Export failed. Error reason:\n%s" and
    /// the panel closes (500 ms Logarithmic).
    fn tick_export(&mut self, app: &mut App) -> bool {
        let Some(job) = self.export_job.clone() else { return false };
        let (progress, prompt, result) = {
            let Ok(g) = job.lock() else { return false };
            (g.progress, g.prompt.clone().filter(|_| g.answer.is_none()), g.result.clone())
        };
        let panel = self.export.clone();
        let bar = panel.as_ref().and_then(|p| app.ctrl(p).and_then(|c| c.path_of("ProgressBar")).map(|q| [p.clone(), q].concat()));
        if let Some(c) = bar.as_ref().and_then(|b| app.ctrl_mut(b)) {
            c.visible = result.is_none();
            c.value = progress.clamp(0.0, 1.0);
        }
        if let Some(q) = prompt {
            if !self.export_alert {
                self.export_alert = true;
                let anchor = bar.clone().unwrap_or_default();
                app.open_menu(&anchor, vec![q, String::new(), "Yes".into(), "No".into()], None);
            }
            return true;
        }
        if let Some(r) = result {
            self.export_job = None;
            if let Err(e) = r.filter_err() {
                self.export_alert = true;
                let anchor = bar.unwrap_or_default();
                app.open_menu(&anchor, vec![format!("Export failed. Error reason:\n{e}")], None);
            }
            self.open = None;
        }
        true
    }

    /// After 2 s of a tried UI scale, the keep question (an alert menu on
    /// the scaling popup). Returns whether it opened.
    pub fn tick(&mut self, app: &mut App) -> bool {
        if self.tick_export(app) {
            return true;
        }
        let Some((_, _, t, asked)) = &mut self.scale_trial else { return false };
        if *asked || t.elapsed().as_secs_f32() < 2.0 {
            return false;
        }
        *asked = true;
        let anchor = self.prefs.as_ref().and_then(|p| app.ctrl(p).and_then(|c| c.path_of("UIScaling")).map(|q| [p.clone(), q].concat())).unwrap_or_default();
        app.open_menu(&anchor, vec![KEEP_SCALE.to_string(), String::new(), "Yes".into(), "No".into()], None);
        true
    }

    /// The keep answer: "No" restores the old scale and size (options
    /// written back), anything else keeps the new one.
    fn keep_scale(&mut self, keep: bool) {
        let Some((old, old_size, _, _)) = self.scale_trial.take() else { return };
        if keep {
            return;
        }
        for o in [&mut self.options, &mut self.staged] {
            o.set("LastEditorScaleFactor", old);
            o.set("LastEditorSize", &old_size);
        }
        self.options.save();
    }

    /// The documents path control (anchor for the alerts).
    fn docs_anchor(&self, app: &App) -> Vec<usize> {
        self.prefs.as_ref().and_then(|p| app.ctrl(p).and_then(|c| c.path_of("DocumentsPath")).map(|q| [p.clone(), q].concat())).unwrap_or_default()
    }

    /// [re:eb8243] straight away, then the
    /// restart alert the first time this showing.
    pub fn write_documents_path(&mut self, app: &mut App, path: &str) {
        for o in [&mut self.options, &mut self.staged] {
            o.set("LocalDocumentsPath", path);
        }
        self.options.save();
        if !self.docs_alerted {
            self.docs_alerted = true;
            self.docs_prompt = Some(DocsPrompt::Alert);
            let anchor = self.docs_anchor(app);
            app.open_menu(&anchor, vec![docs_restart()], None);
        }
        self.sync(app);
    }

    /// The folder the Browse dialog starts in.
    pub fn documents_path(&self) -> String {
        self.options.string("LocalDocumentsPath", &default_documents())
    }

    pub fn handle(&mut self, app: &mut App, ev: &UiEvent) -> bool {
        if let Some(q) = self.docs_prompt {
            if let UiEvent::MenuSelected { index, .. } = ev {
                self.docs_prompt = None;
                // Yes / No prompt: 0 = Yes (item 2).
                if q == DocsPrompt::Reset && *index == 2 {
                    self.write_documents_path(app, &default_documents());
                }
                return true;
            }
            if app.menu.is_none() {
                self.docs_prompt = None;
            }
        }
        // The export alerts: the clipping question's answer (Yes = item 2).
        if self.export_alert {
            if let UiEvent::MenuSelected { index, .. } = ev {
                self.export_alert = false;
                if let Some(Ok(mut g)) = self.export_job.as_ref().map(|j| j.lock()) {
                    if g.prompt.is_some() && g.answer.is_none() {
                        g.answer = Some(*index == 2);
                    }
                }
                return true;
            }
            if app.menu.is_none() {
                self.export_alert = false;
                if let Some(Ok(mut g)) = self.export_job.as_ref().map(|j| j.lock()) {
                    if g.prompt.is_some() && g.answer.is_none() {
                        g.answer = Some(false);
                    }
                }
            }
        }
        if let Some((.., true)) = self.scale_trial {
            if let UiEvent::MenuSelected { index, .. } = ev {
                self.keep_scale(*index != 3);
                return true;
            }
            if app.menu.is_none() {
                self.keep_scale(true);
            }
        }
        if let UiEvent::Animated { path, tag: DIALOG_OUT } = ev {
            if self.closing && self.shown.and_then(|k| self.path(k)).is_some_and(|p| path.starts_with(p)) {
                self.closing = false;
                self.shown = None;
                self.sync(app);
                return true;
            }
        }
        let Some(k) = self.open else { return false };
        if k == Kind::Export {
            return self.handle_export(app, ev);
        }
        match ev {
            UiEvent::Mouse { path, kind: MouseKind::Down, .. } if k == Kind::About && self.mine(path) => {
                self.open = None;
                true
            }
            UiEvent::Clicked { path, name, on, .. } if self.mine(path) => {
                if let Some((_, key, _)) = TOGGLES.iter().find(|(n, _, _)| n == name) {
                    self.staged.set(key, *on);
                    return true;
                }
                let parent = app.ctrl(&path[..path.len() - 1]).map(|c| c.name.clone()).unwrap_or_default();
                if parent == "MIDIModeGroup" {
                    let id = app.ctrl(path).map(|c| c.id).unwrap_or(0);
                    self.staged.set("MIDINumbering", id);
                    return true;
                }
                if let Some(items) = self.items(name) {
                    app.open_menu(path, items, None);
                    return true;
                }
                match name.as_str() {
                    "ConfirmButton" => {
                        self.options = self.staged.clone();
                        self.options.save();
                        self.options_confirmed = true;
                        crate::pattern::set_shading_mode(&self.options.string("PatternEditorShadingMode", SHADING[0]));
                        self.open = None;
                    }
                    "CancelButton" => self.open = None,
                    "ResetPathButton" => {
                        let q = format!("Are you sure you want to reset the Documents path to {}?", default_documents());
                        self.docs_prompt = Some(DocsPrompt::Reset);
                        app.open_menu(path, vec![q, String::new(), "Yes".into(), "No".into()], None);
                    }
                    "BrowseButton" => self.browse_requested = true,
                    _ => {}
                }
                true
            }
            UiEvent::MenuSelected { anchor, name, index } if self.mine(anchor) => {
                let Some(items) = self.items(name) else { return true };
                let Some(v) = items.get(*index) else { return true };
                match name.as_str() {
                    "MetroType" => self.staged.set("MetronomeSound", v),
                    "ShadingMode" => self.staged.set("PatternEditorShadingMode", v),
                    "RemMod" => self.staged.set("RemoveModBehavour", REMOVE_BEHAVIOUR[(*index).min(3)]),
                    // [re:0d12fb] [re:e257c1]: a new scale
                    // applies at once (scale, host resize, options
                    // `LastEditorScaleFactor` / `LastEditorSize`), and 2 s
                    // later the [re:275d67] callback asks whether to keep it.
                    "UIScaling" => {
                        let new = (SCALINGS[(*index).min(10)] * 100.0).floor() as i64;
                        let old = self.options.int("LastEditorScaleFactor", 100);
                        if new != old {
                            let (w, h) = app.size();
                            let f = new as f32 / 100.0;
                            let old_size = self.options.string("LastEditorSize", "0,0");
                            self.scale_trial = Some((old, old_size, std::time::Instant::now(), false));
                            for o in [&mut self.options, &mut self.staged] {
                                o.set("LastEditorScaleFactor", new);
                                o.set("LastEditorSize", format!("{},{}", (w as f32 * f).floor() as i32, (h as f32 * f).floor() as i32));
                            }
                            self.options.save();
                        }
                    }
                    // [re:0d12fb] [re:dfe5ca]: written at once
                    // and applied ([re:cac947], repaint).
                    "UIStyle" => {
                        let name = if v == DEFAULT_STYLE_LABEL { dreist_gui::skin::default_theme().to_string() } else { v.to_string() };
                        for o in [&mut self.options, &mut self.staged] {
                            o.set("StyleSheet", &name);
                        }
                        self.options.save();
                        self.style_changed = Some(name);
                    }
                    _ => {}
                }
                true
            }
            UiEvent::ValueChanged { path, name, value, .. } if self.mine(path) && name == "UndoStages" => {
                self.staged.set("MaxUndoSteps", *value as i64);
                true
            }
            UiEvent::TextCommitted { path, name, text } if self.mine(path) && name == "DocumentsPath" => {
                let t = text.trim();
                if std::path::Path::new(t).is_dir() {
                    if t != self.documents_path() {
                        self.write_documents_path(app, t);
                    }
                } else {
                    // Not a folder: back to the stored path.
                    let stored = self.documents_path();
                    if let Some(c) = app.ctrl_mut(path) {
                        c.text = stored;
                    }
                }
                true
            }
            // Modal: nothing else reaches the editor while open.
            UiEvent::Mouse { path, .. } | UiEvent::Wheel { path, .. } | UiEvent::Clicked { path, .. } | UiEvent::ValueChanged { path, .. } => self.mine(path),
            _ => false,
        }
    }

    pub fn draw(&self, _r: &Renderer, p: &mut Painter, c: &Ctrl) -> bool {
        if c.overload != "ModalOverlay" {
            return false;
        }
        // The dialog shade (colour 9 = (32, 32, 32, 180)).
        p.fill_rect(RectF::new(0.0, 0.0, c.rect.w as f32, c.rect.h as f32), Colour::rgba(32, 32, 32, 180));
        true
    }
}

/// [re:9c6245].
pub fn default_documents() -> String {
    crate::options::default_documents()
}

/// Errors worth an alert (an abort is not).
trait FilterErr {
    fn filter_err(self) -> Result<(), String>;
}

impl FilterErr for Result<Vec<std::path::PathBuf>, String> {
    fn filter_err(self) -> Result<(), String> {
        match self {
            Err(e) if e != "aborted" => Err(e),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;
    use dreist_engine::export::{Files, Normalize, Source};

    #[test]
    fn export_options_from_settings() {
        let mut o = Options::default();
        let d = export_options(&o, 3);
        assert_eq!((d.source, d.files, d.normalize, d.tail, d.looped), (Source::Song, Files::Mixdown, Normalize::IfClipping, 2.0, false));
        o.set("ExportSource", 1);
        o.set("ExportOutputType", 1);
        o.set("ExportSingleOutput", 5);
        o.set("ExportNormalize", 2);
        o.set("ExportTail", "0.5");
        o.set("ExportLoop", true);
        let x = export_options(&o, 3);
        assert_eq!((x.source, x.files, x.normalize, x.tail, x.looped), (Source::Engine(3), Files::Single(5), Normalize::Always, 0.5, true));
    }
}
