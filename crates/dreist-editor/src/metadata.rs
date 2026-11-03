//! Layer "Edit meta data" popup ([re:823bc8],
//! `Pad Editor/MetaDataAnchor.skin`; opened by [re:ecfa09]'s
//! item menu [re:4c4adf]).
//!
//! - A working copy of the active analysis' [re:ed1f12] (User,
//!   else Master; defaults 120 BPM / 1 bar / 0 beats / 440 Hz, a file
//!   without a `.gsa` gets a guessed key). Every box parses with `atof`,
//!   clamps (BPM 20..1000, Bars / Beats ≥ 0, Root 1..20000 Hz, Author /
//!   Description ≤ 63 characters) and [re:6ef6bb] rewrites all
//!   boxes (`%0.2f`). BPM / Bars / Beats do not interlock here.
//! - Root (Hz) and RootNote edit the same pitch: a note n stores
//!   440·2^((n − 69)/12) ([re:abcb3d]); the popup shows
//!   round(12·log2(Hz/440)) + 69.
//! - Guess: key guessing [re:13651e] on the whole file. Save:
//!   the working copy into the active analysis, and saving it
//!   rewrites the `.gsa` ("Metadata failed to save - is the directory
//!   read only?"); the engine reads the new tempo (port: a rebuild). Save
//!   and Cancel close after 0.3 s (port: at once), a click outside closes
//!   without saving.
//! - Placement: right of the layer button, centred on it, raised a pixel
//!   at a time until it fits (≤ 99 steps).

use std::path::PathBuf;

use dreist_engine::gsa::SliceFile;
use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::{Ctrl, Rect, Skin};

use crate::EditorError;
use crate::command::Command;
use crate::model::Model;
use crate::search::atof;

pub struct MetaData {
    overlay: Option<Vec<usize>>,
    /// The file being edited and its working copy.
    open: Option<(PathBuf, u32, f32, SliceFile)>,
}

fn note_of(pitch: f32) -> usize {
    let x = 12.0 * (pitch * 0.002_272_727_2).log2();
    ((x + if x < 0.0 { -0.5 } else { 0.5 }) as i32 + 69).clamp(0, 126) as usize
}

/// [re:8ad0c7] in the global numbering mode.
fn key(n: u8) -> String {
    let mode = crate::options::Options::load().int("MIDINumbering", 1).clamp(0, 2) as u8;
    crate::midiconsole::key_name(n, mode, false)
}

impl MetaData {
    pub fn build(skin: &mut Skin, root: &mut Ctrl) -> Result<MetaData, EditorError> {
        let overlay = crate::modal::overlay(skin, root, "MetaDataAnchor.skin", "MetaDataOverlay")?;
        Ok(MetaData { overlay, open: None })
    }

    /// Open on the selected layer's sample beside the control at `anchor`.
    pub fn show(&mut self, app: &mut App, model: &Model, anchor: &[usize]) -> bool {
        let e = model.engine;
        let Some(file) = model.layer_sample(e, model.pad[e], model.layer[e]) else { return false };
        let Some((frames, sr)) = dreist_engine::audio::info(&file) else { return false };
        let gsa = file.with_extension("gsa");
        let a = SliceFile::load(&gsa).unwrap_or_else(|| {
            let mut a = SliceFile::new(frames as u32);
            (a.bpm, a.bars, a.beats, _) = dreist_engine::engine::guess_key_data(frames as u32, sr, Some(&file));
            a
        });
        self.open = Some((file, frames as u32, sr, a));
        let (ax, ay) = app.origin(anchor);
        let (aw, ah) = app.ctrl(anchor).map(|c| (c.rect.w, c.rect.h)).unwrap_or((0, 0));
        let rh = app.root.rect.h;
        if let Some(panel) = self.overlay.clone().and_then(|p| app.ctrl_mut(&p)).and_then(|o| o.children.first_mut()) {
            let (pw, ph) = (panel.rect.w, panel.rect.h);
            let mut y = ah - ah / 2 - ph / 2;
            let mut iters = 0;
            while ay + y + ph > rh && iters < 99 {
                iters += 1;
                if ay + y <= 0 {
                    y = 0;
                    break;
                }
                y -= 1;
            }
            if ay + y < 0 {
                y += 5 - (ay + y);
            }
            panel.set_rect(Rect::new(ax + aw, ay + y, pw, ph));
        }
        self.sync(app);
        true
    }

    /// Display update [re:6ef6bb].
    pub fn sync(&self, app: &mut App) {
        let open = self.open.is_some();
        let Some(o) = self.overlay.clone().and_then(|p| app.ctrl_mut(&p)) else { return };
        o.visible = open;
        let Some((_, _, _, a)) = &self.open else { return };
        let set = |o: &mut Ctrl, n: &str, t: String| {
            if let Some(c) = o.find_mut(n) {
                c.text = t;
            }
        };
        set(o, "BPMBox", format!("{:.2}", a.bpm));
        set(o, "BarsBox", format!("{:.2}", a.bars));
        set(o, "BeatsBox", format!("{:.2}", a.beats));
        set(o, "Root", format!("{:.2}", a.pitch));
        set(o, "RootNote", key(note_of(a.pitch) as u8));
        set(o, "Author", a.author.clone());
        set(o, "Description", a.desc.clone());
    }

    fn mine(&self, path: &[usize]) -> bool {
        self.open.is_some() && self.overlay.as_ref().is_some_and(|p| path.starts_with(p))
    }

    fn close(&mut self, app: &mut App) {
        self.open = None;
        self.sync(app);
    }

    /// Returns whether the event was the popup's.
    pub fn handle(&mut self, app: &mut App, model: &Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        if self.open.is_none() {
            return false;
        }
        match ev {
            UiEvent::TextCommitted { path, name, text } if self.mine(path) => {
                let Some((_, _, _, a)) = self.open.as_mut() else { return false };
                let v = atof(text);
                match name.as_str() {
                    "BPMBox" => a.bpm = v.min(1000.0).max(20.0),
                    "BarsBox" => a.bars = v.max(0.0),
                    "BeatsBox" => a.beats = v.max(0.0),
                    "Root" => a.pitch = v.min(20000.0).max(1.0),
                    "Author" => a.author = text.chars().take(63).collect(),
                    "Description" => a.desc = text.chars().take(63).collect(),
                    _ => {}
                }
                self.sync(app);
                true
            }
            UiEvent::MenuSelected { anchor, name, index } if self.mine(anchor) && name == "RootNote" => {
                if let Some((_, _, _, a)) = self.open.as_mut() {
                    a.pitch = (440.0 * ((*index as f64 - 69.0) / 12.0).exp2()) as f32;
                }
                self.sync(app);
                true
            }
            UiEvent::Clicked { path, name, .. } if self.mine(path) => {
                match name.as_str() {
                    "RootNote" => {
                        let cur = self.open.as_ref().map(|o| note_of(o.3.pitch));
                        app.open_menu(path, (0..127).map(key).collect(), cur);
                    }
                    "GuessButton" => {
                        if let Some((file, frames, sr, a)) = self.open.as_mut() {
                            let fallback;
                            (a.bpm, a.bars, a.beats, fallback) = dreist_engine::engine::guess_key_data(*frames, *sr, Some(file));
                            if fallback {
                                a.pitch = 440.0;
                            }
                        }
                        self.sync(app);
                    }
                    "CancelButton" => self.close(app),
                    "SaveButton" => {
                        if let Some((file, _, _, a)) = &self.open {
                            if a.save(&file.with_extension("gsa")).is_err() {
                                // [re:af34e2] (port: a message menu).
                                app.open_menu(path, vec![dreist_gui::menu::item("Metadata failed to save - is the directory read only?", false)], None);
                            }
                            out.push(model.rebuild_command());
                        }
                        self.close(app);
                    }
                    _ => {}
                }
                true
            }
            // [re:c02ce8]: a click outside closes without saving.
            UiEvent::Mouse { path, overload, kind: MouseKind::Down | MouseKind::RightDown, .. } if overload == "ModalOverlay" && self.overlay.as_deref() == Some(path.as_slice()) => {
                self.close(app);
                true
            }
            UiEvent::Mouse { path, .. } | UiEvent::Wheel { path, .. } | UiEvent::Pressed { path, .. } | UiEvent::ValueChanged { path, .. } | UiEvent::RightClicked { path, .. } => self.mine(path),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_note_and_atof() {
        assert_eq!(note_of(440.0), 69);
        assert_eq!(note_of(261.63), 60);
        assert_eq!(note_of(1.0), 0);
        assert_eq!(atof(" 128.5bpm"), 128.5);
        assert_eq!(atof("x"), 0.0);
    }
}
