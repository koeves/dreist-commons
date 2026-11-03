//! The editor in a baseview window.
use dreist_gui::app::{App, UiEvent};
use dreist_gui::window::WindowApp;

use crate::{Command, Editor};

/// Called each frame with the commands produced; may also refresh the
/// model (e.g. from the audio thread's snapshot).
pub type Link = Box<dyn FnMut(&mut Editor, Vec<Command>) + Send>;

pub struct Hosted(pub Editor, pub Link);

impl WindowApp for Hosted {
    fn app(&mut self) -> &mut App {
        &mut self.0.app
    }
    fn event(&mut self, ev: &UiEvent) {
        self.0.handle(ev);
    }
    fn frame(&mut self) {
        let cmds = self.0.take_commands();
        (self.1)(&mut self.0, cmds);
    }
    fn captures_keys(&self) -> bool {
        self.0.menu_bar
    }
    fn render(&mut self, scale: f32) -> &dreist_gui::tiny_skia::Pixmap {
        self.0.render_frame(scale)
    }
}

/// Closing: the link's last call, with what the editor still has queued.
impl Drop for Hosted {
    fn drop(&mut self) {
        self.0.closing = true;
        let cmds = self.0.take_commands();
        (self.1)(&mut self.0, cmds);
    }
}

/// Native open/save dialogs (rfd).
pub fn native_dialogs() -> crate::DialogFn {
    Box::new(|d| match d {
        // [re:9d4d1f]: the open dialogs also
        // take the matching Guru files.
        crate::Dialog::OpenProject => rfd::FileDialog::new()
            .add_filter(format!("{} project", crate::product()), &dreist_formats::profile::open_extensions())
            .add_filter("Guru preset files", &["GRU", "gru"])
            .set_directory(crate::browser::content_dir())
            .pick_file(),
        crate::Dialog::SaveProject => {
            let ext = dreist_formats::profile::get().save.project;
            rfd::FileDialog::new().add_filter(format!("{} project", crate::product()), &[ext]).set_file_name(format!("Untitled.{ext}")).save_file()
        }
        crate::Dialog::ExportAudio => rfd::FileDialog::new().add_filter("WAV audio", &["wav"]).set_file_name("Export.wav").save_file(),
        crate::Dialog::SaveSample(name) => rfd::FileDialog::new()
            .set_title("Please select the location to save to")
            .add_filter("Sample files", &["wav"])
            .set_file_name(name)
            .set_directory(crate::modal::default_documents())
            .save_file(),
        crate::Dialog::SaveChainPreset => {
            let mut d = rfd::FileDialog::new().set_title("Please select the location to save to").add_filter("FX chain preset files", &["fxchain"]).set_file_name("New Preset.fxchain");
            if let Some(u) = Some(crate::options::documents_dir().join("Effect Chains")) {
                let _ = std::fs::create_dir_all(&u);
                d = d.set_directory(u);
            }
            d.save_file()
        }
        crate::Dialog::SaveEffectPreset(kind) => {
            let mut d = rfd::FileDialog::new().set_title("Please select the location to save to").add_filter("Effect preset files", &["fxpreset"]).set_file_name("New Preset.fxpreset");
            if let Some(u) = Some(crate::options::documents_dir().join("Effects").join(dreist_formats::params::EFFECT_TYPES.get(kind).copied().unwrap_or(""))) {
                let _ = std::fs::create_dir_all(&u);
                d = d.set_directory(u);
            }
            d.save_file()
        }
        crate::Dialog::OpenPart(part) => {
            let (kind, desc, noun) = part.doc_kind();
            let mut d = rfd::FileDialog::new().set_title(format!("Please choose the {noun} file to load")).add_filter(desc, &dreist_formats::profile::extensions_of(kind));
            d = match part {
                crate::project::Part::Kit(_) => d.add_filter("Guru kit files", &["KIT", "kit"]),
                crate::project::Part::Pattern(..) => d.add_filter("Guru pattern files", &["G01", "g01"]),
                crate::project::Part::Bank(_) => d.add_filter("Guru pattern bank files", &["G24", "g24"]),
                _ => d,
            };
            d.pick_file()
        }
        crate::Dialog::SavePart(part) => {
            let (ext, desc, _) = part.file_type();
            rfd::FileDialog::new().set_title("Please select the location to save to").add_filter(desc, &[ext]).set_file_name(format!("Untitled.{ext}")).save_file()
        }
        crate::Dialog::OpenGuruSwing => rfd::FileDialog::new().set_title("Please choose the file(s) you wish to import").add_filter("GURU swing template files", &["txt"]).pick_file(),
        crate::Dialog::OpenMidiTemplate => rfd::FileDialog::new().set_title("Please choose the file(s) you wish to import").add_filter("MIDI files", &["mid"]).pick_file(),
        crate::Dialog::OpenMidi => rfd::FileDialog::new().set_title("Please choose the MIDI file to load").add_filter("Pattern MIDI files", &["mid", "midi"]).pick_file(),
        crate::Dialog::SaveMidi => rfd::FileDialog::new().set_title("Please select the location to save to").add_filter("Pattern MIDI files", &["mid"]).set_file_name("Pattern.mid").save_file(),
        crate::Dialog::ChooseFolder => rfd::FileDialog::new().set_title("Please choose a search location").pick_folder(),
        crate::Dialog::ExportFolder => {
            let desktop = dreist_formats::paths::home().join("Desktop");
            rfd::FileDialog::new().set_title("Please choose a location to export to").set_directory(desktop).pick_folder()
        }
        crate::Dialog::ChooseDocuments(start) => rfd::FileDialog::new().set_title(crate::modal::DOCS_BROWSE_TITLE).set_directory(start).pick_folder(),
        crate::Dialog::LocateFile => rfd::FileDialog::new().add_filter("Audio", &["wav", "aif", "aiff"]).pick_file(),
        crate::Dialog::SaveSwingTemplate => {
            let mut d = rfd::FileDialog::new().set_title("Please select the location to save to").add_filter("Swing template files", &["swingtemplate"]).set_file_name("Groove.swingtemplate");
            if let Some(u) = crate::swing::user_dir() {
                let _ = std::fs::create_dir_all(&u);
                d = d.set_directory(u);
            }
            d.save_file()
        }
    })
}

pub fn open_blocking(mut ed: Editor, mut on_commands: impl FnMut(Vec<Command>) + Send + 'static) {
    if ed.dialogs.is_none() {
        ed.dialogs = Some(native_dialogs());
    }
    dreist_gui::window::open_blocking(
        Hosted(
            ed,
            Box::new(move |_, c| {
                if !c.is_empty() {
                    on_commands(c)
                }
            }),
        ),
        crate::product(),
    );
}

pub fn open_parented<P: dreist_gui::window::RawHandle>(parent: &P, mut ed: Editor, link: Link) -> dreist_gui::window::WindowHandle {
    if ed.dialogs.is_none() {
        ed.dialogs = Some(native_dialogs());
    }
    dreist_gui::window::open_parented(parent, Hosted(ed, link))
}
