//! Keyboard accelerators and the standalone's menu bar.

use crate::*;

/// Menu bar tags beyond the accelerator ids.
const MENU_ABOUT: isize = 1000;

const MENU_AUDIO: isize = 1001;

const MENU_MIDI: isize = 1002;

impl Editor {
    /// [re:907a56] (see `accel`): run the key's
    /// function. Keys a panel uses first (pattern step nudges, delete in
    /// the pattern / song editors) are left to the panels.
    pub(crate) fn accelerator(&mut self, key: &str, mods: accel::Mods) -> bool {
        let o = &self.modals.options;
        let Some(e) = accel::lookup(key, mods, o.bool("RespondToKeyboard", true), o.bool("RespondToExtendedKeyboard", false), self.menu_bar) else { return false };
        if e.id == 0x18 || (matches!(key, "left" | "right") && self.model.editor_tab == 0 && self.pattern.as_ref().is_some_and(|p| p.has_selection())) {
            return false;
        }
        self.run_action(e.action);
        true
    }

    pub(crate) fn run_action(&mut self, action: accel::Action) {
        match action {
            accel::Action::Nothing => {}
            accel::Action::Toggle(button) | accel::Action::Press(button) => {
                let Some(p) = self.app.root.path_of("Header").and_then(|h| Some([h.clone(), (self.app.ctrl(&h)?).path_of(button)?].concat())) else { return };
                let Some(c) = self.app.ctrl_mut(&p) else { return };
                if matches!(action, accel::Action::Toggle(_)) {
                    c.on = !c.on;
                }
                let ev = UiEvent::Clicked { path: p, name: c.name.clone(), id: c.id, on: c.on };
                self.handle(&ev);
            }
            accel::Action::Editor(id) => self.accelerator_message(id),
        }
    }

    /// The standalone's menu bar ([re:80cba3]: the
    /// entry's function, whatever the keyboard options say).
    pub fn menu_bar_items(&self) -> (Vec<dreist_gui::menubar::Item>, Vec<(String, Vec<dreist_gui::menubar::Item>)>) {
        use dreist_gui::menubar::Item;
        fn item(id: u32) -> Item {
            let (title, _) = accel::entry(id).unwrap_or(("", accel::Action::Nothing));
            let k = accel::TABLE.iter().find(|e| e.id == id);
            Item {
                title: title.into(),
                key: k.map(|k| k.key.to_string()).unwrap_or_default(),
                shift: k.is_some_and(|k| k.mods.shift),
                cmd: k.is_some_and(|k| k.mods.cmd),
                alt: k.is_some_and(|k| k.mods.alt),
                ctrl: k.is_some_and(|k| k.mods.ctrl),
                tag: id as isize,
                submenu: Vec::new(),
            }
        }
        fn items(m: &[accel::Menu]) -> Vec<Item> {
            m.iter()
                .map(|e| match e {
                    accel::Menu::Item(id) => item(*id),
                    accel::Menu::Separator => Item::separator(),
                    accel::Menu::Sub(t, sub) => Item { title: (*t).into(), submenu: items(sub), ..Item::default() },
                })
                .collect()
        }
        let app = vec![
            Item { title: format!("About {}", crate::product()), tag: MENU_ABOUT, ..Item::default() },
            Item::separator(),
            item(0x00),
            Item { title: "Audio Settings...".into(), tag: MENU_AUDIO, ..Item::default() },
            Item { title: "MIDI Settings...".into(), tag: MENU_MIDI, ..Item::default() },
        ];
        (app, accel::MENUS.iter().map(|(t, m)| (t.to_string(), items(m))).collect())
    }

    /// A menu bar item was chosen.
    pub fn menu_command(&mut self, tag: isize) {
        match tag {
            MENU_ABOUT => self.handle(&UiEvent::MenuSelected { anchor: Vec::new(), name: "HelpButton".into(), index: 0 }),
            MENU_AUDIO => self.devices.show(devices::Panel::Audio),
            MENU_MIDI => self.devices.show(devices::Panel::Midi),
            t => {
                if let Some((_, a)) = u32::try_from(t).ok().and_then(accel::entry) {
                    self.run_action(a);
                }
            }
        }
        self.sync();
        self.app.dirty = true;
    }

    /// The editor messages the shortcuts post ([re:695294]).
    pub(crate) fn accelerator_message(&mut self, id: u32) {
        use project::Part;
        let e = self.model.engine;
        let menu = |ed: &mut Editor, name: &str, index: usize| ed.handle(&UiEvent::MenuSelected { anchor: Vec::new(), name: name.into(), index });
        let file = |ed: &mut Editor, action: FileAction| {
            ed.file_menu = Some(vec![action]);
            menu(ed, "LoadButton", 0);
        };
        match id {
            0x16a => menu(self, "ToolButton", 0),
            0x16c => file(self, FileAction::OpenPreset),
            0x16d => file(self, FileAction::Open(Part::Engine(e))),
            0x16e => file(self, FileAction::Open(Part::Kit(e))),
            0x16f => file(self, FileAction::Open(Part::Pad(e, self.model.pad[e]))),
            0x170 => file(self, FileAction::Open(Part::Bank(e))),
            0x171 => file(self, FileAction::Open(Part::Pattern(e, self.model.current_pattern(e)))),
            0x172 => file(self, FileAction::OpenMidi),
            0x173 => file(self, FileAction::Open(Part::Mappings)),
            0x175 => file(self, FileAction::Save { part: Part::Engine(e), as_new: false }),
            0x177 => file(self, FileAction::Save { part: Part::Engine(e), as_new: true }),
            0x17a => file(self, FileAction::Save { part: Part::Pad(e, self.model.pad[e]), as_new: true }),
            0x17b => file(self, FileAction::Save { part: Part::Bank(e), as_new: true }),
            0x17c => file(self, FileAction::Save { part: Part::Pattern(e, self.model.current_pattern(e)), as_new: true }),
            0x17d => file(self, FileAction::SaveMidi),
            0x17e => file(self, FileAction::Save { part: Part::Mappings, as_new: true }),
            0x17f => menu(self, "ToolButton", 8),
            0x174 => file(self, FileAction::SavePreset { as_new: false }),
            0x176 => file(self, FileAction::SavePreset { as_new: true }),
            0x178 => file(self, FileAction::Save { part: Part::Kit(e), as_new: false }),
            0x179 => file(self, FileAction::Save { part: Part::Kit(e), as_new: true }),
            // e_updateDelete: the pattern / song editors delete their selection.
            0x18f => self.handle(&UiEvent::Shortcut { key: "delete".into(), cmd: false, shift: false, alt: false, ctrl: false }),
            0x152 => menu(self, "HelpButton", 2),
            0x153 => menu(self, "HelpButton", 3),
            // Header Retro (`e_grabRetroRecording`).
            0x23 => {
                if let Some(p) = self.app.root.path_of("Header").and_then(|h| Some([h.clone(), (self.app.ctrl(&h)?).path_of("RetroButton")?].concat())) {
                    let ev = UiEvent::Clicked { path: p, name: "RetroButton".into(), id: 0, on: false };
                    self.handle(&ev);
                }
            }
            // [re:dd7138]: Filter starts editing
            // the search box; Find is the box menu's "Advanced search..."
            // (its listeners are notified [re:80cb0a] → the pane toggles).
            0x160 | 0x161 => {
                self.binding_message(&dreist_engine::binding::Message::new(0x14e, &[], &[1.0]));
                let Some(b) = &mut self.browser else { return };
                if id == 0x160 {
                    let page = b.page().to_vec();
                    if let Some(q) = self.app.ctrl(&page).and_then(|c| c.path_of("SearchBox")) {
                        self.app.begin_edit(&[page, q].concat());
                    }
                } else {
                    b.toggle_search_pane(&mut self.app);
                }
            }
            _ => self.binding_message(&dreist_engine::binding::Message::new(id, &[], &[1.0])),
        }
    }
}
