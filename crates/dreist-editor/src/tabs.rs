//! [re:c98a8a] + [re:a4268c] pairs (`EditorTabPanel` /
//! `EditorTabButtons`, `UtilityTabPanel` / `UtilityTabButtonGroup`).
//! [re:1839fe] adds pages sized to the tab panel; the group
//! uses its buttons' `id` as the page index (`optUseChildID`).

use dreist_gui::anchors_fill;
use dreist_gui::app::{App, UiEvent};
use dreist_gui::{Ctrl, Rect};

pub struct Tabs {
    panel: Vec<usize>,
    buttons: Option<Vec<usize>>,
}

impl Tabs {
    pub fn new(root: &Ctrl, panel: &str, buttons: &str) -> Option<Tabs> {
        Some(Tabs { panel: root.path_of(panel)?, buttons: root.path_of(buttons) })
    }

    /// Add an empty page `id` and return its path.
    pub fn add_page(&self, root: &mut Ctrl, id: usize) -> Vec<usize> {
        let mut c = &mut *root;
        for &i in &self.panel {
            c = &mut c.children[i];
        }
        let mut page = Ctrl { class: "CtrlPanel".into(), style: "InvisibleStyle".into(), id: id as i32, visible: true, enabled: true, ..Ctrl::default() };
        page.rect = Rect::new(0, 0, c.rect.w, c.rect.h);
        page.anchors = anchors_fill();
        c.children.push(page);
        let mut p = self.panel.clone();
        p.push(c.children.len() - 1);
        p
    }

    /// Button index → page id (`optUseChildID`: the button's `id`, else its index).
    fn button_id(b: &Ctrl, index: usize) -> usize {
        if b.id >= 0 && b.id < 100 && (b.id != 0 || index == 0) { b.id as usize } else { index }
    }

    pub fn sync(&self, app: &mut App, selected: usize) {
        if let Some(p) = app.ctrl_mut(&self.panel) {
            for c in &mut p.children {
                c.visible = c.id as usize == selected;
            }
        }
        if let Some(bp) = &self.buttons {
            if let Some(g) = app.ctrl_mut(bp) {
                for (i, b) in g.children.iter_mut().enumerate() {
                    b.on = Self::button_id(b, i) == selected;
                }
            }
        }
    }

    /// The page id chosen by a click, if the click was on this tab group.
    pub fn clicked(&self, app: &App, ev: &UiEvent) -> Option<usize> {
        let UiEvent::Clicked { path, .. } = ev else { return None };
        let bp = self.buttons.as_ref()?;
        if path.len() != bp.len() + 1 || !path.starts_with(bp) {
            return None;
        }
        let i = *path.last()?;
        app.ctrl(path).map(|b| Self::button_id(b, i))
    }
}
