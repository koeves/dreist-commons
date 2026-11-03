//! MIDI console (Help › "Show MIDI Console", `MIDIDisplayPanel.skin`):
//! incoming MIDI as text in `OutputView`, filtered by the Notes / CCs /
//! Other toggles, Clear empties it. Lines come from
//! [re:e53b37] with
//! `IntToKeyName` [re:59f16c] note names.

use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::render::Renderer;
use dreist_gui::style::{HAlign, VAlign};
use dreist_gui::{Colour, Ctrl, Skin};

use crate::EditorError;

const ROW: f32 = 15.0;
const MAX_LINES: usize = 1000;

/// `IntToKeyName`: "C#3" style; `mode` 0 → C0 at 0, 1 → C-2, 2 → C-1;
/// `numbers` gives the note number instead.
pub fn key_name(n: u8, mode: u8, numbers: bool) -> String {
    if n > 127 {
        return "--".into();
    }
    if numbers {
        return n.to_string();
    }
    const L: &[u8; 24] = b"CCDDEFFGGAAB # #  # # # ";
    let k = (n % 12) as usize;
    let (a, b) = (L[k] as char, L[k + 12] as char);
    let oct = (n / 12) as i32
        - match mode {
            1 => 2,
            2 => 1,
            _ => 0,
        };
    format!("{a}{b}{oct}")
}

/// `ConvertMIDIMsgBytesToString`.
pub fn describe(m: [u8; 3], mode: u8) -> String {
    let ch = (m[0] & 0x0f) + 1;
    match m[0] >> 4 {
        0x8 => format!("Note off {:03} ({}) Velocity {:03} Channel {:02}", m[1], key_name(m[1], mode, false), m[2], ch),
        0x9 if m[2] == 0 => format!("Note off {:03} ({}) Velocity {:03} Channel {:02}", m[1], key_name(m[1], mode, false), m[2], ch),
        0x9 => format!("Note on  {:03} ({}) Velocity {:03} Channel {:02}", m[1], key_name(m[1], mode, false), m[2], ch),
        0xa => format!("A-touch  {:03} Channel {:02}", m[1], ch),
        0xb => format!("C-Change {:03} Value {:03} Channel {:02}", m[1], m[2], ch),
        0xc => format!("Prog Change {:03} Channel {:02}", m[1], ch),
        0xd => format!("C-Pres   {:03} Channel {:02}", m[1], ch),
        0xe => {
            let v = ((m[1] & 0x7f) as u32 | ((m[2] & 0x7f) as u32) << 7) as f32 / 16383.0;
            format!("Pitch Bend {v:.2} Channel {ch:02}")
        }
        _ => match m[0] {
            0xf1 => "MTC quarterFrame".into(),
            0xf2 => format!("Song Position {:03}{:03}", m[1], m[2]),
            0xf3 => format!("Song Select {:03}", m[1]),
            0xf6 => "Tune Request".into(),
            0xfa => "Song start".into(),
            0xfb => "Song continue".into(),
            _ => "Unknown MIDI msg".into(),
        },
    }
}

pub struct MidiConsole {
    overlay: Option<Vec<usize>>,
    pub open: bool,
    pub lines: Vec<String>,
    notes: bool,
    ccs: bool,
    other: bool,
    scroll_end: bool,
}

impl MidiConsole {
    pub fn build(skin: &mut Skin, root: &mut Ctrl) -> Result<MidiConsole, EditorError> {
        let overlay = crate::modal::overlay(skin, root, "MIDIDisplayPanel.skin", "MIDIConsoleOverlay")?;
        if let Some(op) = &overlay {
            let mut o = &mut *root;
            for &i in op {
                o = &mut o.children[i];
            }
            if let Some(v) = o.find_mut("OutputView") {
                v.overload = "MIDIConsoleView".into();
                v.children.clear();
            }
        }
        Ok(MidiConsole { overlay, open: false, lines: Vec::new(), notes: true, ccs: true, other: true, scroll_end: true })
    }

    /// New MIDI from the engine.
    pub fn push(&mut self, msgs: &[[u8; 3]], mode: u8) -> bool {
        if !self.open || msgs.is_empty() {
            return false;
        }
        for &m in msgs {
            let keep = match m[0] >> 4 {
                0x8 | 0x9 => self.notes,
                0xb => self.ccs,
                _ => self.other,
            };
            if keep {
                self.lines.push(describe(m, mode));
            }
        }
        if self.lines.len() > MAX_LINES {
            let cut = self.lines.len() - MAX_LINES;
            self.lines.drain(..cut);
        }
        true
    }

    pub fn sync(&self, app: &mut App) {
        let (open, n, c, o) = (self.open, self.notes, self.ccs, self.other);
        if let Some(ov) = self.overlay.clone().and_then(|p| app.ctrl_mut(&p)) {
            ov.visible = open;
            for (name, on) in [("Notes", n), ("CCs", c), ("Other", o)] {
                if let Some(b) = ov.find_mut(name) {
                    b.on = on;
                }
            }
        }
    }

    fn mine(&self, path: &[usize]) -> bool {
        self.open && self.overlay.as_ref().is_some_and(|p| path.starts_with(p))
    }

    /// Returns whether the event was the console's.
    pub fn handle(&mut self, app: &App, ev: &UiEvent) -> bool {
        if !self.open {
            return false;
        }
        match ev {
            UiEvent::Clicked { path, name, on, .. } if self.mine(path) => {
                match name.as_str() {
                    "Clear" => self.lines.clear(),
                    "Notes" => self.notes = *on,
                    "CCs" => self.ccs = *on,
                    "Other" => self.other = *on,
                    _ => {}
                }
                true
            }
            // A click on the dimmed area closes the console.
            UiEvent::Mouse { path, overload, kind: MouseKind::Down, .. } if overload == "ModalOverlay" && self.overlay.as_deref() == Some(path.as_slice()) => {
                self.open = false;
                true
            }
            UiEvent::Wheel { path, .. } if self.mine(path) => {
                self.scroll_end = false;
                true
            }
            UiEvent::Mouse { path, .. } | UiEvent::Clicked { path, .. } | UiEvent::ValueChanged { path, .. } => {
                let _ = app;
                self.mine(path)
            }
            _ => false,
        }
    }

    pub fn draw(&self, r: &Renderer, p: &mut Painter, c: &Ctrl) -> bool {
        if c.overload != "MIDIConsoleView" {
            return false;
        }
        let s = r.styles;
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let font = s.font("ListViewSyle");
        let text = s.colour("ListViewSyle", "colText", Colour::WHITE);
        p.fill_rect(RectF::new(0.0, 0.0, w, h), s.colour("ListViewSyle", "colFill", Colour::rgba(20, 20, 20, 255)));
        let rows = (h / ROW).floor() as usize;
        let first = self.lines.len().saturating_sub(rows);
        for (k, l) in self.lines.iter().skip(first).enumerate() {
            p.text(RectF::new(4.0, k as f32 * ROW, w - 8.0, ROW), l, text, &font, HAlign::Left, VAlign::Center);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_read_like_the_original() {
        assert_eq!(key_name(60, 1, false), "C 3");
        assert_eq!(key_name(61, 0, false), "C#5");
        assert_eq!(key_name(0, 1, false), "C -2");
        assert_eq!(describe([0x90, 60, 100], 1), "Note on  060 (C 3) Velocity 100 Channel 01");
        assert_eq!(describe([0x91, 60, 0], 1), "Note off 060 (C 3) Velocity 000 Channel 02");
        assert_eq!(describe([0xb0, 7, 127], 1), "C-Change 007 Value 127 Channel 01");
        assert_eq!(describe([0xe0, 0, 64], 1), "Pitch Bend 0.50 Channel 01");
    }
}
