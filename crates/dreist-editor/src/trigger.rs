//! Modulator trigger panel (a slot's Trigger button,
//! [re:281769], `Transmod/TriggerPanel.skin`):
//! TStart "Reset at transport start" (message 0x2e
//! `e_setModulationSourceTransportStart`, values [slot, on]), E1..E8
//! "Reset on Engine Pattern Change" (0x2f [engine] [slot, on]), ShowEng
//! picks the engine whose 64 pads fill the 4×16 Matrix of latching
//! `DefaultButtonStyle` buttons (0x2d `e_setModulationSourceTrigger`
//! [engine, pad] [slot, on]); a pad shows its name, or its number while the
//! name is default ([re:4f398b]). Right click on a pad offers
//! Select all / none for this engine or all engines (lambda [re:dc35c9]).
//!
//! Port: the original is a [re:eee2e8] attached to the Trigger
//! button; here it is a modal overlay placed beside the button.

use dreist_engine::binding::id;
use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::{Ctrl, Rect, Skin};

use crate::EditorError;
use crate::command::{Command, msg};
use crate::model::Model;

const MENU: [&str; 5] = ["Select all (this engine)", "Select all (all engines)", "", "Select none (this engine)", "Select none (all engines)"];

pub struct TriggerPanel {
    overlay: Option<Vec<usize>>,
    pub open: Option<usize>,
    /// Engine whose pads are shown.
    engine: usize,
    menu: bool,
}

fn modulator(model: &Model, slot: usize) -> Option<&dreist_formats::Node> {
    model.doc.children.iter().find(|c| c.class == "Modulator" && c.name.as_deref() == Some(&format!("Modulator{slot}")))
}

impl TriggerPanel {
    pub fn build(skin: &mut Skin, root: &mut Ctrl) -> Result<TriggerPanel, EditorError> {
        let overlay = crate::modal::overlay(skin, root, "TriggerPanel.skin", "TriggerOverlay")?;
        if let Some(op) = &overlay {
            let mut o = &mut *root;
            for &i in op {
                o = &mut o.children[i];
            }
            if let Some(m) = o.find_mut("Matrix") {
                let cells = crate::pads::matrix_cells(m, m.attr_i32("intNumberOfCols", 4), m.attr_i32("intNumberOfRows", 16));
                m.children = cells
                    .into_iter()
                    .enumerate()
                    .map(|(p, r)| {
                        let mut b = Ctrl {
                            class: "CtrlButton".into(),
                            name: format!("Pad{p}"),
                            style: "DefaultButtonStyle".into(),
                            text: (p + 1).to_string(),
                            visible: true,
                            enabled: true,
                            ..Ctrl::default()
                        };
                        b.set_rect(Rect::new(r.x + 1, r.y + 1, (r.w - 2).max(1), (r.h - 2).max(1)));
                        b
                    })
                    .collect();
            }
        }
        Ok(TriggerPanel { overlay, open: None, engine: 0, menu: false })
    }

    /// Open for `slot` beside the Trigger button at `anchor`.
    pub fn show(&mut self, app: &mut App, model: &Model, slot: usize, anchor: &[usize]) {
        self.open = Some(slot);
        self.engine = model.engine;
        let (ax, ay) = app.origin(anchor);
        let aw = app.ctrl(anchor).map(|c| c.rect.w).unwrap_or(16);
        let (rw, rh) = (app.root.rect.w, app.root.rect.h);
        if let Some(panel) = self.overlay.clone().and_then(|p| app.ctrl_mut(&p)).and_then(|o| o.children.first_mut()) {
            // `intAnchorPoint` 160: the panel's left edge meets the button
            // 160 px below its top.
            let (pw, ph) = (panel.rect.w, panel.rect.h);
            let x = (ax + aw).min(rw - pw).max(0);
            let y = (ay - 160).min(rh - ph).max(0);
            panel.set_rect(Rect::new(x, y, pw, ph));
        }
    }

    pub fn sync(&self, app: &mut App, model: &Model) {
        let open = self.open.is_some();
        let n = self.open.and_then(|s| modulator(model, s));
        let e = self.engine;
        let Some(o) = self.overlay.clone().and_then(|p| app.ctrl_mut(&p)) else { return };
        o.visible = open;
        let Some(n) = n else { return };
        if let Some(b) = o.find_mut("TStart") {
            b.on = n.bool("ResetAtStart").unwrap_or(true);
        }
        for k in 0..8 {
            if let Some(b) = o.find_mut(&format!("E{}", k + 1)) {
                b.on = n.bool(&format!("EngTrig{k}")).unwrap_or(false);
            }
        }
        if let Some(b) = o.find_mut("ShowEng") {
            b.text = (e + 1).to_string();
        }
        if let Some(m) = o.find_mut("Matrix") {
            for (p, b) in m.children.iter_mut().enumerate() {
                b.on = n.bool(&format!("PadTrig{e}-{p}")).unwrap_or(false);
                b.text = model.pad_name(e, p).unwrap_or_else(|| (p + 1).to_string());
            }
        }
    }

    fn mine(&self, path: &[usize]) -> bool {
        self.open.is_some() && self.overlay.as_ref().is_some_and(|p| path.starts_with(p))
    }

    fn set_pads(&self, slot: usize, engines: std::ops::Range<usize>, on: bool, out: &mut Vec<Command>) {
        for e in engines {
            for p in 0..64 {
                out.push(msg(id::SET_MODULATION_SOURCE_TRIGGER, &[e as i32, p], &[slot as f32, on as i32 as f32]));
            }
        }
    }

    /// Returns whether the event was the panel's.
    pub fn handle(&mut self, app: &mut App, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        let Some(slot) = self.open else { return false };
        let s = slot as f32;
        match ev {
            UiEvent::MenuSelected { anchor, name, index } if self.mine(anchor) => {
                if name == "ShowEng" {
                    self.engine = (*index).min(7);
                } else if std::mem::take(&mut self.menu) {
                    let e = self.engine;
                    match index {
                        0 => self.set_pads(slot, e..e + 1, true, out),
                        1 => self.set_pads(slot, 0..8, true, out),
                        3 => self.set_pads(slot, e..e + 1, false, out),
                        4 => self.set_pads(slot, 0..8, false, out),
                        _ => {}
                    }
                }
                true
            }
            UiEvent::RightClicked { path, name } if self.mine(path) => {
                if name.starts_with("Pad") {
                    app.open_menu(path, MENU.iter().map(|s| s.to_string()).collect(), None);
                    self.menu = true;
                }
                true
            }
            UiEvent::Clicked { path, name, on, .. } if self.mine(path) => {
                let v = if *on { 1.0 } else { 0.0 };
                if name == "TStart" {
                    out.push(msg(id::SET_MODULATION_SOURCE_TRANSPORT_START, &[], &[s, v]));
                } else if name == "ShowEng" {
                    app.open_menu(path, (1..=8).map(|k| k.to_string()).collect(), Some(self.engine));
                } else if let Some(k) = name.strip_prefix('E').and_then(|k| k.parse::<i32>().ok()).filter(|k| (1..=8).contains(k)) {
                    out.push(msg(id::SET_MODULATION_SOURCE_PATTERN_CHANGE, &[k - 1], &[s, v]));
                } else if let Some(p) = name.strip_prefix("Pad").and_then(|p| p.parse::<i32>().ok()) {
                    out.push(msg(id::SET_MODULATION_SOURCE_TRIGGER, &[self.engine as i32, p], &[s, v]));
                }
                true
            }
            UiEvent::Mouse { path, overload, kind: MouseKind::Down, .. } if overload == "ModalOverlay" && self.overlay.as_deref() == Some(path.as_slice()) => {
                self.open = None;
                true
            }
            UiEvent::Mouse { path, .. } | UiEvent::Wheel { path, .. } | UiEvent::Pressed { path, .. } | UiEvent::ValueChanged { path, .. } => self.mine(path),
            _ => false,
        }
    }
}
