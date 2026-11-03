//! MIDI / host learn and editor-only binding messages.

use crate::*;

impl Editor {
    /// The names of the containers on `path` (root first).
    pub(crate) fn ancestors(&self, path: &[usize]) -> Vec<String> {
        (0..path.len()).filter_map(|n| self.app.ctrl(&path[..n]).map(|c| c.name.clone())).collect()
    }

    /// An editor-only binding for the control at `path` (`editbind`), if any.
    pub(crate) fn editor_binding_target(&self, path: &[usize], ctrl: &Ctrl) -> Option<learn::Target> {
        let anc = self.ancestors(path);
        let parent = anc.last().cloned().unwrap_or_default();
        let e = self.model.engine;
        let found = editbind::TABLE.iter().find(|b| b.control == ctrl.name && anc.iter().any(|a| a == b.scope)).map(|b| (b.group.to_string(), b.name.to_string(), b.kind, b.id));
        let found = found.or_else(|| match parent.as_str() {
            "EditorTabButtons" => editbind::EDITOR_TABS.iter().find(|t| t.0 as i32 == ctrl.id).map(|t| ("Panels".to_string(), t.1.to_string(), "Trigger", t.2)),
            "UtilityTabButtonGroup" => editbind::UTILITY_TABS.iter().find(|t| t.0 as i32 == ctrl.id).map(|t| ("Panels".to_string(), t.1.to_string(), "Trigger", t.2)),
            "ModeGroup" => {
                let i = *path.last()?;
                editbind::MIXER_MODES.get(i).map(|m| ("Mixer".to_string(), m.0.to_string(), "Trigger", m.1))
            }
            _ => None,
        })?;
        let (group, name, kind, id) = found;
        let group = if group == "Engine" { format!("Engine {}", e + 1) } else { group };
        let mut proto = dreist_engine::binding::Message::new(id, &[], &[0.0, 0.0, 0.0]);
        proto.editor = true;
        Some(learn::Target { path: path.to_vec(), proto, stuffer: 0, name, group, kind })
    }

    pub(crate) fn binding_message(&mut self, m: &dreist_engine::binding::Message) {
        let v = m.value[0];
        // MIDI follow (note start → [re:ea55ac] / pad selection).
        let (e, pad) = (m.index[0], m.index[1]);
        match m.kind {
            dreist_engine::binding::id::UPDATE_ENGINE_SELECTION_CHANGED if (0..model::ENGINES as i32).contains(&e) => {
                let e = e as usize;
                self.model.engine = e;
                self.model.engine_sel = 1 << e;
                self.model.doc.set("CurrentEngine", e);
                return;
            }
            dreist_engine::binding::id::UPDATE_PAD_SELECTION_CHANGED if (0..model::ENGINES as i32).contains(&e) && (0..64).contains(&pad) => {
                let c = multipad::select(&mut self.model, e as usize, pad as usize, false, false);
                self.outbox.push(c);
                return;
            }
            _ => {}
        }
        let find = |ed: &Editor, scope: &str, name: &str| -> Option<Vec<usize>> {
            let sp = ed.app.root.path_of(scope)?;
            let q = (ed.app.ctrl(&sp)?).path_of(name)?;
            Some([sp, q].concat())
        };
        let click = |ed: &mut Editor, p: Vec<usize>, on: bool| {
            let Some(c) = ed.app.ctrl_mut(&p) else { return };
            c.on = on;
            let ev = UiEvent::Clicked { path: p, name: c.name.clone(), id: c.id, on };
            ed.handle(&ev);
        };
        if let Some(b) = editbind::by_id(m.kind) {
            let Some(p) = find(self, b.scope, b.control) else { return };
            let cur = self.app.ctrl(&p).map(|c| c.on).unwrap_or(false);
            match m.kind {
                // Set from the value (on when v ≥ 0.5).
                0x1c5..=0x1c7 | 0x1cd | 0x1ce | 0x15c..=0x15e => click(self, p, v >= 0.5),
                // Flip (0x1bc–0x1be).
                0x1bc..=0x1be => click(self, p, !cur),
                // ⌊v·1023 + 1⌋ steps.
                0x1b2 => {
                    let n = (v.clamp(0.0, 1.0) * 1023.0 + 1.0).floor() as u32;
                    self.handle(&UiEvent::TextCommitted { path: p, name: b.control.into(), text: n.to_string() });
                }
                // The popup item round(v·(n − 1)).
                0x1b3 => {
                    let n = dreist_engine::pattern::RESOLUTION_NAMES.len();
                    let index = (v.clamp(0.0, 1.0) * (n - 1) as f32).round() as usize;
                    self.handle(&UiEvent::MenuSelected { anchor: p, name: b.control.into(), index });
                }
                _ => click(self, p, cur),
            }
            return;
        }
        let tab = |ed: &mut Editor, group: &str, id: usize| {
            let Some(g) = ed.app.root.path_of(group) else { return };
            let Some(i) = ed.app.ctrl(&g).and_then(|c| c.children.iter().position(|k| k.id == id as i32)) else { return };
            click(ed, [g, vec![i]].concat(), true);
        };
        if let Some(t) = editbind::EDITOR_TABS.iter().find(|t| t.2 == m.kind) {
            tab(self, "EditorTabButtons", t.0);
        } else if let Some(t) = editbind::UTILITY_TABS.iter().find(|t| t.2 == m.kind) {
            tab(self, "UtilityTabButtonGroup", t.0);
        } else if let Some(i) = editbind::MIXER_MODES.iter().position(|x| x.1 == m.kind) {
            // The shown mixer's mode group.
            fn visible(c: &Ctrl, path: &mut Vec<usize>, out: &mut Option<Vec<usize>>) {
                if !c.visible || out.is_some() {
                    return;
                }
                if c.name == "ModeGroup" {
                    *out = Some(path.clone());
                    return;
                }
                for (k, ch) in c.children.iter().enumerate() {
                    path.push(k);
                    visible(ch, path, out);
                    path.pop();
                }
            }
            let mut g = None;
            visible(&self.app.root, &mut Vec::new(), &mut g);
            if let Some(g) = g {
                click(self, [g, vec![i]].concat(), true);
            }
        }
    }

    /// MIDI learn: run a control event to find the message it posts (the
    /// binding prototype), then undo its effect.
    pub(crate) fn learn_capture(&mut self, ev: &UiEvent) -> Option<learn::Target> {
        let path = learn::event_path(ev)?.to_vec();
        // [re:7ac7c3]: every
        // control of a graph's controls binds the lane itself
        // (`e_setInstoMIDIGraphValue` 0xd9 [e, track, −1, pattern, track]
        // [value, lane]).
        if let Some((track, lane)) = self.pattern.as_ref().and_then(|pp| pp.graph_binding_target(&self.app, &self.model, &path)) {
            let e = self.model.engine;
            let pat = self.model.current_pattern(e);
            let proto = dreist_engine::binding::Message::new(dreist_engine::binding::id::SET_INSTO_MIDI_GRAPH_VALUE, &[e as i32, track as i32, -1, pat as i32, track as i32], &[0.0, lane as f32]);
            let name = dreist_formats::STEP_LANES.get(lane).copied().unwrap_or("").to_string();
            return Some(learn::Target { path, proto, stuffer: 0, name, group: format!("Engine {} | Pattern {} | Track {}", e + 1, pat + 1, track + 1), kind: "Continuous" });
        }
        if let Some((kind, name)) = self.inspector.marker_learn(&self.app, &path) {
            let e = self.model.engine;
            let (p, l) = (self.model.pad[e], self.model.layer[e]);
            let proto = dreist_engine::binding::Message::new(kind, &[e as i32, p as i32, l as i32], &[0.0]);
            return Some(learn::Target { path, proto, stuffer: 0, name: name.into(), group: format!("Engine {} | Pad {} | Layer {}", e + 1, p + 1, l + 1), kind: "Continuous" });
        }
        let ctrl = self.app.ctrl(&path)?.clone();
        if let Some(t) = self.editor_binding_target(&path, &ctrl) {
            return Some(t);
        }
        if let Some((group, name, kind, msg_id)) = self.app.root.path_of("Header").filter(|h| path.starts_with(h)).and_then(|_| header::binding_for(&ctrl.name)) {
            let proto = dreist_engine::binding::Message::new(msg_id, &[], &[0.0, 0.0, 0.0]);
            return Some(learn::Target { path, proto, stuffer: 0, name: name.into(), group: group.into(), kind });
        }
        let synthetic = match ev {
            UiEvent::RightClicked { .. } | UiEvent::Pressed { .. } if ctrl.class == "CtrlButton" => UiEvent::Clicked { path: path.clone(), name: ctrl.name.clone(), id: ctrl.id, on: ctrl.on },
            UiEvent::RightClicked { .. } | UiEvent::Pressed { .. } => UiEvent::ValueChanged { path: path.clone(), name: ctrl.name.clone(), id: ctrl.id, value: ctrl.value },
            e => e.clone(),
        };
        let before = self.model.doc.clone();
        let view = self.model.pattern_view;
        let n = self.outbox.len();
        self.route(&synthetic);
        let msgs: Vec<dreist_engine::binding::Message> = self.outbox.drain(n..).filter_map(|c| if let Command::Message(m) = c { Some(m) } else { None }).collect();
        self.model.doc = before;
        self.model.pattern_view = view;
        let proto = msgs.last()?.clone();
        let (stuffer, kind) = match &synthetic {
            UiEvent::ValueChanged { value, .. } => (learn::stuffer_for(&proto, *value), "Continuous"),
            UiEvent::Clicked { on, .. } => (learn::stuffer_for(&proto, if *on { 1.0 } else { 0.0 }), if ctrl.attr_bool("optIsMomentary", false) { "Trigger" } else { "Latching" }),
            _ => (0, "Continuous"),
        };
        let name = if ctrl.display_name.is_empty() { ctrl.name.clone() } else { ctrl.display_name.clone() };
        Some(learn::Target { path, proto, stuffer, name, group: "Global".into(), kind })
    }

    /// Bind the learn target to MIDI channel/CC (`midi`) or host parameter.
    pub(crate) fn learn_bind(&mut self, midi: bool, a: usize, b: usize) {
        let Some(t) = self.learn.target.clone() else { return };
        let node = midimap::add_binding(&mut self.model.doc, midi, a, b, &t.proto, t.stuffer, &t.group, &t.name, t.kind);
        if let Some(b) = dreist_engine::binding::NewBinding::from_node(&node) {
            self.outbox.push(Command::AddBinding(Box::new(b)));
        }
        self.learn.labels.insert(t.path.clone(), if midi { format!("CC {b}") } else { format!("Param {}", a + 1) });
        self.sync();
    }

    /// Learn-mode events; true when consumed.
    pub(crate) fn learn_event(&mut self, ev: &UiEvent) -> bool {
        if let UiEvent::Clicked { name, .. } = ev {
            if name == "LearnButton" {
                self.learn.active = !self.learn.active;
                self.learn.target = None;
                self.inspector.learn_buttons(&mut self.app, self.learn.active);
                return true;
            }
        }
        if !self.learn.active {
            return false;
        }
        if let UiEvent::MenuSelected { index, .. } = ev {
            let Some(stage) = self.learn.menu.take() else { return false };
            let anchor = self.learn.target.as_ref().map(|t| t.path.clone());
            let next = match (&stage, *index) {
                (learn::MenuStage::Top, 0) => Some(learn::MenuStage::Channels),
                (learn::MenuStage::Top, 1) => Some(learn::MenuStage::HostGroups),
                (learn::MenuStage::Top, 3) => {
                    if let Some(t) = self.learn.target.clone() {
                        if midimap::remove_matching(&mut self.model.doc, t.proto.kind, &t.proto.index) {
                            self.outbox.push(self.model.rebuild_command());
                        }
                        self.learn.labels.remove(&t.path);
                    }
                    None
                }
                (learn::MenuStage::Channels, c) => Some(learn::MenuStage::CcGroups(c)),
                (learn::MenuStage::CcGroups(c), g) => Some(learn::MenuStage::Ccs(*c, g)),
                (learn::MenuStage::Ccs(c, g), k) => {
                    self.learn_bind(true, *c, g * 16 + k);
                    None
                }
                (learn::MenuStage::HostGroups, g) => Some(learn::MenuStage::Params(g)),
                (learn::MenuStage::Params(g), k) => {
                    self.learn_bind(false, g * 16 + k, 0);
                    None
                }
                _ => None,
            };
            if let (Some(s), Some(a)) = (next, anchor) {
                self.app.open_menu(&a, learn::Learn::menu_items(&s), None);
                self.learn.menu = Some(s);
            }
            return true;
        }
        let path = learn::event_path(ev).map(<[usize]>::to_vec);
        let right = matches!(ev, UiEvent::RightClicked { .. });
        // The press picks the target; the click / value change it causes
        // is swallowed.
        if learn::is_control_event(ev) {
            return true;
        }
        if right || matches!(ev, UiEvent::Pressed { .. }) {
            if let Some(t) = self.learn_capture(ev) {
                self.learn.target = Some(t);
                if right {
                    if let Some(p) = path {
                        self.app.open_menu(&p, learn::Learn::menu_items(&learn::MenuStage::Top), None);
                        self.learn.menu = Some(learn::MenuStage::Top);
                    }
                }
            }
            return true;
        }
        false
    }
}
