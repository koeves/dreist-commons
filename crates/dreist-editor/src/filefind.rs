//! Missing files ([re:f7e56f], [re:37765e]): the audio files
//! a project refers to that could not be found while loading.
//!
//! - Auto Find ([re:513f9b]): choose a start folder (remembered
//!   as `FileFindLastLocation`), then [re:cdeafd]
//!   [re:3c8277] walks it recursively and matches files by name (a trailing
//!   ".truncated" extension is ignored); a match is loaded into every layer
//!   that asked for that file.
//! - Find Selected ([re:46ada2]): locate one file by hand; the
//!   other missing files present in the same folder are fixed too
//!   ([re:607607]).
//! - Remove Selected ([re:174ddf]): the layers drop the
//!   reference. Select All / Select None / Finished.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::render::Renderer;
use dreist_gui::style::{HAlign, VAlign};
use dreist_gui::{Colour, Ctrl, Skin};

use crate::EditorError;
use crate::model::Model;

const ROW: f32 = 18.0;
const LIST_STYLE: &str = "DefaultTableViewStyle";
const HEADER_STYLE: &str = "DefaultTableViewHeaderStyle";
/// Table header height.
const HEADER: f32 = 20.0;

/// [re:5a4467] columns: Filename (table width − 150,
/// 340..600, resizable), Engine / Pad / Layer 50 each.
fn columns(w: f32) -> [(&'static str, f32, f32); 4] {
    let fw = (w - 150.0).clamp(340.0, 600.0);
    [("Filename", 0.0, fw), ("Engine", fw, 50.0), ("Pad", fw + 50.0, 50.0), ("Layer", fw + 100.0, 50.0)]
}

/// A missing file and the layers (engine, pad, layer) that use it.
#[derive(Clone, Debug, PartialEq)]
pub struct Missing {
    pub path: PathBuf,
    pub layers: Vec<(usize, usize, usize)>,
}

/// Layers whose sample file does not exist: one request per layer, as
/// the [re:2e303e] request list the table shows.
pub fn missing_files(model: &Model) -> Vec<Missing> {
    let mut out: Vec<Missing> = Vec::new();
    for e in 0..crate::model::ENGINES {
        for p in 0..64 {
            for l in 0..8 {
                let Some(f) =
                    model.layer_sample(e, p, l).or_else(|| model.layer_node(e, p, l).and_then(|n| n.attr("AudioFileName")).filter(|s| !s.is_empty()).map(|s| PathBuf::from(s.replace('\\', "/"))))
                else {
                    continue;
                };
                // A REX ".truncated" path names the file without it.
                if f.as_os_str().is_empty() || dreist_engine::rex::strip_truncated(&f).0.exists() {
                    continue;
                }
                out.push(Missing { path: f, layers: vec![(e, p, l)] });
            }
        }
    }
    out
}

/// File name used for matching (".truncated" dropped).
fn match_name(p: &Path) -> Option<String> {
    let p = if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("truncated")) { p.with_extension("") } else { p.to_path_buf() };
    p.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// A running [re:fb2fba]: what the worker reports.
#[derive(Debug, Default)]
pub struct JobState {
    /// Progress: 0..1, each file adding its share of its folder.
    pub progress: f32,
    /// The entry being looked at (shown in ScanLabel).
    pub current: String,
    pub abort: bool,
    pub done: bool,
    /// Per wanted name, the file found for it.
    pub found: Vec<Option<PathBuf>>,
}

/// [re:8fb33a]: the folder's entries in
/// reverse listing order; each gets `share` / count of the progress —
/// files add theirs, folders pass it on to their own entries. A file whose
/// name matches pending requests satisfies all of them; the search stops
/// when none is pending or on abort.
pub fn search_location(dir: &Path, share: f32, wanted: &[String], st: &std::sync::Mutex<JobState>) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    entries.reverse();
    if entries.is_empty() {
        return false;
    }
    let part = share / entries.len() as f32;
    for p in entries {
        let Ok(mut g) = st.lock() else { return true };
        if g.abort {
            return true;
        }
        g.current = p.to_string_lossy().into_owned();
        if p.is_dir() {
            drop(g);
            if search_location(&p, part, wanted, st) {
                return true;
            }
            continue;
        }
        g.progress += part;
        if let Some(n) = p.file_name().map(|n| n.to_string_lossy().into_owned()) {
            for (i, w) in wanted.iter().enumerate() {
                if g.found[i].is_none() && *w == n {
                    g.found[i] = Some(p.clone());
                }
            }
        }
        if g.found.iter().all(Option::is_some) {
            return true;
        }
    }
    false
}

/// Panel rect animation tags (`UiEvent::Animated`).
const SCAN_IN: u32 = 0xf1f0;
const SCAN_OUT: u32 = 0xf1f1;

pub struct FileFind {
    slide: crate::modal::Slide,
    overlay: Option<Vec<usize>>,
    pub open: bool,
    pub items: Vec<Missing>,
    pub selected: BTreeSet<usize>,
    scroll: usize,
    /// The editor runs the folder / file dialogs: (auto find, manual find index).
    pub request: Option<Request>,
    /// The Auto Find job and the panel's rect before it shrank.
    job: Option<(std::sync::Arc<std::sync::Mutex<JobState>>, dreist_gui::Rect)>,
    /// TitleLabel's text from the skin.
    title: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    AutoFind,
    Locate(usize),
}

impl FileFind {
    pub fn build(skin: &mut Skin, root: &mut Ctrl) -> Result<FileFind, EditorError> {
        let overlay = crate::modal::overlay(skin, root, "FileFindPanel.skin", "FileFindOverlay")?;
        let mut title = String::new();
        if let Some(op) = &overlay {
            let mut o = &mut *root;
            for &i in op {
                o = &mut o.children[i];
            }
            if let Some(sp) = o.find_mut("ScanningPanel") {
                sp.visible = false;
            }
            if let Some(fl) = o.find_mut("FileList") {
                fl.overload = "FileFindList".into();
                fl.children.clear();
            }
            if let Some(t) = o.find("TitleLabel") {
                title = t.text.clone();
            }
        }
        Ok(FileFind { overlay, slide: Default::default(), open: false, items: Vec::new(), selected: BTreeSet::new(), scroll: 0, request: None, job: None, title })
    }

    /// [re:af38f1] with the missing files.
    pub fn show(&mut self, items: Vec<Missing>) {
        self.open = !items.is_empty();
        self.items = items;
        self.selected.clear();
        self.scroll = 0;
    }

    pub fn sync(&mut self, app: &mut App) {
        // [re:89e6b8] / closing [re:48676b]: slides from the
        // top (500 ms Exponential in, Logarithmic out).
        let open = self.overlay.clone().map(|p| self.slide.sync(app, &p, self.open)).unwrap_or(false);
        // [re:7315e8]: everything but Select All needs a
        // selection.
        let any = !self.selected.is_empty();
        if let Some(o) = self.overlay.clone().and_then(|p| app.ctrl_mut(&p)) {
            o.visible = open;
            for name in ["ManualButton", "AutoButton", "AbortButton", "RemoveButton", "SelectNone"] {
                if let Some(b) = o.find_mut(name) {
                    b.enabled = any;
                }
            }
            if let Some(b) = o.find_mut("SelectAll") {
                b.enabled = true;
            }
        }
    }

    fn mine(&self, path: &[usize]) -> bool {
        self.open && self.overlay.as_ref().is_some_and(|p| path.starts_with(p))
    }

    /// Point the layers of item `i` at `file` (the caller rebuilds).
    pub fn resolve(&mut self, model: &mut Model, i: usize, file: &Path) {
        if let Some(m) = self.items.get(i) {
            for &(e, p, l) in &m.layers {
                model.layer_mut(e, p, l).set("AudioFileName", file.to_string_lossy());
            }
        }
    }

    /// Drop resolved items.
    pub fn retain(&mut self, keep: impl Fn(usize) -> bool) {
        let items = std::mem::take(&mut self.items);
        self.items = items.into_iter().enumerate().filter(|(i, _)| keep(*i)).map(|(_, m)| m).collect();
        self.selected.clear();
        if self.items.is_empty() {
            self.open = false;
        }
    }

    fn panel_path(&self) -> Option<Vec<usize>> {
        self.overlay.as_ref().map(|o| [o.clone(), vec![0]].concat())
    }

    fn set_child(&self, app: &mut App, name: &str, f: impl FnOnce(&mut Ctrl)) {
        if let Some(c) = self.panel_path().and_then(|p| app.ctrl_mut(&p)).and_then(|c| c.find_mut(name)) {
            f(c);
        }
    }

    /// Auto Find [re:513f9b]: HoldingPanel hidden, ScanningPanel shown,
    /// ScanLabel cleared, the title "Scanning folders to find missing
    /// files...", the panel shrinks to the scanning panel's bottom (750 ms
    /// Logarithmic) and the [re:fb2fba] searches `root` on a worker.
    pub fn start_auto_find(&mut self, app: &mut App, root: &Path) {
        let wanted: Vec<String> = self.items.iter().map(|m| match_name(&m.path).unwrap_or_default()).collect();
        let st = std::sync::Arc::new(std::sync::Mutex::new(JobState { found: vec![None; wanted.len()], ..JobState::default() }));
        let worker = st.clone();
        let root = root.to_path_buf();
        let job = move || {
            search_location(&root, 1.0, &wanted, &worker);
            if let Ok(mut g) = worker.lock() {
                g.done = true;
            }
        };
        std::thread::spawn(job);
        self.set_child(app, "HoldingPanel", |c| c.visible = false);
        self.set_child(app, "ScanningPanel", |c| c.visible = true);
        self.set_child(app, "ScanLabel", |c| c.text.clear());
        self.set_child(app, "TitleLabel", |c| c.text = "Scanning folders to find missing files...".into());
        self.set_child(app, "ProgressBar", |c| c.value = 0.0);
        let Some(pp) = self.panel_path() else { return };
        let full = app.ctrl(&pp).map(|c| c.rect).unwrap_or_default();
        let bottom = app.ctrl(&pp).and_then(|c| c.find("ScanningPanel")).map(|s| s.rect.y + s.rect.h).unwrap_or(full.h);
        app.animate_rect(&pp, full, dreist_gui::Rect::new(full.x, full.y, full.w, bottom), 750.0, dreist_gui::collapse::curve, SCAN_IN);
        self.job = Some((st, full));
    }

    /// [re:02d115]: the progress bar and ScanLabel
    /// follow the job; when it ends the found files are applied (true:
    /// rebuild), the panel grows back (400 ms Linear) with the list, or
    /// closes when nothing is left.
    pub fn tick(&mut self, app: &mut App, model: &mut Model) -> bool {
        let Some((st, full)) = self.job.clone() else { return false };
        let (progress, current, done, found) = {
            let Ok(g) = st.lock() else { return false };
            (g.progress, g.current.clone(), g.done, g.found.clone())
        };
        self.set_child(app, "ProgressBar", |c| c.value = progress.clamp(0.0, 1.0));
        self.set_child(app, "ScanLabel", |c| c.text = current);
        app.dirty = true;
        if !done {
            return false;
        }
        self.job = None;
        let mut hit = BTreeSet::new();
        for (i, f) in found.iter().enumerate() {
            if let Some(f) = f {
                self.resolve(model, i, f);
                hit.insert(i);
            }
        }
        self.retain(|i| !hit.contains(&i));
        let title = self.title.clone();
        self.set_child(app, "HoldingPanel", |c| c.visible = true);
        self.set_child(app, "ScanningPanel", |c| c.visible = false);
        self.set_child(app, "TitleLabel", |c| c.text = title);
        if let Some(pp) = self.panel_path() {
            let now = app.ctrl(&pp).map(|c| c.rect).unwrap_or(full);
            app.animate_rect(&pp, now, full, 400.0, |t| t, SCAN_OUT);
        }
        !hit.is_empty()
    }

    /// Whether an Auto Find job runs.
    pub fn scanning(&self) -> bool {
        self.job.is_some()
    }

    /// Find Selected for item `i` located at `file`, plus the others whose
    /// names exist next to it.
    pub fn manual_find(&mut self, model: &mut Model, i: usize, file: &Path) {
        let dir = file.parent().map(Path::to_path_buf);
        let mut done = BTreeSet::new();
        self.resolve(model, i, file);
        done.insert(i);
        if let Some(dir) = dir {
            for k in 0..self.items.len() {
                if k == i {
                    continue;
                }
                if let Some(n) = match_name(&self.items[k].path) {
                    let cand = dir.join(&n);
                    if cand.exists() {
                        self.resolve(model, k, &cand);
                        done.insert(k);
                    }
                }
            }
        }
        self.retain(|k| !done.contains(&k));
    }

    /// Returns (changed, rebuild needed).
    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent) -> (bool, bool) {
        if let Some(p) = self.overlay.clone() {
            if self.slide.animated(app, &p, ev) {
                return (true, false);
            }
        }
        if !self.open {
            return (false, false);
        }
        match ev {
            UiEvent::Mouse { path, overload, kind: MouseKind::Down, y, .. } if overload == "FileFindList" => {
                let _ = path;
                if *y < HEADER {
                    return (true, false);
                }
                let i = self.scroll + ((*y - HEADER) / ROW) as usize;
                if i < self.items.len() {
                    if app.mods.shift || app.mods.cmd {
                        if !self.selected.insert(i) {
                            self.selected.remove(&i);
                        }
                    } else {
                        self.selected.clear();
                        self.selected.insert(i);
                    }
                }
                (true, false)
            }
            UiEvent::Wheel { overload, dy, .. } if overload == "FileFindList" => {
                self.scroll = (self.scroll as i32 - dy.signum() as i32 * 3).clamp(0, self.items.len().saturating_sub(1) as i32) as usize;
                (true, false)
            }
            UiEvent::Clicked { path, name, .. } if self.mine(path) => match name.as_str() {
                "SelectAll" => {
                    self.selected = (0..self.items.len()).collect();
                    (true, false)
                }
                "SelectNone" => {
                    self.selected.clear();
                    (true, false)
                }
                "Finished" => {
                    self.open = false;
                    (true, false)
                }
                "AutoButton" => {
                    self.request = Some(Request::AutoFind);
                    (true, false)
                }
                // [re:242e1b].
                "AbortButton" => {
                    if let Some(Ok(mut g)) = self.job.as_ref().map(|(st, _)| st.lock()) {
                        g.abort = true;
                    }
                    (true, false)
                }
                "ManualButton" => {
                    if let Some(&i) = self.selected.iter().next() {
                        self.request = Some(Request::Locate(i));
                    }
                    (true, false)
                }
                "RemoveButton" => {
                    let sel = self.selected.clone();
                    for &i in &sel {
                        if let Some(m) = self.items.get(i) {
                            for &(e, p, l) in &m.layers {
                                let n = model.layer_mut(e, p, l);
                                n.attrs.retain(|(k, _)| k != "AudioFileName");
                                n.children.retain(|c| c.class != "AudioFileMarkers");
                            }
                        }
                    }
                    self.retain(|i| !sel.contains(&i));
                    (true, true)
                }
                _ => (true, false),
            },
            // Modal.
            UiEvent::Mouse { path, .. } | UiEvent::Wheel { path, .. } | UiEvent::Clicked { path, .. } | UiEvent::ValueChanged { path, .. } => (self.mine(path), false),
            _ => (false, false),
        }
    }

    pub fn draw(&self, r: &Renderer, p: &mut Painter, c: &Ctrl) -> bool {
        if c.overload != "FileFindList" {
            return false;
        }
        let s = r.styles;
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        let font = s.font(LIST_STYLE);
        let text = s.colour(LIST_STYLE, "colText", Colour::WHITE);
        let alt = s.colour(LIST_STYLE, "colAltFill", Colour::TRANSPARENT);
        let sel = s.colour(LIST_STYLE, "colRowSelection", Colour::rgba(80, 120, 200, 255));
        p.fill_rect(RectF::new(0.0, 0.0, w, h), s.colour(LIST_STYLE, "colFill", Colour::rgba(30, 30, 30, 255)));
        let cols = columns(w);
        // Header cells.
        let hfill = s.colour(HEADER_STYLE, "colFill", Colour::WHITE);
        let htext = s.colour(HEADER_STYLE, "colText", Colour::BLACK);
        let hdiv = s.colour(HEADER_STYLE, "colAltBorder", Colour::BLACK);
        let hfont = s.font(HEADER_STYLE);
        p.fill_rect(RectF::new(0.0, 0.0, w, HEADER), hfill);
        for (title, x, cw) in cols {
            p.text(RectF::new(x + 4.0, 0.0, cw - 8.0, HEADER), title, htext, &hfont, HAlign::Left, VAlign::Center);
            p.fill_rect(RectF::new(x + cw - 1.0, 0.0, 1.0, HEADER), hdiv);
        }
        let divider = s.colour(LIST_STYLE, "colColDivider", Colour::TRANSPARENT);
        let visible = ((h - HEADER) / ROW).ceil() as usize;
        for (k, m) in self.items.iter().enumerate().skip(self.scroll).take(visible) {
            let y = HEADER + (k - self.scroll) as f32 * ROW;
            let bg = if self.selected.contains(&k) {
                sel
            } else if k % 2 == 1 {
                alt
            } else {
                Colour::TRANSPARENT
            };
            p.fill_rect(RectF::new(0.0, y, w, ROW), bg);
            // [re:28fc2e]: the file name (".truncated" dropped),
            // then engine / pad / layer counted from 1.
            let name = match_name(&m.path).unwrap_or_default();
            let (e, pad, l) = m.layers.first().copied().unwrap_or_default();
            let cells = [name, format!("{}", e + 1), format!("{}", pad + 1), format!("{}", l + 1)];
            for ((_, x, cw), t) in cols.iter().zip(cells.iter()) {
                let shown = p.fonts.shorten(&font, t, cw - 8.0, dreist_gui::style::Shorten::End);
                p.text(RectF::new(x + 4.0, y, cw - 8.0, ROW), &shown, text, &font, HAlign::Left, VAlign::Center);
            }
        }
        for (_, x, cw) in cols {
            p.fill_rect(RectF::new(x + cw - 1.0, HEADER, 1.0, h - HEADER), divider);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_by_name_and_ignores_truncated() {
        let d = std::env::temp_dir().join(format!("dreist-find-{}", std::process::id()));
        std::fs::create_dir_all(d.join("a/b")).unwrap();
        std::fs::write(d.join("a/b/kick.wav"), b"").unwrap();
        assert_eq!(match_name(Path::new("/x/kick.wav.truncated")).as_deref(), Some("kick.wav"));
        let st = std::sync::Mutex::new(JobState { found: vec![None; 2], ..JobState::default() });
        search_location(&d, 1.0, &["kick.wav".into(), "snare.wav".into()], &st);
        let g = st.lock().unwrap();
        assert_eq!(g.found[0].as_deref(), Some(d.join("a/b/kick.wav").as_path()));
        assert!(g.found[1].is_none());
        assert!((g.progress - 1.0).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&d);
    }
}
