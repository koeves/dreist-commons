//! Keys a plugin host hands the editor itself instead of its window (VST3
//! `IPlugView::onKeyDown`): Live on Windows types into a plugin's text boxes
//! only this way. The host calls in on its UI thread; the window takes the
//! keys on its next frame.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// One key from the host, in VST3 terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostKey {
    pub down: bool,
    /// The UTF-16 character, or 0 for a virtual key.
    pub character: u16,
    /// The VST3 virtual key code (`KEY_*`, see [`vk`]), or 0.
    pub key_code: i16,
    /// The VST3 modifier bits (see [`modifier`]).
    pub modifiers: i16,
}

/// VST3 virtual key codes (`VirtualKeyCodes`).
pub mod vk {
    pub const BACK: i16 = 1;
    pub const TAB: i16 = 2;
    pub const RETURN: i16 = 4;
    pub const ESCAPE: i16 = 6;
    pub const SPACE: i16 = 7;
    pub const END: i16 = 9;
    pub const HOME: i16 = 10;
    pub const LEFT: i16 = 11;
    pub const UP: i16 = 12;
    pub const RIGHT: i16 = 13;
    pub const DOWN: i16 = 14;
    pub const ENTER: i16 = 19;
    pub const DELETE: i16 = 22;
}

/// VST3 key modifier bits (`KeyModifier`).
pub mod modifier {
    pub const SHIFT: i16 = 1 << 0;
    pub const ALT: i16 = 1 << 1;
    /// Ctrl off macOS, Command on macOS.
    pub const COMMAND: i16 = 1 << 2;
    /// Ctrl on macOS.
    pub const CONTROL: i16 = 1 << 3;
}

/// The keys the host passed and not yet taken, plus whether a text box is
/// being edited (the window updates it every frame).
#[derive(Debug, Default)]
pub struct HostKeys {
    editing: AtomicBool,
    queue: Mutex<Vec<HostKey>>,
}

impl HostKeys {
    /// A key from the host's VST3 `onKeyDown` / `onKeyUp`: true when the
    /// editor takes it. Only while a text box is edited, and only on
    /// Windows (elsewhere the hosts give the editor's window the keys), so
    /// the host keeps its own shortcuts.
    pub fn vst3(&self, down: bool, character: u16, key_code: i16, modifiers: i16) -> bool {
        if !cfg!(windows) || !self.editing.load(Ordering::Relaxed) {
            return false;
        }
        self.push(HostKey { down, character, key_code, modifiers });
        true
    }

    fn push(&self, key: HostKey) {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).push(key);
    }

    /// Whether a text box is being edited (the window sets it each frame).
    pub fn set_editing(&self, editing: bool) {
        self.editing.store(editing, Ordering::Relaxed);
    }

    /// The keys passed since the last call, oldest first.
    pub fn take(&self) -> Vec<HostKey> {
        std::mem::take(&mut *self.queue.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_pass_only_while_a_text_box_is_edited() {
        let keys = HostKeys::default();
        assert!(!keys.vst3(true, 'a' as u16, 0, 0));
        keys.set_editing(true);
        assert_eq!(keys.vst3(true, 'a' as u16, 0, 0), cfg!(windows));
        let taken = keys.take();
        assert_eq!(taken.len(), usize::from(cfg!(windows)));
        assert!(keys.take().is_empty());
    }
}
