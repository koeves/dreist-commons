//! Single-line editing for text boxes: caret, selection, word and line
//! moves, deletion and undo ([re:f26c4b], [re:e25508]). Positions count
//! characters; the text itself stays in the control.

/// Attributes of a control being edited: the caret and the selection's
/// other end, for the drawing.
pub const CARET: &str = "editCaret";
pub const ANCHOR: &str = "editAnchor";

/// Undo steps kept per edit.
const UNDO_STEPS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    WordLeft,
    WordRight,
    Start,
    End,
}

type Snap = (String, usize, usize);

/// The edit of one text box.
#[derive(Clone, Debug)]
pub struct Edit {
    pub path: Vec<usize>,
    /// The text before the edit (Escape puts it back).
    pub orig: String,
    pub caret: usize,
    /// The other end of the selection (the caret when nothing is selected).
    pub anchor: usize,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
}

fn len(text: &str) -> usize {
    text.chars().count()
}

fn byte(text: &str, pos: usize) -> usize {
    text.char_indices().nth(pos).map_or(text.len(), |(i, _)| i)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Edit {
    /// An edit of `text` with all of it selected.
    pub fn new(path: Vec<usize>, text: &str) -> Edit {
        Edit { path, orig: text.to_string(), caret: len(text), anchor: 0, undo: Vec::new(), redo: Vec::new() }
    }

    /// The selection, first and one past the last character.
    pub fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn selected<'t>(&self, text: &'t str) -> &'t str {
        let (lo, hi) = self.selection();
        &text[byte(text, lo)..byte(text, hi)]
    }

    /// Put the caret at `pos`; `extend` keeps the anchor (a selection).
    pub fn set_caret(&mut self, text: &str, pos: usize, extend: bool) {
        self.caret = pos.min(len(text));
        if !extend {
            self.anchor = self.caret;
        }
    }

    pub fn select_all(&mut self, text: &str) {
        self.anchor = 0;
        self.caret = len(text);
    }

    /// Select the word (or the run of other characters) at `pos`.
    pub fn select_word(&mut self, text: &str, pos: usize) {
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return;
        }
        let at = pos.min(chars.len() - 1);
        let kind = is_word(chars[at]);
        let lo = (0..at).rev().take_while(|&i| is_word(chars[i]) == kind).last().unwrap_or(at);
        let hi = (at..chars.len()).take_while(|&i| is_word(chars[i]) == kind).last().unwrap_or(at) + 1;
        self.anchor = lo;
        self.caret = hi;
    }

    /// A caret move; without `extend` a selection collapses to its end in
    /// the move's direction.
    pub fn move_caret(&mut self, text: &str, m: Move, extend: bool) {
        let chars: Vec<char> = text.chars().collect();
        let (lo, hi) = self.selection();
        let selected = lo != hi && !extend;
        let pos = match m {
            Move::Left if selected => lo,
            Move::Right if selected => hi,
            Move::Left => self.caret.saturating_sub(1),
            Move::Right => (self.caret + 1).min(chars.len()),
            Move::WordLeft => {
                let mut i = if selected { lo } else { self.caret };
                while i > 0 && !is_word(chars[i - 1]) {
                    i -= 1;
                }
                while i > 0 && is_word(chars[i - 1]) {
                    i -= 1;
                }
                i
            }
            Move::WordRight => {
                let mut i = if selected { hi } else { self.caret };
                while i < chars.len() && !is_word(chars[i]) {
                    i += 1;
                }
                while i < chars.len() && is_word(chars[i]) {
                    i += 1;
                }
                i
            }
            Move::Start => 0,
            Move::End => chars.len(),
        };
        self.set_caret(text, pos, extend);
    }

    fn snap(&mut self, text: &str) {
        if self.undo.len() == UNDO_STEPS {
            self.undo.remove(0);
        }
        self.undo.push((text.to_string(), self.caret, self.anchor));
        self.redo.clear();
    }

    fn replace(&mut self, text: &mut String, lo: usize, hi: usize, with: &str) {
        self.snap(text);
        text.replace_range(byte(text, lo)..byte(text, hi), with);
        self.caret = lo + len(with);
        self.anchor = self.caret;
    }

    /// Typed or pasted text in place of the selection (one line: control
    /// characters are left out).
    pub fn insert(&mut self, text: &mut String, s: &str) -> bool {
        let s: String = s.chars().filter(|c| !c.is_control()).collect();
        let (lo, hi) = self.selection();
        if s.is_empty() && lo == hi {
            return false;
        }
        self.replace(text, lo, hi, &s);
        true
    }

    /// Delete the selection, else the character before (`forward`: after)
    /// the caret.
    pub fn delete(&mut self, text: &mut String, forward: bool) -> bool {
        let (mut lo, mut hi) = self.selection();
        if lo == hi {
            if forward {
                hi = (hi + 1).min(len(text));
            } else {
                lo = lo.saturating_sub(1);
            }
        }
        if lo == hi {
            return false;
        }
        self.replace(text, lo, hi, "");
        true
    }

    /// Take the selection out of the text.
    pub fn cut(&mut self, text: &mut String) -> Option<String> {
        let s = self.selected(text).to_string();
        if s.is_empty() {
            return None;
        }
        self.delete(text, false);
        Some(s)
    }

    pub fn undo(&mut self, text: &mut String) -> bool {
        let Some((t, caret, anchor)) = self.undo.pop() else { return false };
        self.redo.push((std::mem::replace(text, t), self.caret, self.anchor));
        (self.caret, self.anchor) = (caret, anchor);
        true
    }

    pub fn redo(&mut self, text: &mut String) -> bool {
        let Some((t, caret, anchor)) = self.redo.pop() else { return false };
        self.undo.push((std::mem::replace(text, t), self.caret, self.anchor));
        (self.caret, self.anchor) = (caret, anchor);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(text: &str) -> (Edit, String) {
        let mut e = Edit::new(Vec::new(), text);
        e.set_caret(text, usize::MAX, false);
        (e, text.to_string())
    }

    #[test]
    fn a_new_edit_selects_everything_and_typing_replaces_it() {
        let mut t = "120.00".to_string();
        let mut e = Edit::new(Vec::new(), &t);
        assert_eq!(e.selected(&t), "120.00");
        assert!(e.insert(&mut t, "98"));
        assert_eq!((t.as_str(), e.caret, e.anchor), ("98", 2, 2));
    }

    #[test]
    fn typing_goes_in_at_the_caret() {
        let (mut e, mut t) = edit("Kick");
        e.move_caret(&t, Move::Start, false);
        e.insert(&mut t, "Big ");
        assert_eq!((t.as_str(), e.caret), ("Big Kick", 4));
        e.insert(&mut t, "\r\n");
        assert_eq!(t, "Big Kick");
    }

    #[test]
    fn moves_by_character_word_and_line() {
        let (mut e, t) = edit("Snare top, läuft 2");
        e.move_caret(&t, Move::Left, false);
        assert_eq!(e.caret, 17);
        e.move_caret(&t, Move::WordLeft, false);
        assert_eq!(e.caret, 11);
        e.move_caret(&t, Move::WordLeft, false);
        assert_eq!(e.caret, 6);
        e.move_caret(&t, Move::WordRight, false);
        assert_eq!(e.caret, 9);
        e.move_caret(&t, Move::Start, false);
        assert_eq!(e.caret, 0);
        e.move_caret(&t, Move::Left, false);
        assert_eq!(e.caret, 0);
        e.move_caret(&t, Move::End, false);
        e.move_caret(&t, Move::Right, false);
        assert_eq!(e.caret, 18);
    }

    #[test]
    fn shift_extends_and_a_plain_move_collapses_the_selection() {
        let (mut e, t) = edit("Hat open");
        e.move_caret(&t, Move::WordLeft, true);
        assert_eq!((e.selection(), e.selected(&t)), ((4, 8), "open"));
        e.move_caret(&t, Move::Left, true);
        assert_eq!(e.selected(&t), " open");
        e.move_caret(&t, Move::Right, false);
        assert_eq!((e.caret, e.anchor), (8, 8));
        e.move_caret(&t, Move::Start, true);
        e.move_caret(&t, Move::Left, false);
        assert_eq!((e.caret, e.anchor), (0, 0));
    }

    #[test]
    fn deletes_backward_forward_and_the_selection() {
        let (mut e, mut t) = edit("Tömb");
        assert!(e.delete(&mut t, false));
        assert_eq!(t, "Töm");
        assert!(!e.delete(&mut t, true));
        e.move_caret(&t, Move::Start, false);
        assert!(!e.delete(&mut t, false));
        assert!(e.delete(&mut t, true));
        assert_eq!(t, "öm");
        e.select_all(&t);
        assert!(e.delete(&mut t, true));
        assert_eq!((t.as_str(), e.caret), ("", 0));
    }

    #[test]
    fn cut_and_word_selection() {
        let (mut e, mut t) = edit("Clap (wide) 2");
        e.select_word(&t, 7);
        assert_eq!(e.selected(&t), "wide");
        e.select_word(&t, 5);
        assert_eq!(e.selected(&t), " (");
        e.select_word(&t, 99);
        assert_eq!(e.selected(&t), "2");
        e.select_word(&t, 7);
        assert_eq!(e.cut(&mut t).as_deref(), Some("wide"));
        assert_eq!((t.as_str(), e.caret), ("Clap () 2", 6));
        assert_eq!(e.cut(&mut t), None);
    }

    #[test]
    fn undo_and_redo_restore_text_and_selection() {
        let (mut e, mut t) = edit("Tom");
        e.insert(&mut t, " 1");
        e.select_all(&t);
        e.insert(&mut t, "x");
        assert!(e.undo(&mut t));
        assert_eq!((t.as_str(), e.selection()), ("Tom 1", (0, 5)));
        assert!(e.undo(&mut t));
        assert_eq!(t, "Tom");
        assert!(!e.undo(&mut t));
        assert!(e.redo(&mut t));
        assert!(e.redo(&mut t));
        assert_eq!(t, "x");
        assert!(!e.redo(&mut t));
        e.undo(&mut t);
        e.insert(&mut t, "!");
        assert!(!e.redo(&mut t));
    }
}
