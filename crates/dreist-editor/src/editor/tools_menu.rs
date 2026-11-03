//! The Tools and Help menus.

use crate::*;

impl Editor {
    /// Tools ([re:4139f8] [re:a00968]) and Help ([re:0316e2])
    /// menus.
    pub(crate) fn tools_help_menu(&mut self, ev: &UiEvent) -> bool {
        let reset = format!("Reset {}", crate::product());
        let tools: [&str; 9] = ["Preferences", "", &reset, "Save as default preset", "Clear default preset", "", "Find missing files...", "", "Export as audio..."];
        const HELP: [&str; 5] = ["About", "", "Reset UI size to default", "Reset UI scale to 100%", "Show MIDI Console"];
        match ev {
            UiEvent::Clicked { path, name, .. } if (name == "ToolButton" || name == "HelpButton") && !self.browser.as_ref().is_some_and(|b| path.starts_with(b.page())) => {
                let items = if name == "ToolButton" { &tools[..] } else { &HELP[..] };
                self.app.open_menu(path, items.iter().map(|s| s.to_string()).collect(), None);
                false
            }
            UiEvent::MenuSelected { anchor, name, index } if name == "ToolButton" && !self.browser.as_ref().is_some_and(|b| anchor.starts_with(b.page())) => {
                let default = options::default_preset();
                match *index {
                    0 => self.modals.show(&mut self.app, modal::Kind::Preferences),
                    // Reset: the default preset if saved, else a new document.
                    2 => {
                        let loaded = default.as_ref().filter(|p| p.is_file()).and_then(|p| (dreist_formats::profile::get().read_project)(p).map(|(raw, _)| (raw, p.clone())));
                        self.project_path = None;
                        match loaded {
                            Some((raw, p)) => self.open_document(raw, Some(dreist_formats::model::samples_dir(&p)), None),
                            None => self.open_document(project::new_document(), None, None),
                        }
                        self.outbox.push(self.model.rebuild_command());
                    }
                    3 => {
                        if let Some(p) = default {
                            if let Some(d) = p.parent() {
                                let _ = std::fs::create_dir_all(d);
                            }
                            let _ = project::save(&self.model.doc, self.model.samples.as_deref(), &p);
                        }
                    }
                    4 => {
                        if let Some(p) = default {
                            let _ = std::fs::remove_file(&p);
                            let _ = std::fs::remove_dir_all(dreist_formats::model::samples_dir(&p));
                        }
                    }
                    // [re:af38f1].
                    6 => self.filefind.show(filefind::missing_files(&self.model)),
                    // [re:c7a8f1].
                    8 => self.modals.show(&mut self.app, modal::Kind::Export),
                    _ => {}
                }
                true
            }
            UiEvent::MenuSelected { name, index, .. } if name == "HelpButton" => {
                match *index {
                    0 => self.modals.show(&mut self.app, modal::Kind::About),
                    // [re:4139f8]: the host resizes
                    // to the default window size.
                    2 => {
                        let (w, h) = self.window_size.0;
                        self.app.resize(w, h);
                        self.app.resize_request = true;
                    }
                    // Lambda #8: scale 1 (when not already), the window at
                    // the root's size; LastEditorScaleFactor 100,
                    // LastEditorSize = that size.
                    3 => {
                        if (self.app.ui_scale - 1.0).abs() > 1e-6 {
                            self.modals.options.set("LastEditorScaleFactor", 100);
                            let (w, h) = self.app.size();
                            self.modals.options.set("LastEditorSize", format!("{w},{h}"));
                            if !cfg!(test) {
                                self.modals.options.save();
                            }
                            self.window_size.1 = (w, h);
                            self.app.ui_scale = 1.0;
                            self.app.resize_request = true;
                        }
                    }
                    4 => self.midiconsole.open = true,
                    _ => {}
                }
                true
            }
            _ => false,
        }
    }
}
