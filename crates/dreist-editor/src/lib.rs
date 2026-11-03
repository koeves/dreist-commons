//! The v2 editor: the installed skin (via `dreist-gui`) assembled as
//! [re:e6e2c9] does, with the the original panels bound to a
//! [`Model`] and emitting [`Command`]s for the engine. See
//! the dreist-re GUI notes.

mod accel;
pub mod browser;
pub mod command;
mod conv;
mod convertor;
mod cursors;
pub mod devices;
mod dragdrop;
mod editbind;
mod editor;
pub mod filefind;
mod fxchain;
mod fxtext;
mod guru;
mod header;
mod inspector;
mod learn;
mod metadata;
mod meters;
mod midiconsole;
mod midimap;
mod mixer;
mod modal;
pub mod model;
mod multipad;
pub mod options;
mod padmenu;
mod pads;
mod patmenu;
mod pattern;
mod popups;
pub mod project;
mod retro;
mod sampler;
mod scenes;
mod search;
mod slicer;
mod song;
mod swing;
mod tabs;
mod timeline;
mod tooltips;
mod transmod;
mod transmodvis;
mod trigger;
mod undo;
mod velsplit;
mod zoomslider;

use dreist_gui::app::{App, UiEvent};
use dreist_gui::{Ctrl, Skin};

pub use command::Command;
use editor::engine_menu::EngineAction;
use editor::knobs::KnobAction;
pub use model::Model;

/// What the file dialogs are asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    OpenProject,
    SaveProject,
    ExportAudio,
    /// Missing files: Auto Find start folder.
    ChooseFolder,
    /// Source list › Export to folder (`ExportShortlist` [re:24982a]): the
    /// folder to copy the shortlist into (from the desktop).
    ExportFolder,
    /// Missing files: Find Selected.
    LocateFile,
    /// Extract groove: where to save the `.swingtemplate`.
    SaveSwingTemplate,
    /// Pad menu › Save layer sample...: where to write the copy.
    /// Default file name.
    SaveSample(String),
    /// FX chain menu › Save preset (`DoSaveCurrentPreset`): a `.fxchain`.
    SaveChainPreset,
    /// Effect menu › Save preset: a `.fxpreset` for the effect type.
    SaveEffectPreset(usize),
    /// File menu › Open Engine / Kit / others: a part file.
    OpenPart(project::Part),
    /// File menu › Save … As: a part file.
    SavePart(project::Part),
    /// File menu › Save others › Save Pattern As MIDI: a `.mid`.
    SaveMidi,
    /// File menu › Open others › Open Pattern From MIDI: a `.mid`.
    OpenMidi,
    /// Swing menu › Import Guru shuffle: a Guru 1 `.txt` template.
    OpenGuruSwing,
    /// Swing menu › Import MIDI template: a `.mid` ("Please choose the
    /// file(s) you wish to import").
    OpenMidiTemplate,
    /// Preferences › Browse: the new documents folder, starting at the
    /// current one.
    ChooseDocuments(std::path::PathBuf),
}

/// The product name shown in the UI (the installed profile's).
pub fn product() -> &'static str {
    dreist_formats::profile::get().product
}

/// File menu items ([re:4139f8] $_21 [re:3ce6a1]).
#[derive(Debug, Clone)]
enum FileAction {
    None,
    OpenPreset,
    RecentMenu,
    Recent(std::path::PathBuf),
    OthersMenu,
    SaveOthersMenu,
    Open(project::Part),
    SavePreset { as_new: bool },
    Save { part: project::Part, as_new: bool },
    SaveMidi,
    OpenMidi,
}

/// [re:b86a03] [re:2761f5]: five entries,
/// newest first, kept as options `RecentDoc0..4`.
const RECENT: usize = 5;

pub fn recent_presets() -> Vec<std::path::PathBuf> {
    let o = options::Options::load();
    (0..RECENT).map(|i| o.string(&format!("RecentDoc{i}"), "")).filter(|v| !v.is_empty()).map(std::path::PathBuf::from).collect()
}

/// [re:bc555c]: move `path` to the front, keep five.
pub fn add_recent_preset(path: &std::path::Path) {
    let mut list = recent_presets();
    list.retain(|p| p != path);
    list.insert(0, path.to_path_buf());
    list.truncate(RECENT);
    let mut o = options::Options::load();
    for i in 0..RECENT {
        o.set(&format!("RecentDoc{i}"), list.get(i).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());
    }
    o.save();
}

/// [re:7f1242]: "w,h", both > 0.
fn parse_size(s: &str) -> Option<(i32, i32)> {
    let (w, h) = s.split_once(',')?;
    let (w, h) = (w.trim().parse::<i32>().ok()?, h.trim().parse::<i32>().ok()?);
    (w > 0 && h > 0).then_some((w, h))
}

/// [re:48ffa9] (the
/// port's options file; tests leave it alone).
fn save_split(key: &str, pos: i32) {
    if cfg!(test) {
        return;
    }
    let mut o = options::Options::load();
    o.set(key, pos);
    o.save();
}

/// `Editor::open` → [re:4ca6d7] for every
/// splitter: a saved position ≥ 0 is clamped and applied.
fn restore_splits(c: &mut Ctrl, o: &options::Options) {
    if c.class == "CtrlSplitterPanel" {
        if let Some(key) = c.attr("strAutoSaveName").filter(|k| !k.is_empty()).map(str::to_string) {
            let v = o.int(&key, -1);
            if v >= 0 {
                let p = dreist_gui::splitter::clamp(c, v as i32);
                dreist_gui::splitter::set_position(c, p);
            }
        }
    }
    for ch in &mut c.children {
        restore_splits(ch, o);
    }
}

pub type DialogFn = Box<dyn FnMut(Dialog) -> Option<std::path::PathBuf> + Send>;

pub struct Editor {
    /// The standalone app: its menu bar carries the keyboard shortcuts, so
    /// they work whatever the keyboard options say (`accel`).
    pub menu_bar: bool,
    /// The window is closing: the host link's last call (hand the
    /// document back now).
    pub closing: bool,
    /// Files being dragged in from outside the window.
    pub(crate) external_drag: Option<Vec<std::path::PathBuf>>,
    /// Pattern key / engine selector drop highlights.
    pub(crate) drop_hl: dragdrop::DropHighlights,
    /// Root size from the skin ([re:93103c]) and the
    /// size last written to `LastEditorSize`.
    window_size: ((i32, i32), (i32, i32)),
    pub app: App,
    /// Native file dialogs (set by the window host; `None` in tests).
    pub dialogs: Option<DialogFn>,
    /// Where the project was last opened/saved.
    pub project_path: Option<std::path::PathBuf>,
    /// Undo / redo steps (document snapshots).
    history: undo::History,
    /// Undo (false) / redo (true) history menu open.
    history_menu: Option<bool>,
    /// A knob's right-click menu: the control and the items' actions.
    knob_menu: Option<(Vec<usize>, String, Vec<KnobAction>)>,
    /// An open engine selector menu (anchor, engine, item actions); the
    /// "Reset all engines" confirmation; the engine being renamed.
    engine_menu: Option<(Vec<usize>, usize, Vec<EngineAction>)>,
    engine_rename: Option<usize>,
    /// An open pad bank menu: the button and its bank.
    bank_menu: Option<(Vec<usize>, usize)>,
    /// A typed knob edit in progress: the knob, its id and the conversion.
    knob_edit: Option<(Vec<usize>, String, i32, dreist_formats::params::Curve)>,
    /// The open File menu level (main menu or a submenu) and its actions.
    file_menu: Option<Vec<FileAction>>,
    /// Last file per part type and engine ([re:4196af] dialog paths): what
    /// "Save Engine" / "Save Kit" / … write to without asking.
    part_paths: std::collections::HashMap<(&'static str, usize), std::path::PathBuf>,
    /// Undo name of the last drop (`CreateEngineUndo` texts).
    drop_label: String,
    pub model: Model,
    /// Commands produced since the last `take_commands`.
    pub outbox: Vec<Command>,
    pads: pads::Pads,
    sampler: sampler::Sampler,
    header: header::Header,
    editor_tabs: Option<tabs::Tabs>,
    utility_tabs: Option<tabs::Tabs>,
    pattern: Option<pattern::PatternPanel>,
    inspector: inspector::Inspector,
    mixers: Vec<mixer::Mixer>,
    scenes: Option<scenes::Scenes>,
    song: Option<song::SongPanel>,
    browser: Option<browser::Browser>,
    transmod: transmod::Transmod,
    midimap: Option<midimap::MidiMap>,
    slicer: slicer::Slicer,
    padmenu: padmenu::PadMenu,
    /// The open layer row menu: its anchor and (engine, pad, layer).
    layer_menu: Option<(Vec<usize>, (usize, usize, usize))>,
    filefind: filefind::FileFind,
    /// Standalone audio / MIDI settings panels.
    pub devices: devices::Devices,
    midiconsole: midiconsole::MidiConsole,
    trigger: trigger::TriggerPanel,
    metadata: metadata::MetaData,
    retro: retro::PatternRetro,
    convertor: convertor::Convertor,
    patmenu: patmenu::PatMenu,
    clipboard: padmenu::Clipboard,
    modals: modal::Modals,
    learn: learn::Learn,
    /// The popup buttons `popups::sync` labels.
    popup_paths: dreist_gui::app::PathCache,
}

#[derive(Debug)]
pub struct EditorError(pub String);

impl std::fmt::Display for EditorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EditorError {}

fn err(e: impl std::fmt::Display) -> EditorError {
    EditorError(e.to_string())
}

impl Editor {
    /// Build the editor from the installed resources (`theme`, e.g. "Dreist").
    pub fn new(theme: &str, model: Model) -> Result<Editor, EditorError> {
        let mut skin = Skin::load(&Skin::resources_dir(), theme).map_err(err)?;
        let mut root = skin.load_skin("RootSkin.skin").map_err(err)?;
        let header = header::Header::build(&mut skin, &mut root)?;
        let pads = pads::Pads::build(&mut skin, &mut root, &model)?;
        let sampler = sampler::Sampler::build(&mut skin, &mut root)?;
        let editor_tabs = tabs::Tabs::new(&root, "EditorTabPanel", "EditorTabButtons");
        let utility_tabs = tabs::Tabs::new(&root, "UtilityTabPanel", "UtilityTabButtonGroup");
        let pattern = match &editor_tabs {
            Some(t) => {
                let page = t.add_page(&mut root, 0);
                Some(pattern::PatternPanel::build(&mut skin, &mut root, &page)?)
            }
            None => None,
        };
        let inspector = inspector::Inspector::build(&mut skin, &mut root)?;
        let mut mixers = Vec::new();
        if let Some(t) = &editor_tabs {
            for (page_id, kind) in [(3, mixer::Kind::Layer), (4, mixer::Kind::Pad), (5, mixer::Kind::Global)] {
                let page = t.add_page(&mut root, page_id);
                mixers.push(mixer::Mixer::build(&mut skin, &mut root, &page, kind)?);
            }
        }
        let scenes = match &editor_tabs {
            Some(t) => {
                let page = t.add_page(&mut root, 1);
                Some(scenes::Scenes::build(&mut skin, &mut root, &page)?)
            }
            None => None,
        };
        let song = match &editor_tabs {
            Some(t) => {
                let page = t.add_page(&mut root, 2);
                Some(song::SongPanel::build(&mut skin, &mut root, &page)?)
            }
            None => None,
        };
        let midimap = match &editor_tabs {
            Some(t) => {
                let page = t.add_page(&mut root, 6);
                Some(midimap::MidiMap::build(&mut skin, &mut root, &page)?)
            }
            None => None,
        };
        let browser = match &utility_tabs {
            Some(t) => {
                let page = t.add_page(&mut root, 0);
                Some(browser::Browser::build(&mut skin, &mut root, &page)?)
            }
            None => None,
        };
        let tm_page = utility_tabs.as_ref().map(|t| t.add_page(&mut root, 1));
        let transmod = transmod::Transmod::build(&mut skin, &mut root, tm_page.as_deref())?;
        // MIDI learn overlay (drawn over everything, never hit).
        root.children.push(Ctrl {
            class: "CtrlPanel".into(),
            name: "LearnOverlay".into(),
            overload: "LearnOverlay".into(),
            style: "InvisibleStyle".into(),
            rect: dreist_gui::Rect::new(0, 0, root.rect.w, root.rect.h),
            zorder: 3000,
            visible: false,
            enabled: true,
            passive: true,
            ..Ctrl::default()
        });
        let slicer = slicer::Slicer::build(&mut skin, &mut root)?;
        let modals = modal::Modals::build(&mut skin, &mut root)?;
        let filefind = filefind::FileFind::build(&mut skin, &mut root)?;
        let devices = devices::Devices::build(&mut skin, &mut root)?;
        let midiconsole = midiconsole::MidiConsole::build(&mut skin, &mut root)?;
        let trigger = trigger::TriggerPanel::build(&mut skin, &mut root)?;
        let metadata = metadata::MetaData::build(&mut skin, &mut root)?;
        let convertor = convertor::Convertor::build(&mut skin, &mut root)?;
        let mut ed = Editor {
            menu_bar: false,
            closing: false,
            app: App::new(skin, root),
            dialogs: None,
            project_path: None,
            history: undo::History::default(),
            history_menu: None,
            knob_menu: None,
            engine_menu: None,
            engine_rename: None,
            bank_menu: None,
            knob_edit: None,
            file_menu: None,
            part_paths: Default::default(),
            drop_label: String::new(),
            external_drag: None,
            drop_hl: Default::default(),
            window_size: ((0, 0), (0, 0)),
            model,
            outbox: Vec::new(),
            pads,
            sampler,
            header,
            editor_tabs,
            utility_tabs,
            pattern,
            inspector,
            mixers,
            scenes,
            song,
            browser,
            transmod,
            midimap,
            slicer,
            modals,
            learn: learn::Learn::default(),
            popup_paths: Default::default(),
            padmenu: padmenu::PadMenu::default(),
            layer_menu: None,
            filefind,
            devices,
            midiconsole,
            trigger,
            metadata,
            retro: retro::PatternRetro::default(),
            convertor,
            patmenu: patmenu::PatMenu::default(),
            clipboard: padmenu::Clipboard::default(),
        };
        // `LastEditorScaleFactor` (Preferences › UI scaling), applied when
        // the window opens.
        ed.app.ui_scale = (ed.modals.options.int("LastEditorScaleFactor", 100).clamp(50, 300) as f32) / 100.0;
        // `Editor::open`: the window sizer; `ComputeStartupSize` [re:4aa577]:
        // `LastEditorSize` (window pixels, "0,0" = the default size).
        ed.app.set_has_window_sizer();
        let default = ed.app.size();
        let mut size = default;
        if !cfg!(test) {
            let o = ed.modals.options.clone();
            if let Some((w, h)) = parse_size(&o.string("LastEditorSize", "0,0")) {
                let ui = ed.app.ui_scale.max(0.25);
                size = ((w as f32 / ui).round() as i32, (h as f32 / ui).round() as i32);
                ed.app.resize(size.0, size.1);
            }
            restore_splits(&mut ed.app.root, &o);
        }
        let ui = ed.app.ui_scale;
        ed.window_size = (default, ((size.0 as f32 * ui).floor() as i32, (size.1 as f32 * ui).floor() as i32));
        // [re:f98edd] (`PostMessagingInitialized`).
        let startup = sampler::Sampler::startup_messages(&mut ed.model);
        // The document follows, as the engine's does (`mirror_message`).
        for c in &startup {
            ed.model.apply(c);
        }
        ed.outbox.extend(startup);
        // `e_registryOptionsChanged` [ForcedLayersFallThrough,
        // SendActiveSensing] ([re:0d12fb] $_0 posts it on
        // Confirm; the engine also reads it when built).
        let o = &ed.modals.options;
        let reg = command::msg(dreist_engine::binding::id::REGISTRY_OPTIONS_CHANGED, &[], &[o.bool("ForcedLayersFallThrough", false) as i32 as f32, o.bool("SendActiveSensing", false) as i32 as f32]);
        ed.outbox.push(reg);
        // MIDI out options (`e_registryMIDIOptionsChanged` from the options).
        if let Some(m) = &ed.midimap {
            ed.outbox.push(m.midi_options_message());
        }
        ed.sync();
        Ok(ed)
    }

    /// Turn MIDI learn mode on or off (header LearnButton).
    pub fn set_learn(&mut self, on: bool) {
        self.learn.active = on;
        self.learn.target = None;
        self.sync();
    }

    /// Show the About or Preferences panel.
    pub fn show_modal(&mut self, which: &str) {
        // A sample missing-files list (screenshots).
        if which.eq_ignore_ascii_case("filefind") {
            let items = (0..3).map(|i| filefind::Missing { path: std::path::PathBuf::from(format!("/Missing/Kick {i:02}.wav")), layers: vec![(0, i, 0)] }).collect();
            self.filefind.show(items);
            self.sync();
            return;
        }
        // The layer meta data editor beside layer row 1 (screenshots).
        if which.eq_ignore_ascii_case("metadata") {
            fn row(c: &Ctrl, path: &mut Vec<usize>) -> bool {
                for (i, k) in c.children.iter().enumerate() {
                    path.push(i);
                    if k.overload == "LayerRow" && k.visible || row(k, path) {
                        return true;
                    }
                    path.pop();
                }
                false
            }
            let mut p = Vec::new();
            if row(&self.app.root, &mut p) {
                self.metadata.show(&mut self.app, &self.model, &p);
            }
            self.sync();
            return;
        }
        if which.eq_ignore_ascii_case("audio") || which.eq_ignore_ascii_case("midi") {
            self.devices.show(if which.eq_ignore_ascii_case("audio") { devices::Panel::Audio } else { devices::Panel::Midi });
            self.sync();
            return;
        }
        let k = if which.eq_ignore_ascii_case("about") {
            modal::Kind::About
        } else if which.eq_ignore_ascii_case("export") {
            modal::Kind::Export
        } else {
            modal::Kind::Preferences
        };
        self.modals.show(&mut self.app, k);
        self.sync();
    }

    /// `Editor::open` $_3 (root rect changed): `LastEditorSize` = the size
    /// × scale, floored ("%d,%d"); written once a live resize ends.
    fn save_window_size(&mut self) {
        let (w, h) = self.app.size();
        let ui = self.app.ui_scale;
        let px = ((w as f32 * ui).floor() as i32, (h as f32 * ui).floor() as i32);
        if px == self.window_size.1 || self.app.live_resizing() {
            return;
        }
        self.window_size.1 = px;
        let v = format!("{},{}", px.0, px.1);
        self.modals.options.set("LastEditorSize", &v);
        if !cfg!(test) {
            let mut o = options::Options::load();
            o.set("LastEditorSize", v);
            o.save();
        }
    }

    #[cfg(test)]
    fn pads_matrix(&self) -> Option<Vec<usize>> {
        Some(self.pads.matrix().to_vec())
    }

    /// Per-frame animations (scene activity labels).
    pub fn animate(&mut self) {
        for ev in self.app.animate() {
            self.handle(&ev);
        }
        if self.transmod.animate_flashes(&mut self.app) {
            self.app.dirty = true;
        }
        if self.pads.animate(&mut self.app, &self.model) {
            self.app.dirty = true;
        }
        self.animate_drop_highlights();
        self.save_window_size();
        if self.filefind.scanning() && self.filefind.tick(&mut self.app, &mut self.model) {
            self.outbox.push(self.model.rebuild_command());
            self.sync();
        }
        if self.modals.tick(&mut self.app) {
            self.app.dirty = true;
        }
        if self.scenes.as_mut().is_some_and(|sc| sc.animate()) {
            if let Some(sc) = &mut self.scenes {
                sc.sync(&mut self.app, &self.model);
            }
            self.app.dirty = true;
        }
    }

    /// Open the slicer on an audio file ([re:b73f33]).
    pub fn open_slicer(&mut self, file: &std::path::Path) -> bool {
        let ok = self.slicer.start(&self.model, file);
        self.sync();
        ok
    }

    /// Replace the document (project loaded or reset).
    pub fn set_document(&mut self, doc: dreist_formats::Node) {
        self.retro.clear();
        let keep = (self.model.bank, self.model.pad, self.model.name.take(), self.model.samples.take(), self.model.editor_tab, self.model.utility_tab);
        self.model = Model::new(doc);
        (self.model.bank, self.model.pad, self.model.name, self.model.samples, self.model.editor_tab, self.model.utility_tab) = keep;
        self.sync();
    }
}

#[cfg(feature = "window")]
pub mod window;

pub mod screens;
#[cfg(test)]
mod screens_test;

#[cfg(test)]
mod tests;
