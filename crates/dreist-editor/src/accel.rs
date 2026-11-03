//! Keyboard shortcuts ([re:a191fd], ev_16eeb3a7…, ev_ce262c09…,
//! ev_68554242…).
//!
//! [re:3b32b8] hands each key to
//! [re:9454a7], which looks the key up in
//! the accelerator table (the menus' key equivalents) and
//! runs the entry's function [re:df170c]:
//! - nothing happens unless option `RespondToKeyboard` (default on) is set;
//! - an entry matches when the key matches and either both sides have no
//!   modifiers or the modifier sets are equal;
//! - entries without modifiers only respond with `RespondToExtendedKeyboard`
//!   (default off), except the browser arrows 0x24–0x27.
//!
//! The standalone app also carries the table as its menu bar's key
//! equivalents ([re:c60fcb]; [re:80cba3] runs
//! the function regardless of the options), so there every entry works.

/// Modifier set of an entry (shift, command, option, control).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub cmd: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl Mods {
    pub fn any(&self) -> bool {
        self.shift || self.cmd || self.alt || self.ctrl
    }
}

/// What an entry's function does (54 functions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Editor message `id`.
    Editor(u32),
    /// Engine message `id` with the flag flipped, for a transport toggle
    /// (the header button with that message).
    Toggle(&'static str),
    /// Engine message `id` for a transport trigger (the header button).
    Press(&'static str),
    /// Loop: an empty function.
    Nothing,
}

pub struct Accel {
    pub id: u32,
    /// Key as `App::shortcut` names it (a lowercase character, " ",
    /// "left" / "right" / "up" / "down", "enter").
    pub key: &'static str,
    pub mods: Mods,
    /// Menu item title (`Construct*Menu`).
    pub name: &'static str,
    pub action: Action,
}

const N: Mods = Mods { shift: false, cmd: false, alt: false, ctrl: false };
const C: Mods = Mods { shift: false, cmd: true, alt: false, ctrl: false };
const SC: Mods = Mods { shift: true, cmd: true, alt: false, ctrl: false };
const CA: Mods = Mods { shift: false, cmd: true, alt: true, ctrl: false };
const SCA: Mods = Mods { shift: true, cmd: true, alt: true, ctrl: false };
const S: Mods = Mods { shift: true, cmd: false, alt: false, ctrl: false };
const AK: Mods = Mods { shift: false, cmd: false, alt: true, ctrl: true };

const fn a(id: u32, key: &'static str, mods: Mods, name: &'static str, action: Action) -> Accel {
    Accel { id, key, mods, name, action }
}

/// The entries with a key (menu items without one are left out). Editor
/// messages: e_updateOpenPrefs 0x16a, e_updateOpenPreset / Engine / Kit
/// 0x16c–0x16e, e_updateSavePreset 0x174, e_updateSavePresetAs 0x176,
/// e_updateSaveKit / As 0x178 / 0x179, e_updateUndo / Redo 0x18d / 0x18e,
/// e_updateDelete 0x18f, e_updateBrowserStartFilter / StartFind 0x160 /
/// 0x161, e_grabRetroRecording 0x23, browser 0x155–0x15b, panels
/// 0x146–0x14e, e_resetUISizeToDefault / Scale 0x152 / 0x153. Synth:
/// e_togglePlayback 0x15 (![re:64b7fd]), e_toggleRecord 0x14,
/// e_toggleMetronome 0x18, e_toggleQuantize 0x19,
/// e_songSetActive 0x117 (!active, 0), e_returnToZero 0x1d, e_panic 0x12.
pub const TABLE: &[Accel] = &[
    a(0x00, ",", C, "Preferences", Action::Editor(0x16a)),
    a(0x01, "o", C, "Open Preset", Action::Editor(0x16c)),
    a(0x02, "o", CA, "Open Engine", Action::Editor(0x16d)),
    a(0x03, "o", SC, "Open Kit", Action::Editor(0x16e)),
    a(0x09, "s", C, "Save Preset", Action::Editor(0x174)),
    a(0x0a, "s", SC, "Save Preset As...", Action::Editor(0x176)),
    a(0x0d, "s", CA, "Save Kit", Action::Editor(0x178)),
    a(0x0e, "s", SCA, "Save Kit As...", Action::Editor(0x179)),
    a(0x16, "z", C, "Undo", Action::Editor(0x18d)),
    a(0x17, "z", SC, "Redo", Action::Editor(0x18e)),
    a(0x18, "delete", N, "Delete Selected Steps", Action::Editor(0x18f)),
    a(0x19, "f", SC, "Filter Browser List", Action::Editor(0x160)),
    a(0x1a, "f", C, "Find", Action::Editor(0x161)),
    a(0x1b, " ", N, "Play", Action::Toggle("PlayButton")),
    a(0x1c, "r", N, "Record", Action::Toggle("RecButton")),
    a(0x1d, "r", CA, "Retro Record", Action::Editor(0x23)),
    a(0x1e, "m", N, "Metronome", Action::Toggle("MetroButton")),
    a(0x1f, "k", N, "Input Quantize", Action::Toggle("QuantizeButton")),
    a(0x20, "s", N, "Song mode", Action::Toggle("SongButton")),
    a(0x21, "l", N, "Loop", Action::Nothing),
    a(0x22, "0", N, "Return to zero", Action::Press("RestartButton")),
    a(0x23, "p", SC, "Panic", Action::Press("PanicButton")),
    a(0x24, "down", N, "Next File", Action::Editor(0x155)),
    a(0x25, "up", N, "Previous File", Action::Editor(0x156)),
    a(0x26, "right", N, "Next Location", Action::Editor(0x157)),
    a(0x27, "left", N, "Previous Location", Action::Editor(0x158)),
    a(0x28, "enter", N, "Enter Folder", Action::Editor(0x15a)),
    a(0x29, "left", S, "Parent Folder", Action::Editor(0x15b)),
    a(0x2a, "r", SC, "Refresh", Action::Editor(0x159)),
    a(0x33, "1", N, "Browser", Action::Editor(0x14e)),
    a(0x32, "2", N, "Transmod", Action::Editor(0x14d)),
    a(0x2b, "3", N, "Pattern", Action::Editor(0x146)),
    a(0x30, "4", N, "Layer Mixer", Action::Editor(0x14b)),
    a(0x2f, "5", N, "Pad Mixer", Action::Editor(0x14a)),
    a(0x2e, "6", N, "Mixer", Action::Editor(0x149)),
    a(0x2c, "7", N, "Scenes", Action::Editor(0x147)),
    a(0x2d, "8", N, "Song", Action::Editor(0x148)),
    a(0x31, "9", N, "Mapping", Action::Editor(0x14c)),
    a(0x34, "1", AK, "Reset UI size to default", Action::Editor(0x152)),
    a(0x35, "2", AK, "Reset UI scale to 100%", Action::Editor(0x153)),
];

/// Menu items without a key (File menu): id, title, editor
/// message (functions 4–8, 11, 12, 15–20).
pub const UNKEYED: &[(u32, &str, u32)] = &[
    (0x04, "Open Pad", 0x16f),
    (0x05, "Open PatternBank", 0x170),
    (0x06, "Open Pattern", 0x171),
    (0x07, "Open Pattern As MIDI", 0x172),
    (0x08, "Open Mapping", 0x173),
    (0x0b, "Save Engine", 0x175),
    (0x0c, "Save Engine As...", 0x177),
    (0x0f, "Save Pad", 0x17a),
    (0x10, "Save PatternBank", 0x17b),
    (0x11, "Save Pattern", 0x17c),
    (0x12, "Save Pattern As MIDI", 0x17d),
    (0x13, "Save Mapping", 0x17e),
    (0x14, "Export As Audio", 0x17f),
];

/// A menu bar entry.
pub enum Menu {
    Item(u32),
    Separator,
    Sub(&'static str, &'static [Menu]),
}

use Menu::{Item as I, Separator as Sep, Sub};

/// The standalone's menus (File, Edit, Browser, Transport, View;
/// Preferences sits in the app menu).
pub const MENUS: &[(&str, &[Menu])] = &[
    (
        "File",
        &[
            I(0x01),
            I(0x02),
            I(0x03),
            Sub("Open Others...", &[I(0x04), I(0x05), I(0x06), I(0x07), I(0x08)]),
            Sep,
            I(0x09),
            I(0x0a),
            I(0x0b),
            I(0x0c),
            I(0x0d),
            I(0x0e),
            Sub("Save Others...", &[I(0x0f), I(0x10), I(0x11), I(0x12), I(0x13)]),
            Sep,
            I(0x14),
        ],
    ),
    ("Edit", &[I(0x16), I(0x17), Sep, I(0x18), Sep, I(0x1a), I(0x19)]),
    ("Browser", &[I(0x24), I(0x25), I(0x26), I(0x27), I(0x28), I(0x29), I(0x2a)]),
    ("Transport", &[I(0x1b), I(0x1c), I(0x1d), I(0x1e), I(0x1f), I(0x20), I(0x22), I(0x21), I(0x23)]),
    ("View", &[I(0x33), I(0x32), I(0x2b), I(0x30), I(0x2f), I(0x2e), I(0x2c), I(0x2d), I(0x31), Sep, I(0x34), I(0x35)]),
];

/// An entry's title and function (keyed or not).
pub fn entry(id: u32) -> Option<(&'static str, Action)> {
    TABLE.iter().find(|e| e.id == id).map(|e| (e.name, e.action)).or_else(|| UNKEYED.iter().find(|u| u.0 == id).map(|u| (u.1, Action::Editor(u.2))))
}

/// The entry for `key` with `mods`, if it responds.
/// `menu_bar`: the standalone's menu key equivalents (no option gating).
pub fn lookup(key: &str, mods: Mods, keyboard: bool, extended: bool, menu_bar: bool) -> Option<&'static Accel> {
    let e = TABLE.iter().find(|e| e.key == key && e.mods == mods)?;
    if menu_bar {
        return Some(e);
    }
    if !keyboard {
        return None;
    }
    (e.mods.any() || extended || (0x24..=0x27).contains(&e.id)).then_some(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gating() {
        // Space plays from the menu bar; in a plugin only with extended keys.
        assert_eq!(lookup(" ", N, true, false, true).map(|e| e.id), Some(0x1b));
        assert!(lookup(" ", N, true, false, false).is_none());
        assert_eq!(lookup(" ", N, true, true, false).map(|e| e.id), Some(0x1b));
        // Modified keys need only RespondToKeyboard; the browser arrows
        // always respond.
        assert_eq!(lookup("z", SC, true, false, false).map(|e| e.id), Some(0x17));
        assert!(lookup("z", SC, false, false, false).is_none());
        assert_eq!(lookup("down", N, true, false, false).map(|e| e.id), Some(0x24));
        assert!(lookup("enter", N, true, false, false).is_none());
        // Modifiers must match exactly.
        assert!(lookup("z", CA, true, true, true).is_none());
        assert_eq!(lookup("left", S, true, false, true).map(|e| e.id), Some(0x29));
    }
}
