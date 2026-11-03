//! The engine selector and pad bank menus.

use crate::*;

/// Engine selector menu items ([re:b6961a]
/// [re:796ecd]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum EngineAction {
    Rename,
    Cut,
    Copy,
    Paste,
    Power,
    UnMute,
    UnSolo,
    Reset,
    ResetAll,
    ConfirmResetAll,
    None,
}

/// The engine name edit box.
pub(crate) const ENGINE_NAME_EDIT: &str = "EngineNameEdit";

impl Editor {
    /// Pad right-click menu ([re:816870]).
    /// The engine selectors' right-click menu ([re:b6961a]
    ///  [re:796ecd], items [re:45f3d6]..[re:11c0d7]): Rename
    /// engine (0x8a), Cut / Copy / Paste (0x97), Pattern Power (0xa8),
    /// Un-mute / Un-solo all pads / layers on this engine (0x8e / 0x8f,
    /// enabled while the engine's muted / soloed icon is lit), Reset engine
    /// (0x9b), Reset all engines (confirmed; 0x9b for every engine). The
    /// port edits the document and rebuilds.
    pub(crate) fn engine_menu(&mut self, ev: &UiEvent) -> bool {
        use dreist_engine::binding::id;
        match ev {
            UiEvent::RightClicked { path, .. } => {
                let Some(base) = self.header.selectors.clone() else { return false };
                let Some((e, _)) = header::selector_hit(&self.app, &base, path) else { return false };
                let e = e.min(model::ENGINES - 1);
                let icon = |ed: &Editor, n: &str| ed.app.ctrl(&[base.as_slice(), &[e]].concat()).and_then(|s| s.find(n)).is_some_and(|c| c.on);
                let check = |on: bool, t: &str| if on { format!("{}{t}", dreist_gui::menu::CHECK) } else { t.to_string() };
                let items = vec![
                    ("Rename engine".to_string(), EngineAction::Rename),
                    (String::new(), EngineAction::None),
                    ("Cut".into(), EngineAction::Cut),
                    ("Copy".into(), EngineAction::Copy),
                    (dreist_gui::menu::item("Paste", self.clipboard.engine.is_some()), EngineAction::Paste),
                    (String::new(), EngineAction::None),
                    (check(self.model.pattern_power(e), "Pattern Power"), EngineAction::Power),
                    (dreist_gui::menu::item("Un-mute all pads / layers on this engine", icon(self, "EngineMutedIcon")), EngineAction::UnMute),
                    (dreist_gui::menu::item("Un-solo all pads / layers on this engine", icon(self, "EngineSoloedIcon")), EngineAction::UnSolo),
                    (String::new(), EngineAction::None),
                    ("Reset engine".into(), EngineAction::Reset),
                    ("Reset all engines".into(), EngineAction::ResetAll),
                ];
                let (labels, acts): (Vec<String>, Vec<EngineAction>) = items.into_iter().unzip();
                self.engine_menu = Some((path.clone(), e, acts));
                self.app.open_menu(path, labels, None);
                true
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.engine_menu.as_ref().is_some_and(|(p, _, _)| p == anchor) => {
                let Some((path, e, acts)) = self.engine_menu.take() else { return false };
                let name = format!("Engine{e}");
                let rebuild = |ed: &mut Editor| ed.outbox.push(ed.model.rebuild_command());
                let reset = |ed: &mut Editor, k: usize| {
                    let n = format!("Engine{k}");
                    ed.model.doc.children.retain(|c| !(c.class == "Engine" && c.name.as_deref() == Some(&n)));
                };
                match acts.get(*index).copied().unwrap_or(EngineAction::None) {
                    EngineAction::Rename => {
                        let Some(c) = self.app.ctrl(&path).cloned() else { return true };
                        let rect = dreist_gui::Rect::new(c.rect.x + c.rect.w / 2 - 75, c.rect.y + c.rect.h / 2 - 10, 150, 20);
                        self.app.open_edit_box(&path[..path.len() - 1], ENGINE_NAME_EDIT, rect, self.model.engine_name(e));
                        self.engine_rename = Some(e);
                    }
                    EngineAction::Cut | EngineAction::Copy => {
                        let mut n = self.model.engine_node(e).cloned().unwrap_or_else(|| dreist_formats::Node::new("Engine", Some(&name)));
                        n.name = None;
                        self.clipboard.engine = Some(n);
                        if acts[*index] == EngineAction::Cut {
                            reset(self, e);
                            rebuild(self);
                        }
                    }
                    EngineAction::Paste => {
                        let Some(mut n) = self.clipboard.engine.clone() else { return true };
                        n.name = Some(name.clone());
                        match self.model.doc.children.iter_mut().find(|c| c.class == "Engine" && c.name.as_deref() == Some(&name)) {
                            Some(old) => *old = n,
                            None => self.model.doc.children.push(n),
                        }
                        rebuild(self);
                    }
                    EngineAction::Power => self.outbox.push(command::msg(id::SET_PATTERN_POWER_STATE, &[e as i32], &[!self.model.pattern_power(e) as i32 as f32])),
                    EngineAction::UnMute | EngineAction::UnSolo => {
                        let solo = acts[*index] == EngineAction::UnSolo;
                        let (pk, lk) = if solo { (id::SOLO_PAD, id::SET_LAYER_SOLO) } else { (id::MUTE_PAD, id::SET_LAYER_MUTE) };
                        for p in 0..64 {
                            if self.model.pad_node(e, p).is_none() {
                                continue;
                            }
                            self.outbox.push(command::msg(pk, &[e as i32, p as i32], &[0.0]));
                            for l in 0..model::LAYERS {
                                if self.model.layer_node(e, p, l).is_some() {
                                    self.outbox.push(command::msg(lk, &[e as i32, p as i32, l as i32], &[0.0]));
                                }
                            }
                        }
                    }
                    EngineAction::Reset => {
                        reset(self, e);
                        rebuild(self);
                    }
                    EngineAction::ResetAll => {
                        let items = vec!["Are you sure you want to reset all engines? This action cannot be undone!".into(), String::new(), "Yes".into(), "No".into()];
                        self.engine_menu = Some((path.clone(), e, vec![EngineAction::None, EngineAction::None, EngineAction::ConfirmResetAll, EngineAction::None]));
                        self.app.open_menu(&path, items, None);
                    }
                    EngineAction::ConfirmResetAll => {
                        for k in 0..model::ENGINES {
                            reset(self, k);
                        }
                        rebuild(self);
                    }
                    EngineAction::None => {}
                }
                true
            }
            UiEvent::TextCommitted { path, name, text } if name == ENGINE_NAME_EDIT => {
                let parent = path[..path.len() - 1].to_vec();
                if let Some(c) = self.app.ctrl_mut(&parent) {
                    c.children.retain(|k| k.name != ENGINE_NAME_EDIT);
                }
                if let Some(e) = self.engine_rename.take() {
                    let t: String = text.trim().chars().take(63).collect();
                    self.model.engine_mut(e).set("Name", t);
                }
                true
            }
            _ => false,
        }
    }

    pub(crate) fn pad_menu(&mut self, ev: &UiEvent) -> bool {
        match ev {
            UiEvent::RightClicked { path, .. } if self.ancestors(path).last().is_some_and(|n| n == "PadBank") => {
                let b = *path.last().unwrap();
                if b > 3 {
                    return false;
                }
                let mut items: Vec<String> = padmenu::BANK_ITEMS.iter().map(|s| s.to_string()).collect();
                if self.clipboard.bank.is_none() {
                    items[2] = dreist_gui::menu::item("Paste", false);
                }
                self.bank_menu = Some((path.clone(), b));
                self.app.open_menu(path, items, None);
                true
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.bank_menu.as_ref().is_some_and(|(p, _)| p == anchor) => {
                let Some((_, b)) = self.bank_menu.take() else { return false };
                padmenu::bank_action(&mut self.model, &mut self.clipboard, b, *index, &mut self.outbox)
            }
            UiEvent::RightClicked { path, .. } if self.mixers.iter().any(|m| m.pad_strip_at(&self.app, path).is_some()) => {
                let Some(k) = self.mixers.iter().find_map(|m| m.pad_strip_at(&self.app, path)) else { return false };
                let path = path.clone();
                self.padmenu.open(&mut self.app, &path, &self.model, &self.clipboard, k);
                true
            }
            UiEvent::Mouse { path, overload, kind: dreist_gui::app::MouseKind::RightDown, .. } if overload == "LayerRow" => {
                let Some(l) = self.app.ctrl(path).and_then(|r| r.attr("layer")).and_then(|v| v.parse::<usize>().ok()) else { return false };
                let e = self.model.engine;
                let p = self.model.pad[e];
                let items = padmenu::layer_items(&self.model, &self.clipboard, e, p, l);
                self.layer_menu = Some((path.clone(), (e, p, l)));
                self.app.open_menu(path, items, None);
                true
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.layer_menu.as_ref().is_some_and(|(a, _)| a == anchor) => {
                let Some((_, (e, p, l))) = self.layer_menu.take() else { return false };
                let anchor = anchor.clone();
                let o = padmenu::layer_action(&mut self.model, &mut self.clipboard, e, p, l, *index, &mut self.outbox);
                if let Some(f) = o.slicer {
                    self.open_slicer(&f);
                }
                if o.meta && self.metadata.show(&mut self.app, &self.model, &anchor) {
                    return true;
                }
                o.changed
            }
            UiEvent::Mouse { path, overload, kind: dreist_gui::app::MouseKind::RightDown, .. } if overload == "EditorPad" => {
                let Some(idx) = self.app.ctrl(path).and_then(|c| c.attr("padIndex")).and_then(|v| v.parse::<usize>().ok()) else { return false };
                let path = path.clone();
                self.model.pad[self.model.engine] = idx;
                self.padmenu.open(&mut self.app, &path, &self.model, &self.clipboard, idx);
                true
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.padmenu.anchor() == Some(anchor.as_slice()) => {
                let anchor = anchor.clone();
                let o = self.padmenu.select(&mut self.app, &anchor, &mut self.model, &mut self.clipboard, *index, &mut self.outbox);
                if let (Some(d), Some(b)) = (o.browse, self.browser.as_mut()) {
                    b.browse(d);
                    self.model.utility_tab = 0;
                }
                if let Some(l) = o.save {
                    let e = self.model.engine;
                    let p = self.model.pad[e];
                    let stem = self.model.layer_sample(e, p, l).and_then(|f| f.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_default();
                    if let Some(mut dst) = self.dialog(Dialog::SaveSample(format!("{stem}.wav"))) {
                        // The save dialog forces the "wav" extension.
                        dst.set_extension("wav");
                        if let Err(msg) = padmenu::save_layer_sample(&self.model, e, p, l, &dst) {
                            let anchor = anchor.clone();
                            self.app.open_menu(&anchor, vec![msg], None);
                        }
                    }
                }
                o.changed
            }
            _ => false,
        }
    }
}
