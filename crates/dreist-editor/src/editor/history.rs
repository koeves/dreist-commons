//! The header's undo / redo buttons and history menus.

use crate::*;

impl Editor {
    /// Undo/Redo buttons: restore a snapshot and rebuild the engine.
    pub(crate) fn undo_redo(&mut self, ev: &UiEvent) -> bool {
        // [re:54e11d]: the newest 16 action
        // names (a divider after the first), or a disabled "No undo/redo
        // actions available"; choosing item i steps i + 1 times.
        if let UiEvent::MenuSelected { index, .. } = ev {
            if let Some(redo) = self.history_menu.take() {
                let steps = match *index {
                    0 => 1,
                    k => k,
                };
                for _ in 0..steps {
                    if !self.step_undo(redo) {
                        break;
                    }
                }
                return true;
            }
        }
        let UiEvent::Clicked { path, name, .. } = ev else { return false };
        match name.as_str() {
            "UndoButton" => {
                self.step_undo(false);
                true
            }
            "RedoButton" => {
                self.step_undo(true);
                true
            }
            "UndoHistory" | "RedoHistory" => {
                let redo = name == "RedoHistory";
                let list = self.history.labels(redo);
                let mut items: Vec<String> = Vec::new();
                if list.is_empty() {
                    items.push(format!("(No {} actions available)", if redo { "redo" } else { "undo" }));
                } else {
                    for (i, label) in list.iter().take(16).enumerate() {
                        items.push(label.to_string());
                        if i == 0 && list.len() > 1 {
                            items.push(String::new());
                        }
                    }
                    self.history_menu = Some(redo);
                }
                self.app.open_menu(path, items, None);
                true
            }
            _ => false,
        }
    }

    /// One undo (or redo) step: restore the snapshot and rebuild the engine.
    pub(crate) fn step_undo(&mut self, redo: bool) -> bool {
        let current = std::mem::replace(&mut self.model.doc, dreist_formats::Node::new("root", None));
        // The metronome settings are not undone.
        match self.history.step(redo, current, header::carry_metronome) {
            Ok(doc) => {
                self.model.doc = doc;
                self.outbox.push(self.model.rebuild_command());
                true
            }
            Err(current) => {
                self.model.doc = current;
                false
            }
        }
    }

    /// Option `MaxUndoSteps` (1..250, default 20).
    pub(crate) fn undo_limit(&self) -> usize {
        self.modals.options.int("MaxUndoSteps", modal::UNDO_DEFAULT).clamp(1, 250) as usize
    }

    /// Undo entry name for the event that changed the document (the
    /// control's display name, else its name).
    pub(crate) fn undo_label(&self, ev: &UiEvent) -> String {
        let path = match ev {
            UiEvent::Clicked { path, .. }
            | UiEvent::ValueChanged { path, .. }
            | UiEvent::TextCommitted { path, .. }
            | UiEvent::RightClicked { path, .. }
            | UiEvent::Pressed { path, .. }
            | UiEvent::Mouse { path, .. }
            | UiEvent::Wheel { path, .. }
            | UiEvent::DoubleClicked { path, .. } => Some(path),
            UiEvent::MenuSelected { anchor, .. } | UiEvent::MenuSlider { anchor, .. } => Some(anchor),
            UiEvent::Shortcut { key, .. } => return format!("Key {key}"),
            UiEvent::DragOver { .. }
            | UiEvent::Hover { .. }
            | UiEvent::HoverChanged { .. }
            | UiEvent::Collapse { .. }
            | UiEvent::Animated { .. }
            | UiEvent::Files { .. }
            | UiEvent::Split { .. }
            | UiEvent::DragOut { .. }
            | UiEvent::ToolTip { .. } => return "Edit".into(),
            UiEvent::Dropped { .. } => return if self.drop_label.is_empty() { "Drag and drop".into() } else { self.drop_label.clone() },
        };
        path.and_then(|p| self.app.ctrl(p))
            .map(|c| {
                if !c.display_name.is_empty() {
                    c.display_name.clone()
                } else if !c.name.is_empty() {
                    c.name.clone()
                } else {
                    "Edit".into()
                }
            })
            .unwrap_or_else(|| "Edit".into())
    }
}
