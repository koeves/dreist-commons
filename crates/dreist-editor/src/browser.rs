//! Browser (`Browser.skin` with `Header.skin` and `FileList.skin`): source
//! list, path navigator and file list.
//!
//! - Source list ([re:ab066b]):
//!   the categories of [re:74962a] (Content,
//!   Places, Drives, Favourites, Searches, Shortlists), 25 px rows in
//!   `BrowserSourceListItemStyle`.
//! - File list rows in `BrowserListItemStyle`.
//!
//! Loading (port): presets replace the project (`Command::LoadProject`);
//! kits replace the selected engine's pads and audio files the selected
//! layer's sample, as document edits followed by `Command::Rebuild`.

use std::path::{Path, PathBuf};

use dreist_formats::Node;
use dreist_gui::app::{App, MouseKind, UiEvent};
use dreist_gui::paint::{Painter, RectF};
use dreist_gui::render::Renderer;
use dreist_gui::style::{HAlign, VAlign};
use dreist_gui::{Colour, Ctrl, Skin};

use crate::command::Command;
use crate::model::Model;
use crate::{EditorError, err};

pub const CATEGORIES: [&str; 6] = ["Content", "Places", "Drives", "Favourites", "Searches", "Shortlists"];
const SOURCE_ROW: f32 = 25.0;
const FILE_ROW: f32 = 18.0;
const ITEM_STYLE: &str = "BrowserListItemStyle";
const SOURCE_STYLE: &str = "BrowserSourceListItemStyle";

/// Listed files besides projects and kits of every family.
const EXTENSIONS: [&str; 13] = ["wav", "aif", "aiff", "patterntemplate", "rx2", "rex", "rcy", "rex2", "rcso", "gru", "kit", "g01", "g24"];

/// Whether the browser lists a file with extension `e`.
fn listed(e: &std::ffi::OsStr) -> bool {
    use dreist_formats::profile::{DocKind, doc_kind};
    let e = e.to_string_lossy();
    matches!(doc_kind(&e), Some(DocKind::Project | DocKind::Kit)) || EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x))
}

#[derive(Clone, Debug)]
struct Source {
    label: String,
    path: Option<PathBuf>,
    header: bool,
    /// Its category (`GetSourceListCategoryStringForCell` [re:1624e1]).
    cat: &'static str,
    /// Colour label index (0 = none).
    colour: usize,
}

#[derive(Clone, Debug)]
pub struct Entry {
    name: String,
    path: PathBuf,
    dir: bool,
}

/// Back-button places kept.
const HISTORY_KEPT: usize = 100;

pub struct Browser {
    page: Vec<usize>,
    sources: Vec<Source>,
    dir: Option<PathBuf>,
    entries: Vec<Entry>,
    selected: Option<usize>,
    /// Multiple selection (`CtrlTable` cmd / shift clicks); empty when
    /// only `selected` is.
    multi: Vec<usize>,
    /// Row to open when the press is released (second click, folders).
    pending_open: Option<usize>,
    /// The file being loaded and its progress ([re:1ae459]
    ///  [re:04782c], [re:60e1b2]
    ///  [re:12f2f4]).
    pub loading: Option<(std::path::PathBuf, f32)>,
    /// The listing is saved search results.
    saved: bool,
    /// A plain press on a row of a multiple selection: collapse to it on
    /// release unless it was dragged.
    collapse: Option<usize>,
    scroll: usize,
    history: Vec<PathBuf>,
    future: Vec<PathBuf>,
    auto_load: bool,
    /// `SliceModeButton`: audio files open the slicer instead of loading.
    slice_mode: bool,
    /// Audio file the slicer should open (taken by the editor).
    pub slice_request: Option<PathBuf>,
    /// `PreviewButton`: audio files preview when clicked.
    preview_mode: bool,
    preview_volume: f32,
    /// The listing is search results for these terms.
    search: Option<String>,
    /// Searches made this session (Searches source category).
    searches: Vec<String>,
    favourites: Vec<PathBuf>,
    /// Shortlists (name, files) and the one listed.
    shortlists: Vec<(String, Vec<PathBuf>)>,
    shortlist: Option<String>,
    /// The open context menu's selection is files only (shortlist items
    /// enabled).
    shortlist_ok: bool,
    /// Right-clicked entry with the file list menu open.
    menu_entry: Option<usize>,
    /// Actions of the open file list menu.
    menu: Vec<FileAction>,
    /// Header Options menu open ([re:5eecea] $_1): 1 the
    /// menu, 2 its Preview output submenu.
    header_menu: u8,
    /// Preview output pair (1-based, `ShowPreviewOutputMenu`).
    pub preview_output: usize,
    /// "Pads track browser location" (option `PadsTrackBrowser`).
    pub pads_track: bool,
    /// Search box menu and the advanced search pane.
    pane: crate::search::SearchPane,
    /// The open source list item menu: the item (source row) and actions.
    source_menu: Option<(usize, Vec<SourceAction>)>,
    /// Source row being renamed ("Set name...").
    renaming: Option<usize>,
    /// "Export to folder": the shortlist and whether to add the folder to
    /// the favourites, taken by the editor (folder dialog).
    pub export_request: Option<(String, bool)>,
    /// The navigator's buttons as last drawn (hit tests).
    nav: std::cell::RefCell<Vec<NavButton>>,
    /// Pressed navigator button.
    nav_pressed: Option<usize>,
    /// The open navigator menu: its folder and actions.
    nav_menu: Option<(PathBuf, Vec<NavAction>)>,
}

/// [re:2717b8] / `GloballyLockAllPatterns`
/// [re:970ae8]: lock everything unless everything is locked, then unlock.
pub fn toggle_lock_all(model: &mut Model, class: &str, count: usize) {
    let e = model.engine;
    let name = |i: usize| format!("{class}{i}");
    let all = (0..count).all(|i| {
        model.engine_node(e).and_then(|n| n.children.iter().find(|c| c.class == class && c.name.as_deref() == Some(&name(i)))).is_some_and(|n| n.attr("LockState").is_some_and(|v| v.trim() == "true"))
    });
    let eng = model.engine_mut(e);
    for i in 0..count {
        let n = eng.child_mut_or_insert(class, &name(i));
        if all {
            n.attrs.retain(|(k, _)| k != "LockState");
        } else {
            n.set("LockState", "true");
        }
    }
}

/// Whether every pad / pattern of the engine is locked (button state).
pub fn all_locked(model: &Model, class: &str, count: usize) -> bool {
    let e = model.engine;
    (0..count).all(|i| {
        model
            .engine_node(e)
            .and_then(|n| n.children.iter().find(|c| c.class == class && c.name.as_deref() == Some(&format!("{class}{i}"))))
            .is_some_and(|n| n.attr("LockState").is_some_and(|v| v.trim() == "true"))
    })
}

const HEADER_MENU: [&str; 9] = ["New shortlist", "New shortlist from selection", "", "Preview output ▸", "", "(Un)Lock all pads", "(Un)Lock all patterns", "", "Pads track browser location"];

/// File list context menu items ([re:05c1d3] $_6
/// [re:9dbfe6]).
#[derive(Clone, Copy, Debug, PartialEq)]
enum FileAction {
    SelectAll,
    SaveSearch,
    DeleteSliceInfo,
    DeleteAllSliceInfo,
    ConfirmDelete { all: bool },
    NoSlice(bool),
    Reveal,
    RevealFolder,
    ShortlistMenu,
    AddToShortlist(usize),
    AddToNewShortlist,
    RemoveFromShortlist,
    Favourite,
    FavouriteFolder,
    LoadRandom,
    None,
}

/// [re:5e37a7]: a [re:233db2] file
/// ("No slice") in a folder stops browser slicing there.
pub fn slicing_allowed(dir: &Path) -> bool {
    !dir.join(dreist_formats::legacy::NOSLICE_FILE).exists()
}

fn shortlists_path() -> Option<PathBuf> {
    crate::options::settings_dir().map(|d| d.join("Shortlists.txt"))
}

/// Shortlists file (the port's own; the original keeps `Shortlists.xml`):
/// `[name]` lines followed by file paths.
fn load_shortlists() -> Vec<(String, Vec<PathBuf>)> {
    parse_shortlists(&shortlists_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default())
}

fn parse_shortlists(text: &str) -> Vec<(String, Vec<PathBuf>)> {
    let mut v: Vec<(String, Vec<PathBuf>)> = Vec::new();
    for l in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(n) = l.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            v.push((n.to_string(), Vec::new()));
        } else if let Some(last) = v.last_mut() {
            last.1.push(PathBuf::from(l));
        }
    }
    v
}

/// Saved search results ([re:5b9e85]): one
/// `<term>[ n].searchcache` per search, the result paths one per line.
fn searches_dir() -> Option<PathBuf> {
    crate::options::settings_dir().map(|d| d.join("Searches"))
}

fn load_saved_searches() -> Vec<String> {
    let Some(d) = searches_dir() else { return Vec::new() };
    let mut v: Vec<PathBuf> = std::fs::read_dir(d).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "searchcache")).collect()).unwrap_or_default();
    v.sort();
    v.into_iter().map(|p| format!("cache:{}", p.display())).collect()
}

/// [re:c0f70d]: the first free
/// "<term>.searchcache" / "<term> <n>.searchcache".
fn save_search(term: &str, files: &[PathBuf]) -> Option<PathBuf> {
    let d = searches_dir()?;
    std::fs::create_dir_all(&d).ok()?;
    let safe: String = term.chars().map(|c| if c == '/' || c == ':' { '_' } else { c }).collect();
    let mut f = d.join(format!("{safe}.searchcache"));
    let mut n = 1;
    while f.exists() {
        f = d.join(format!("{safe} {n}.searchcache"));
        n += 1;
    }
    let text: String = files.iter().map(|p| format!("{}\n", p.display())).collect();
    dreist_formats::file::write_atomic(&f, text).ok()?;
    Some(f)
}

fn save_shortlists(v: &[(String, Vec<PathBuf>)]) {
    let Some(p) = shortlists_path() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut text = String::new();
    for (n, files) in v {
        text.push_str(&format!("[{n}]\n"));
        for f in files {
            text.push_str(&format!("{}\n", f.display()));
        }
    }
    let _ = dreist_formats::file::write_atomic(p, text);
}

/// Up to 64 random audio files under `dir` (`$_6` lambda #17 [re:64ec65],
/// "Load random sounds to pads").
pub fn random_sounds(dir: &Path, seed: u64) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in list(&d) {
            if e.dir {
                stack.push(e.path);
            } else if e.path.extension().is_some_and(|x| ["wav", "aif", "aiff"].iter().any(|a| x.eq_ignore_ascii_case(a))) {
                files.push(e.path);
            }
        }
        if files.len() > 20000 {
            break;
        }
    }
    let mut r = seed | 1;
    let mut out = Vec::new();
    while !files.is_empty() && out.len() < 64 {
        r ^= r << 13;
        r ^= r >> 7;
        r ^= r << 17;
        out.push(files.swap_remove((r % files.len() as u64) as usize));
    }
    out
}

/// Favourites file (the port's own; the original keeps
/// `<UserLocalSystem>/Favourites.xml`).
fn favourites_path() -> Option<PathBuf> {
    crate::options::settings_dir().map(|d| d.join("Favourites.txt"))
}

fn load_favourites() -> Vec<PathBuf> {
    favourites_path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| t.lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from).collect()).unwrap_or_default()
}

fn save_favourites(v: &[PathBuf]) {
    let Some(p) = favourites_path() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let text: String = v.iter().map(|p| format!("{}\n", p.display())).collect();
    let _ = dreist_formats::file::write_atomic(p, text);
}

/// Source list item names and colour labels ([re:15ef1c] /
/// `SetColour`; the original keeps them in its Favourites / Searches /
/// Shortlists XML): the port's `SourceLabels.txt`, one
/// `key<TAB>name<TAB>colour index` line per item (key = its path).
fn labels_path() -> Option<PathBuf> {
    crate::options::settings_dir().map(|d| d.join("SourceLabels.txt"))
}

fn load_labels() -> Vec<(String, String, usize)> {
    let text = labels_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            Some((it.next()?.to_string(), it.next().unwrap_or("").to_string(), it.next().and_then(|c| c.parse().ok()).unwrap_or(0)))
        })
        .collect()
}

/// An item's (name, colour index) labels.
fn label_of(labels: &[(String, String, usize)], path: &Path) -> (Option<String>, usize) {
    let key = path.to_string_lossy();
    labels.iter().find(|(k, _, _)| *k == key).map(|(_, n, c)| ((!n.is_empty()).then(|| n.clone()), *c)).unwrap_or((None, 0))
}

fn labels_get(path: &Path) -> (Option<String>, usize) {
    label_of(&load_labels(), path)
}

fn labels_set(path: &Path, name: Option<Option<&str>>, colour: Option<usize>) {
    let Some(f) = labels_path() else { return };
    let key = path.to_string_lossy().into_owned();
    let mut v = load_labels();
    let i = v.iter().position(|(k, _, _)| *k == key).unwrap_or_else(|| {
        v.push((key.clone(), String::new(), 0));
        v.len() - 1
    });
    if let Some(n) = name {
        v[i].1 = n.unwrap_or("").to_string();
    }
    if let Some(c) = colour {
        v[i].2 = c;
    }
    v.retain(|(_, n, c)| !n.is_empty() || *c != 0);
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let text: String = v.iter().map(|(k, n, c)| format!("{k}\t{n}\t{c}\n")).collect();
    let _ = dreist_formats::file::write_atomic(f, text);
}

/// Content packs removed from the source list (`RemoveItemsFromBrowser
/// ObjectArrays` [re:3b2adc] drops them from the content list; the folders
/// stay): `HiddenContent.txt`.
fn hidden_content() -> Vec<PathBuf> {
    crate::options::settings_dir().and_then(|d| std::fs::read_to_string(d.join("HiddenContent.txt")).ok()).map(|t| t.lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect()).unwrap_or_default()
}

fn hide_content(p: &Path) {
    let Some(d) = crate::options::settings_dir() else { return };
    let mut v = hidden_content();
    v.push(p.to_path_buf());
    let _ = std::fs::create_dir_all(&d);
    let text: String = v.iter().map(|p| format!("{}\n", p.display())).collect();
    let _ = dreist_formats::file::write_atomic(d.join("HiddenContent.txt"), text);
}

/// A path navigator segment button ([re:68bb96]
/// [re:ef9964] / `ConstructButton` [re:22723d]).
#[derive(Clone, Debug, PartialEq)]
pub struct NavButton {
    pub x: f32,
    pub w: f32,
    pub label: String,
    /// Display name (its tooltip).
    pub display: String,
    /// Path index it navigates to (`NavigateToPathIndex`): keeps that many
    /// + 1 components; -1 for the single non-path button.
    pub index: i32,
    pub last: bool,
}

/// Lay out the navigator for `parts` (path components, or one label for
/// search results / a shortlist when `path` is false) in `width`:
/// - `DividePath` [re:b77cf0]: each part text width + 10, the last + 20 when
///   slicing is disallowed there.
/// - Fitting (total < width − 15): every part left to right.
/// - Else a ".." button (text width; "Navigate to parent folder", index
///   first − 1) then the tail parts that fit in width − 10 − w(".."); the
///   last one's display name gains " (No slice)" when slicing is
///   disallowed (only here: the fitting layout never adds it).
pub fn nav_layout(measure: &mut dyn FnMut(&str) -> f32, parts: &[String], path: bool, no_slice: bool, width: f32) -> Vec<NavButton> {
    if !path || parts.is_empty() {
        let label = parts.first().cloned().unwrap_or_else(|| "/".into());
        let w = measure(&label) + 5.0;
        return vec![NavButton { x: 0.0, w, display: label.clone(), label, index: -1, last: true }];
    }
    let mut widths: Vec<f32> = parts.iter().map(|p| measure(p) + 10.0).collect();
    if no_slice {
        *widths.last_mut().unwrap() += 20.0;
    }
    let total: f32 = widths.iter().sum();
    let n = parts.len();
    let mut out = Vec::new();
    let mut x = 0.0;
    if total < width - 15.0 {
        for i in 0..n {
            out.push(NavButton { x, w: widths[i], label: parts[i].clone(), display: parts[i].clone(), index: i as i32, last: i + 1 == n });
            x += widths[i];
        }
        return out;
    }
    let dots = measure("..");
    let first = if n < 2 {
        n - 1
    } else {
        let mut avail = width - 10.0 - dots;
        let mut k = n;
        loop {
            avail -= widths[k - 1];
            if avail < 0.0 || k <= 2 {
                break if avail < 0.0 { k } else { k - 2 };
            }
            k -= 1;
        }
    };
    out.push(NavButton { x: 0.0, w: dots, label: "..".into(), display: "Navigate to parent folder".into(), index: first as i32 - 1, last: false });
    x = dots;
    for i in first..n {
        let last = i + 1 == n;
        let display = if last && no_slice { format!("{} (No slice)", parts[i]) } else { parts[i].clone() };
        out.push(NavButton { x, w: widths[i], label: parts[i].clone(), display, index: i as i32, last });
        x += widths[i];
    }
    out
}

/// Path navigator segment menu (`ConstructButton` $_0 [re:1edecf]).
#[derive(Clone, Copy, Debug, PartialEq)]
enum NavAction {
    DeleteSliceInfo,
    ConfirmDelete,
    NoSlice(bool),
    CopyPath,
    Reveal,
    Favourite,
    None,
}

/// Source list item menu actions ([re:4b8b2d]
/// [re:eee232]).
#[derive(Clone, Copy, Debug, PartialEq)]
enum SourceAction {
    Remove,
    ConfirmRemove,
    Reveal,
    SetName,
    ColourMenu,
    Colour(usize),
    Refresh,
    Export { favourite: bool },
    None,
}

pub const SOURCE_RENAME_EDIT: &str = "SourceNameEdit";

/// The content packs: `DREIST_CONTENT`, else `Content` in the documents
/// folder.
pub fn content_dir() -> PathBuf {
    std::env::var_os("DREIST_CONTENT").map(PathBuf::from).unwrap_or_else(|| crate::options::documents_dir().join("Content"))
}

fn subdirs(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !hidden(p)).collect()).unwrap_or_default();
    v.sort();
    v
}

fn hidden(p: &Path) -> bool {
    p.file_name().map(|n| n.to_string_lossy().starts_with('.')).unwrap_or(true)
}

fn sources(favourites: &[PathBuf], searches: &[String], shortlists: &[(String, Vec<PathBuf>)]) -> Vec<Source> {
    let mut v = Vec::new();
    let home = dreist_formats::paths::home();
    let (labels, hidden) = (load_labels(), hidden_content());
    for cat in CATEGORIES {
        v.push(Source { label: cat.into(), path: None, header: true, cat, colour: 0 });
        let items: Vec<(String, PathBuf)> = match cat {
            "Content" => subdirs(&content_dir()).into_iter().map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), p)).collect(),
            "Places" => ["", "Desktop", "Documents", "Music"].iter().map(|d| (if d.is_empty() { "Home".to_string() } else { d.to_string() }, home.join(d))).filter(|(_, p)| p.is_dir()).collect(),
            "Drives" => subdirs(Path::new("/Volumes")).into_iter().map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), p)).collect(),
            "Favourites" => favourites.iter().map(|p| (p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), p.clone())).collect(),
            // Searches carry the terms in a pseudo path.
            // (saved ones "cache:<file>").
            "Searches" => searches
                .iter()
                .map(|t| match t.strip_prefix("cache:") {
                    Some(f) => (Path::new(f).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), PathBuf::from(t)),
                    None => (t.clone(), PathBuf::from(format!("search:{t}"))),
                })
                .collect(),
            "Shortlists" => shortlists.iter().map(|(n, _)| (n.clone(), PathBuf::from(format!("shortlist:{n}")))).collect(),
            _ => Vec::new(),
        };
        for (label, path) in items {
            if cat == "Content" && hidden.contains(&path) {
                continue;
            }
            let (name, colour) = label_of(&labels, &path);
            v.push(Source { label: name.unwrap_or(label), path: Some(path), header: false, cat, colour });
        }
    }
    v
}

fn list(dir: &Path) -> Vec<Entry> {
    let mut v: Vec<Entry> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| !hidden(p))
                .filter(|p| {
                    // `.samples` folders belong to presets/kits.
                    let samples = p.extension().is_some_and(|e| e == "samples");
                    (p.is_dir() && !samples) || p.extension().is_some_and(listed)
                })
                .map(|p| Entry { name: p.file_name().unwrap().to_string_lossy().into_owned(), dir: p.is_dir(), path: p })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.dir.cmp(&a.dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    v
}

/// Make every `AudioFileName` under `n` absolute against `samples`.
pub(crate) fn absolutise(n: &mut Node, samples: &Path) {
    if let Some((_, v)) = n.attrs.iter_mut().find(|(k, _)| k == "AudioFileName") {
        let rel = v.replace('\\', "/");
        if !Path::new(&rel).is_absolute() {
            *v = dreist_formats::model::resolve_sample(samples, &rel).to_string_lossy().into_owned();
        }
    }
    for c in &mut n.children {
        absolutise(c, samples);
    }
}

/// Put `file` into the selected layer of the document.
pub fn load_sample_into_doc(model: &mut Model, file: &Path) {
    let e = model.engine;
    let (p, l) = (model.pad[e], model.layer[e]);
    load_sample_into_layer(model, e, p, l, file);
}

/// A sample's name: the file stem (a REX ".truncated" suffix ignored).
pub fn sample_stem(file: &Path) -> String {
    let (f, _) = dreist_engine::rex::strip_truncated(file);
    f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Point layer (`e`, `p`, `l`) at an audio file
/// ([re:24af02]): nothing when the pad
/// or the layer is locked; the layer keeps its parameters unless the
/// `AllocatorClearLayersOnLoad` preference is on (then a fresh layer);
/// `ResetHoldTime` sets both envelope holds from the sample's length; the
/// pad is named "P<n> <file>" when its name is automatic (matches one of
/// its layers' files) or it has no samples, and it has at most one sample.
/// Returns whether the sample was loaded.
pub fn load_sample_into_layer(model: &mut Model, e: usize, p: usize, l: usize, file: &Path) -> bool {
    let locked = |n: Option<&Node>| n.is_some_and(Node::locked);
    if locked(model.pad_node(e, p)) || locked(model.layer_node(e, p, l)) {
        return false;
    }
    // `DoLoadAudioFileOnPath` $_7 [re:c5d89e]: a REX file loads as
    // "<file>.truncated" (slices at their musical positions, see
    // `dreist_engine::rex`).
    let rex_path;
    let file = if dreist_engine::rex::is_rex(file) && !dreist_engine::rex::strip_truncated(file).1 {
        rex_path = PathBuf::from(format!("{}.truncated", file.display()));
        rex_path.as_path()
    } else {
        file
    };
    let stem = sample_stem(file);
    // Pad name rule (before the new sample counts).
    let valid: Vec<String> = (0..crate::model::LAYERS)
        .filter_map(|k| model.layer_node(e, p, k).and_then(|n| n.attr("AudioFileName")).filter(|f| !f.is_empty()).map(|f| sample_stem(Path::new(&f.replace('\\', "/")))))
        .collect();
    let pad_name = model.pad_name(e, p).unwrap_or_default();
    let auto = valid.iter().any(|f| pad_name == format!("P{} {f}", p + 1));
    let rename = (auto || valid.is_empty()) && valid.len() < 2;
    let reset = crate::options::Options::load().bool("AllocatorClearLayersOnLoad", false);
    let eng = model.doc.child_mut_or_insert("Engine", &format!("Engine{e}"));
    let pad = eng.child_mut_or_insert("Pad", &format!("Pad{p}"));
    let layer = pad.child_mut_or_insert("Layer", &format!("Layer{l}"));
    if reset {
        *layer = Node::new("Layer", Some(&format!("Layer{l}")));
    }
    layer.children.retain(|c| c.class != "AudioFileMarkers");
    layer.set("AudioFileName", file.to_string_lossy());
    layer.child_mut_or_insert("FixedString", "LayerName").set("string", &stem);
    if let Some((frames, sr)) = dreist_engine::audio::info(file) {
        let n = layer.clone();
        let get = |q: dreist_formats::params::LayerParam| n.f32(q.name()).unwrap_or(q.default_value()).clamp(0.0, 1.0);
        let bound = n.attr("PlaybackMode").map(dreist_dsp::voice::PlaybackMode::canonical) == Some("OneShotLengthBound");
        let h = dreist_engine::hold::reset_hold_time(&get, bound, frames as f64, frames as f64, sr, false);
        layer.set("AmpEnvHold", h).set("FreeEnvHold", h);
    }
    if rename {
        pad.child_mut_or_insert("FixedString", "PadName").set("string", format!("P{} {stem}", p + 1));
    }
    true
}

/// Replace the selected engine's pads with a kit's.
pub fn load_kit_into_doc(model: &mut Model, kit: &Path) -> bool {
    let before = (!crate::project::restore_outputs()).then(|| model.doc.clone());
    let ok = load_kit_inner(model, kit);
    if let (true, Some(old)) = (ok, before) {
        crate::project::keep_outputs(&old, &mut model.doc);
    }
    ok
}

fn load_kit_inner(model: &mut Model, kit: &Path) -> bool {
    let Some(mut root) = (dreist_formats::profile::get().read_document)(kit) else { return false };
    absolutise(&mut root, &dreist_formats::model::samples_dir(kit));
    let e = model.engine;
    let eng = model.doc.child_mut_or_insert("Engine", &format!("Engine{e}"));
    // [re:69fc26]: locked pads stay.
    let locked = |c: &Node| c.class == "Pad" && c.locked();
    let keep: Vec<String> = eng.children.iter().filter(|c| locked(c)).filter_map(|c| c.name.clone()).collect();
    eng.children.retain(|c| c.class != "Pad" || locked(c));
    eng.children.extend(root.children.into_iter().filter(|c| c.class == "Pad" && !c.name.as_ref().is_some_and(|n| keep.contains(n))));
    if let Some(stem) = kit.file_stem() {
        eng.set("KitName", stem.to_string_lossy());
    }
    true
}

impl Browser {
    pub fn build(skin: &mut Skin, root: &mut Ctrl, page: &[usize]) -> Result<Browser, EditorError> {
        let mut host = &mut *root;
        for &i in page {
            host = &mut host.children[i];
        }
        skin.mount(host, "Browser.skin").map_err(err)?;
        crate::search::SearchPane::mount(skin, host).map_err(err)?;
        if let Some(h) = host.find_mut("Header") {
            skin.mount(h, "Header.skin").map_err(err)?;
            // [re:9c477d]..[re:af9e3a]:
            // `SetUserEnableState(false)` on the Spinner (shown only while an
            // asynchronous search runs; the port's search is synchronous) and
            // on LockPadsButton / LockPatternsButton (never shown again; the
            // Options menu has the (Un)Lock all items).
            for n in ["Spinner", "LockPadsButton", "LockPatternsButton"] {
                if let Some(c) = h.find_mut(n) {
                    c.visible = false;
                    c.enabled = false;
                }
            }
        }
        if let Some(b) = host.find_mut("Browser") {
            skin.mount(b, "FileList.skin").map_err(err)?;
        }
        let start = subdirs(&content_dir()).into_iter().next();
        let mut b = Browser {
            page: page.to_vec(),
            sources: Vec::new(),
            dir: None,
            entries: Vec::new(),
            selected: None,
            multi: Vec::new(),
            saved: false,
            pending_open: None,
            loading: None,
            collapse: None,
            scroll: 0,
            history: Vec::new(),
            future: Vec::new(),
            auto_load: false,
            slice_mode: false,
            slice_request: None,
            preview_mode: false,
            preview_volume: 0.793_700_5,
            search: None,
            searches: Vec::new(),
            favourites: load_favourites(),
            shortlists: load_shortlists(),
            shortlist: None,
            shortlist_ok: false,
            menu_entry: None,
            menu: Vec::new(),
            header_menu: 0,
            pane: Default::default(),
            source_menu: None,
            renaming: None,
            export_request: None,
            nav: Default::default(),
            nav_pressed: None,
            nav_menu: None,
            preview_output: 1,
            pads_track: crate::options::Options::load().bool("PadsTrackBrowser", false),
        };
        b.searches = load_saved_searches();
        b.sources = sources(&b.favourites, &b.searches, &b.shortlists);
        if let Some(d) = start {
            b.go(d, false);
        }
        Ok(b)
    }

    /// Pads track browser: show `dir` unless it is already shown.
    pub fn follow(&mut self, dir: PathBuf) {
        if self.dir.as_ref() != Some(&dir) {
            self.go(dir, true);
        }
    }

    /// [re:971a71].
    pub fn previous_location(&self) -> Option<&Path> {
        self.history.last().map(PathBuf::as_path)
    }

    /// [re:ee28a0].
    pub fn next_location(&self) -> Option<&Path> {
        self.future.last().map(PathBuf::as_path)
    }

    /// Show a folder (pad menu "Open parent folder in browser").
    pub fn browse(&mut self, dir: PathBuf) {
        self.go(dir, true);
    }

    fn go(&mut self, dir: PathBuf, record: bool) {
        if record {
            if let Some(d) = self.dir.take() {
                self.history.push(d);
                // A long browsing session keeps the recent places.
                if self.history.len() > HISTORY_KEPT {
                    self.history.remove(0);
                }
            }
            self.future.clear();
        }
        if let Some(t) = dir.to_str().and_then(|s| s.strip_prefix("search:")) {
            let t = t.to_string();
            self.run_search(&t);
            return;
        }
        if let Some(f) = dir.to_str().and_then(|s| s.strip_prefix("cache:")) {
            let text = std::fs::read_to_string(f).unwrap_or_default();
            self.entries = text
                .lines()
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
                .map(|p| Entry { name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), dir: p.is_dir(), path: p })
                .collect();
            self.search = Path::new(f).file_stem().map(|s| s.to_string_lossy().into_owned());
            self.shortlist = None;
            self.saved = true;
            self.selected = None;
            self.scroll = 0;
            self.multi.clear();
            return;
        }
        self.saved = false;
        if let Some(n) = dir.to_str().and_then(|s| s.strip_prefix("shortlist:")) {
            self.show_shortlist(n.to_string());
            return;
        }
        self.shortlist = None;
        // A favourite file: open its folder with it selected.
        let (dir, select) = if dir.is_file() { (dir.parent().map(Path::to_path_buf).unwrap_or(dir.clone()), Some(dir)) } else { (dir, None) };
        self.entries = list(&dir);
        self.dir = Some(dir);
        self.search = None;
        self.selected = select.and_then(|s| self.entries.iter().position(|e| e.path == s));
        self.scroll = 0;
        self.multi.clear();
    }

    /// [re:4f921b]: the shortlist's files.
    fn show_shortlist(&mut self, name: String) {
        let files = self.shortlists.iter().find(|(n, _)| *n == name).map(|(_, f)| f.clone()).unwrap_or_default();
        self.entries = files.into_iter().map(|p| Entry { name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), dir: p.is_dir(), path: p }).collect();
        self.shortlist = Some(name);
        self.search = None;
        self.selected = None;
        self.scroll = 0;
        self.multi.clear();
    }

    /// The navigator's parts: the folder's components, else one label
    /// ("Search Results", the shortlist's name).
    fn nav_parts(&self) -> (Vec<String>, bool) {
        if self.search.is_some() {
            return (vec!["Search Results".into()], false);
        }
        if let Some(n) = &self.shortlist {
            return (vec![n.clone()], false);
        }
        match &self.dir {
            Some(d) => (
                d.components()
                    .filter_map(|c| match c {
                        std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                        _ => None,
                    })
                    .collect(),
                true,
            ),
            None => (Vec::new(), false),
        }
    }

    /// The folder button `index` navigates to (`NavigateToPathIndex`).
    fn nav_target(&self, index: i32) -> Option<PathBuf> {
        let d = self.dir.as_ref()?;
        let (parts, path) = self.nav_parts();
        if !path || index < 0 {
            return None;
        }
        let keep = (index as usize + 1).min(parts.len());
        let mut p = PathBuf::from("/");
        for c in &parts[..keep] {
            p.push(c);
        }
        let _ = d;
        Some(p)
    }

    fn nav_hit(&self, x: f32) -> Option<usize> {
        self.nav.borrow().iter().position(|b| x >= b.x && x < b.x + b.w)
    }

    /// The navigator button under `x` (tooltips).
    pub fn nav_button_at(&self, x: f32) -> Option<NavButton> {
        self.nav_hit(x).and_then(|i| self.nav.borrow().get(i).cloned())
    }

    /// `ConstructButton` $_0 [re:1edecf]: the segment's folder menu.
    fn nav_items(&self, dir: &Path) -> Vec<(String, NavAction)> {
        let allowed = slicing_allowed(dir);
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "/".into());
        vec![
            ("Delete all slice information in this folder".into(), NavAction::DeleteSliceInfo),
            (if allowed { "Disallow slicing of files in this folder" } else { "Allow slicing of files in this folder" }.into(), NavAction::NoSlice(allowed)),
            (String::new(), NavAction::None),
            ("Copy path".into(), NavAction::CopyPath),
            ("Reveal in Finder".into(), NavAction::Reveal),
            (String::new(), NavAction::None),
            (format!("Add \"{name}\" to favourites"), NavAction::Favourite),
        ]
    }

    fn nav_action(&mut self, app: &mut App, anchor: &[usize], dir: PathBuf, a: NavAction) -> bool {
        match a {
            NavAction::DeleteSliceInfo => {
                let items = vec![
                    (dreist_gui::menu::item("Remove the slice information for all the files in this folder?", false), NavAction::None),
                    (dreist_gui::menu::item("This action cannot be undone!", false), NavAction::None),
                    (String::new(), NavAction::None),
                    ("Yes".to_string(), NavAction::ConfirmDelete),
                    ("No".into(), NavAction::None),
                ];
                let (labels, actions): (Vec<String>, Vec<NavAction>) = items.into_iter().unzip();
                self.nav_menu = Some((dir, actions));
                app.open_menu(anchor, labels, None);
                false
            }
            NavAction::ConfirmDelete => {
                for e in list(&dir).into_iter().filter(|e| !e.dir) {
                    let g = e.path.with_extension("gsa");
                    if g.exists() {
                        let _ = std::fs::remove_file(g);
                    }
                }
                true
            }
            NavAction::NoSlice(disallow) => {
                let f = dir.join(dreist_formats::legacy::NOSLICE_FILE);
                let _ = if disallow { std::fs::write(f, "No slice") } else { std::fs::remove_file(f) };
                true
            }
            NavAction::CopyPath => {
                if let Ok(mut c) = std::process::Command::new("pbcopy").stdin(std::process::Stdio::piped()).spawn() {
                    use std::io::Write;
                    if let Some(i) = c.stdin.as_mut() {
                        let _ = i.write_all(dir.to_string_lossy().as_bytes());
                    }
                    let _ = c.wait();
                }
                false
            }
            NavAction::Reveal => {
                let _ = std::process::Command::new("open").arg("-R").arg(&dir).spawn();
                false
            }
            NavAction::Favourite => {
                if !self.favourites.contains(&dir) {
                    self.favourites.push(dir);
                    save_favourites(&self.favourites);
                    self.sources = sources(&self.favourites, &self.searches, &self.shortlists);
                }
                true
            }
            NavAction::None => false,
        }
    }

    /// `LaunchItemMenu` [re:eee232] for source row `i` (category items only):
    /// Content — Remove, Reveal, colour; Places / Drives — Reveal;
    /// Favourites — Remove, Reveal, Set name, colour; Searches / Shortlists
    /// — Remove, Set name, colour, then Refresh search / Export to folder
    /// (and add as favourite).
    fn source_items(&self, i: usize) -> Vec<(String, SourceAction)> {
        let Some(src) = self.sources.get(i).filter(|s| !s.header) else { return Vec::new() };
        let cat = src.cat;
        let remove = !matches!(cat, "Places" | "Drives");
        let reveal = !matches!(cat, "Searches" | "Shortlists");
        let name = !matches!(cat, "Content" | "Places" | "Drives");
        let mut v = Vec::new();
        if remove {
            v.push((format!("Remove \"{}\"", src.label), SourceAction::Remove));
        }
        if reveal {
            v.push((format!("Reveal \"{}\" in Finder", src.label), SourceAction::Reveal));
        }
        if name || remove {
            v.push((String::new(), SourceAction::None));
        }
        if name {
            v.push(("Set name...".into(), SourceAction::SetName));
        }
        if remove {
            v.push(("Set colour label... ▸".into(), SourceAction::ColourMenu));
        }
        match cat {
            "Searches" => {
                v.push((String::new(), SourceAction::None));
                v.push(("Refresh search".into(), SourceAction::Refresh));
            }
            "Shortlists" => {
                v.push((String::new(), SourceAction::None));
                v.push(("Export to folder".into(), SourceAction::Export { favourite: false }));
                v.push(("Export to folder and add as favourite".into(), SourceAction::Export { favourite: true }));
            }
            _ => {}
        }
        v
    }

    fn source_action(&mut self, app: &mut App, anchor: &[usize], i: usize, a: SourceAction) -> bool {
        let Some(src) = self.sources.get(i).cloned() else { return false };
        let Some(path) = src.path.clone() else { return false };
        let open = |me: &mut Browser, app: &mut App, items: Vec<(String, SourceAction)>| {
            let (labels, actions): (Vec<String>, Vec<SourceAction>) = items.into_iter().unzip();
            me.source_menu = Some((i, actions));
            app.open_menu(anchor, labels, None);
        };
        match a {
            // `RemoveNode` [re:95bd08] asks (Yes / No) first.
            SourceAction::Remove => {
                let q = format!("Are you sure you want to remove \"{}\". This action cannot be undone...", src.label);
                open(
                    self,
                    app,
                    vec![(dreist_gui::menu::item(q, false), SourceAction::None), (String::new(), SourceAction::None), ("Yes".into(), SourceAction::ConfirmRemove), ("No".into(), SourceAction::None)],
                );
                false
            }
            // `RemoveItemsFromBrowserObjectArrays` [re:3b2adc]: favourites
            // leave the list, saved searches and shortlists lose their
            // files, content packs leave the content list.
            SourceAction::ConfirmRemove => {
                let key = path.to_string_lossy().into_owned();
                match src.cat {
                    "Favourites" => {
                        self.favourites.retain(|f| *f != path);
                        save_favourites(&self.favourites);
                    }
                    "Searches" => {
                        if let Some(f) = key.strip_prefix("cache:") {
                            let _ = std::fs::remove_file(f);
                        }
                        let term = key.strip_prefix("search:").unwrap_or(&key).to_string();
                        self.searches.retain(|t| *t != term && *t != key);
                    }
                    "Shortlists" => {
                        let n = key.strip_prefix("shortlist:").unwrap_or(&key).to_string();
                        self.shortlists.retain(|(s, _)| *s != n);
                        save_shortlists(&self.shortlists);
                        if self.shortlist.as_deref() == Some(n.as_str()) {
                            self.shortlist = None;
                            self.entries.clear();
                        }
                    }
                    "Content" => hide_content(&path),
                    _ => return false,
                }
                labels_set(&path, Some(None), Some(0));
                self.sources = sources(&self.favourites, &self.searches, &self.shortlists);
                true
            }
            SourceAction::Reveal => {
                let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
                false
            }
            // `BeginNameEdit` [re:ecaf90]: an edit box over the row.
            SourceAction::SetName => {
                let Some(c) = app.ctrl(anchor).cloned() else { return false };
                let y = c.rect.y + (i as f32 * SOURCE_ROW) as i32 + 2;
                let rect = dreist_gui::ctrl::Rect::new(c.rect.x + 14, y, (c.rect.w - 18).max(40), SOURCE_ROW as i32 - 4);
                app.open_edit_box(&anchor[..anchor.len() - 1], SOURCE_RENAME_EDIT, rect, src.label.clone());
                self.renaming = Some(i);
                true
            }
            SourceAction::ColourMenu => {
                self.source_menu = Some((i, (0..=crate::padmenu::COLOURS.len()).map(SourceAction::Colour).collect()));
                app.open_menu(anchor, vec![crate::padmenu::colour_grid_item()], None);
                false
            }
            // `SetColourOfBrowserObject` [re:86689a] (cell 0 = none).
            SourceAction::Colour(k) => {
                labels_set(&path, None, Some(k));
                self.sources = sources(&self.favourites, &self.searches, &self.shortlists);
                true
            }
            // `RefreshSearch` [re:51bf3d]: run it again.
            SourceAction::Refresh => {
                let key = path.to_string_lossy().into_owned();
                match key.strip_prefix("search:") {
                    Some(t) => {
                        let t = t.to_string();
                        self.run_search(&t);
                    }
                    None => self.go(path, true),
                }
                true
            }
            SourceAction::Export { favourite } => {
                let key = path.to_string_lossy().into_owned();
                self.export_request = key.strip_prefix("shortlist:").map(|n| (n.to_string(), favourite));
                false
            }
            SourceAction::None => false,
        }
    }

    /// [re:af0614]: the shortlist's
    /// files copied into `<dir>/<name>`, which joins the favourites when
    /// asked.
    pub fn export_shortlist(&mut self, name: &str, dir: &Path, favourite: bool) -> std::io::Result<PathBuf> {
        let to = dir.join(name);
        std::fs::create_dir_all(&to)?;
        let files = self.shortlists.iter().find(|(n, _)| n == name).map(|(_, f)| f.clone()).unwrap_or_default();
        for f in files {
            if let Some(n) = f.file_name() {
                if f.is_dir() {
                    continue;
                }
                std::fs::copy(&f, to.join(n))?;
            }
        }
        if favourite && !self.favourites.contains(&to) {
            self.favourites.push(to.clone());
            save_favourites(&self.favourites);
            self.sources = sources(&self.favourites, &self.searches, &self.shortlists);
        }
        Ok(to)
    }

    /// The rename commit (`EndNameEdit` [re:16e4b7] →
    /// `SetNameOfBrowserObject` [re:b5847b]): shortlists take the new name,
    /// other items show it.
    fn commit_source_name(&mut self, app: &mut App, path: &[usize], text: &str) {
        let parent = path[..path.len() - 1].to_vec();
        if let Some(c) = app.ctrl_mut(&parent) {
            c.children.retain(|k| k.name != SOURCE_RENAME_EDIT);
        }
        let Some(i) = self.renaming.take() else { return };
        let Some(src) = self.sources.get(i).cloned() else { return };
        let Some(p) = src.path else { return };
        let name = text.trim();
        if name.is_empty() {
            return;
        }
        if src.cat == "Shortlists" {
            let old = p.to_string_lossy().strip_prefix("shortlist:").unwrap_or_default().to_string();
            if !self.shortlists.iter().any(|(n, _)| n == name) {
                if let Some(s) = self.shortlists.iter_mut().find(|(n, _)| *n == old) {
                    s.0 = name.to_string();
                }
                save_shortlists(&self.shortlists);
                let colour = labels_get(&p).1;
                labels_set(&p, Some(None), Some(0));
                labels_set(&PathBuf::from(format!("shortlist:{name}")), None, Some(colour));
                if self.shortlist.as_deref() == Some(old.as_str()) {
                    self.shortlist = Some(name.to_string());
                }
            }
        } else {
            labels_set(&p, Some(Some(name)), None);
        }
        self.sources = sources(&self.favourites, &self.searches, &self.shortlists);
    }

    /// [re:6fa118]: "Shortlist n".
    fn new_shortlist(&mut self, files: Vec<PathBuf>) -> usize {
        let n = (1..).find(|k| !self.shortlists.iter().any(|(s, _)| *s == format!("Shortlist {k}"))).unwrap_or(1);
        self.shortlists.push((format!("Shortlist {n}"), files));
        save_shortlists(&self.shortlists);
        self.sources = sources(&self.favourites, &self.searches, &self.shortlists);
        self.shortlists.len() - 1
    }

    /// The file list context menu for entry `i` (`DisplayFileInfo` $_6
    /// [re:9dbfe6]): with two or more rows selected the reveal / shortlist /
    /// favourite items act on the whole selection.
    fn file_menu(&mut self, i: usize) -> Vec<(String, FileAction)> {
        use dreist_gui::menu::item;
        let entry = self.entries.get(i).cloned();
        let name = entry.as_ref().map(|e| e.name.clone()).unwrap_or_default();
        let sel: Vec<Entry> = self.selection().into_iter().filter_map(|k| self.entries.get(k).cloned()).collect();
        let many = sel.len() >= 2;
        let search = self.search.is_some();
        let listing = search || self.shortlist.is_some();
        let files_only = !sel.is_empty() && sel.iter().all(|e| !e.dir);
        let folders_only = !sel.is_empty() && sel.iter().all(|e| e.dir);
        let mut v = vec![("Select all".to_string(), FileAction::SelectAll), (String::new(), FileAction::None)];
        let gsa = entry.as_ref().is_some_and(|e| e.path.with_extension("gsa").exists());
        v.push((item("Delete slice information", gsa), FileAction::DeleteSliceInfo));
        v.push(("Delete all slice information in this folder".into(), FileAction::DeleteAllSliceInfo));
        if let (Some(d), false) = (&self.dir, listing) {
            let allowed = slicing_allowed(d);
            v.push((if allowed { "Disallow slicing of files in this folder" } else { "Allow slicing of files in this folder" }.into(), FileAction::NoSlice(allowed)));
        }
        v.push((String::new(), FileAction::None));
        v.push((if many { "Reveal selected items in Finder".to_string() } else { format!("Reveal \"{name}\" in Finder") }, FileAction::Reveal));
        v.push((item("Reveal current folder in Finder", !listing), FileAction::RevealFolder));
        v.push((String::new(), FileAction::None));
        let label = if many {
            "Add selected to shortlist ▸".to_string()
        } else {
            let stem = entry.as_ref().and_then(|e| e.path.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            format!("Add {stem} to shortlist ▸")
        };
        self.shortlist_ok = files_only;
        v.push((label, FileAction::ShortlistMenu));
        if self.shortlist.is_some() {
            v.push((if many { "Remove selected items from this shortlist".to_string() } else { format!("Remove \"{name}\" from this shortlist") }, FileAction::RemoveFromShortlist));
        }
        v.push((String::new(), FileAction::None));
        let fav = if many { "Add selected items to favourites".to_string() } else { format!("Add \"{name}\" to favourites") };
        v.push((item(fav, folders_only), FileAction::Favourite));
        if !listing {
            v.push(("Add current folder to favourites".into(), FileAction::FavouriteFolder));
        } else if search && !self.saved && self.shortlist.is_none() {
            v.push(("Save search results".into(), FileAction::SaveSearch));
        }
        v.push((String::new(), FileAction::None));
        v.push(("Load random sounds to pads".into(), FileAction::LoadRandom));
        v
    }

    /// The menu's target rows: the selection.
    fn menu_targets(&self) -> Vec<Entry> {
        self.selection().into_iter().filter_map(|k| self.entries.get(k).cloned()).collect()
    }

    fn file_action(&mut self, app: &mut App, anchor: &[usize], a: FileAction, model: &mut Model, out: &mut Vec<Command>) -> bool {
        let open = |items: Vec<(String, FileAction)>, app: &mut App, me: &mut Browser| {
            let (labels, actions): (Vec<String>, Vec<FileAction>) = items.into_iter().unzip();
            me.menu = actions;
            app.open_menu(anchor, labels, None);
        };
        match a {
            FileAction::SelectAll => {
                self.multi = (0..self.entries.len()).collect();
                if self.multi.len() < 2 {
                    self.selected = self.multi.pop();
                }
                true
            }
            // `DoSaveSearch` [re:d4b4cd] → [re:edf64c]
            // [re:fa659c]: the saved search joins the Searches category.
            FileAction::SaveSearch => {
                let files: Vec<PathBuf> = self.entries.iter().map(|e| e.path.clone()).collect();
                let term = self.search.clone().unwrap_or_default();
                if let Some(f) = save_search(&term, &files) {
                    self.searches.push(format!("cache:{}", f.display()));
                    self.sources = sources(&self.favourites, &self.searches, &self.shortlists);
                }
                true
            }
            // `DoDeleteSliceAnalysis` [re:0b8b32] asks first.
            FileAction::DeleteSliceInfo | FileAction::DeleteAllSliceInfo => {
                let all = a == FileAction::DeleteAllSliceInfo;
                let q = if all { "Remove the slice information for all the files in this folder?" } else { "Remove the slice information for all the selected files?" };
                open(
                    vec![
                        (dreist_gui::menu::item(q, false), FileAction::None),
                        (dreist_gui::menu::item("This action cannot be undone!", false), FileAction::None),
                        (String::new(), FileAction::None),
                        ("Yes".into(), FileAction::ConfirmDelete { all }),
                        ("No".into(), FileAction::None),
                    ],
                    app,
                    self,
                );
                false
            }
            // `DoDeleteSliceAnalysis` [re:0b8b32]: every selected row's `.gsa`.
            FileAction::ConfirmDelete { all } => {
                let files: Vec<PathBuf> = if all { self.entries.iter().filter(|e| !e.dir).map(|e| e.path.clone()).collect() } else { self.menu_targets().into_iter().map(|e| e.path).collect() };
                for f in files {
                    let g = f.with_extension("gsa");
                    if g.exists() {
                        let _ = std::fs::remove_file(g);
                    }
                }
                true
            }
            FileAction::NoSlice(disallow) => {
                if let Some(d) = &self.dir {
                    let f = d.join(dreist_formats::legacy::NOSLICE_FILE);
                    let _ = if disallow { std::fs::write(f, "No slice") } else { std::fs::remove_file(f) };
                }
                true
            }
            FileAction::Reveal | FileAction::RevealFolder => {
                let ts: Vec<PathBuf> = if a == FileAction::Reveal { self.menu_targets().into_iter().map(|e| e.path).collect() } else { self.dir.clone().into_iter().collect() };
                if !ts.is_empty() {
                    let _ = std::process::Command::new("open").arg("-R").args(ts).spawn();
                }
                false
            }
            FileAction::ShortlistMenu => {
                // Enabled only when the selection is files only.
                let ok = self.shortlist_ok;
                let mut items: Vec<(String, FileAction)> = self.shortlists.iter().enumerate().map(|(k, (n, _))| (dreist_gui::menu::item(n.clone(), ok), FileAction::AddToShortlist(k))).collect();
                if !items.is_empty() {
                    items.push((String::new(), FileAction::None));
                }
                items.push((dreist_gui::menu::item("Add to new shortlist", ok), FileAction::AddToNewShortlist));
                open(items, app, self);
                false
            }
            FileAction::AddToShortlist(k) => {
                let paths: Vec<PathBuf> = self.menu_targets().into_iter().map(|e| e.path).collect();
                if let Some((_, files)) = self.shortlists.get_mut(k) {
                    for p in paths {
                        if !files.contains(&p) {
                            files.push(p);
                        }
                    }
                }
                save_shortlists(&self.shortlists);
                true
            }
            FileAction::AddToNewShortlist => {
                let paths: Vec<PathBuf> = self.menu_targets().into_iter().map(|e| e.path).collect();
                if paths.is_empty() {
                    return false;
                }
                self.new_shortlist(paths);
                true
            }
            FileAction::RemoveFromShortlist => {
                let Some(name) = self.shortlist.clone() else { return false };
                let gone: Vec<PathBuf> = self.menu_targets().into_iter().map(|e| e.path).collect();
                if let Some((_, files)) = self.shortlists.iter_mut().find(|(n, _)| *n == name) {
                    files.retain(|f| !gone.contains(f));
                }
                save_shortlists(&self.shortlists);
                self.show_shortlist(name);
                true
            }
            // Folders only (`IsSelectionExclusivelyFolders`); already
            // present favourites stay.
            FileAction::Favourite | FileAction::FavouriteFolder => {
                let ts: Vec<PathBuf> = if a == FileAction::Favourite { self.menu_targets().into_iter().filter(|e| e.dir).map(|e| e.path).collect() } else { self.dir.clone().into_iter().collect() };
                for t in ts {
                    if !self.favourites.contains(&t) {
                        self.favourites.push(t);
                    }
                }
                save_favourites(&self.favourites);
                self.sources = sources(&self.favourites, &self.searches, &self.shortlists);
                true
            }
            FileAction::LoadRandom => {
                let Some(d) = self.dir.clone() else { return false };
                let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.as_nanos() as u64).unwrap_or(1);
                let files = random_sounds(&d, seed);
                if files.is_empty() {
                    return false;
                }
                let e = model.engine;
                for (p, f) in files.iter().enumerate() {
                    load_sample_into_layer(model, e, p, 0, f);
                }
                out.push(model.rebuild_command());
                true
            }
            FileAction::None => false,
        }
    }

    /// Quick search ([re:111944]): one
    /// "File name Contains <text>" option over the current folder (sub
    /// folders and favourites as the search box menu sets them).
    fn run_search(&mut self, terms: &str) {
        self.entries = self.scan(&[crate::search::SearchOption::name_contains(terms)], false);
        self.search = Some(terms.to_string());
        self.saved = false;
        if !self.searches.iter().any(|s| s == terms) {
            self.searches.push(terms.to_string());
            self.sources = sources(&self.favourites, &self.searches, &self.shortlists);
        }
        self.selected = None;
        self.scroll = 0;
        self.multi.clear();
    }

    /// Search locations (`GetSearchLocations` [re:b4f753]): the last folder
    /// shown, plus the favourites when "Search within favourites" is on.
    fn scan(&self, opts: &[crate::search::SearchOption], match_all: bool) -> Vec<Entry> {
        let deep = self.pane.sub_folders;
        let mut locs = vec![(self.dir.clone().unwrap_or_else(content_dir), deep)];
        if self.pane.favourites {
            locs.extend(self.favourites.iter().filter(|f| f.is_dir()).map(|f| (f.clone(), deep)));
        }
        let mut v: Vec<Entry> =
            crate::search::scan(&locs, opts, match_all).into_iter().map(|(p, dir)| Entry { name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), path: p, dir }).collect();
        v.sort_by(|a, b| b.dir.cmp(&a.dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        v
    }

    /// Advanced search pane "Search".
    fn run_advanced(&mut self, opts: &[crate::search::SearchOption], match_all: bool) {
        self.entries = self.scan(opts, match_all);
        self.search = Some("Advanced search".into());
        self.saved = false;
        self.selected = None;
        self.scroll = 0;
        self.multi.clear();
    }

    /// Preview an audio file ([re:8e7ba7]).
    fn preview(&self, path: &Path, out: &mut Vec<Command>) {
        let mut cache = dreist_engine::audio::SampleCache::default();
        if let Some(d) = cache.get(path) {
            out.push(Command::Preview(Some(d)));
        }
    }

    #[cfg(test)]
    pub fn pane_sub_folders(&self) -> bool {
        self.pane.sub_folders
    }

    #[cfg(test)]
    pub fn test_shortlist(&mut self, files: Vec<PathBuf>) -> String {
        let k = self.new_shortlist(files);
        self.shortlists[k].0.clone()
    }

    #[cfg(test)]
    pub fn source_row(&self, label: &str) -> Option<(usize, usize)> {
        self.sources.iter().position(|s| s.label == label).map(|i| (i, self.sources[i].colour))
    }

    #[cfg(test)]
    pub fn entry_paths(&self) -> Vec<PathBuf> {
        self.entries.iter().map(|e| e.path.clone()).collect()
    }

    /// Selected rows ([re:bfc142]).
    pub fn selection(&self) -> Vec<usize> {
        if self.multi.is_empty() { self.selected.into_iter().collect() } else { self.multi.clone() }
    }

    /// [re:fb6b2b]: the selected entries'
    /// paths. A drag ends any pending collapse of the selection.
    pub fn drag_items(&self) -> Vec<PathBuf> {
        self.selection().into_iter().filter_map(|i| self.entries.get(i)).map(|e| e.path.clone()).collect()
    }

    /// The press became a drag: keep the multiple selection.
    pub fn drag_started(&mut self) {
        self.collapse = None;
        self.pending_open = None;
    }

    /// Open or close the advanced search pane.
    pub fn toggle_search_pane(&mut self, app: &mut App) {
        let open = !self.pane.is_open();
        self.pane.set_open(app, &self.page, open);
    }

    pub fn page(&self) -> &[usize] {
        &self.page
    }

    pub fn sync(&mut self, app: &mut App, model: &Model) {
        self.pane.sync(app, &self.page);
        let (lp, lt) = (all_locked(model, "Pad", 64), all_locked(model, "StepPattern", 24));
        if let Some(page) = app.ctrl_mut(&self.page) {
            if let Some(b) = page.find_mut("LockPadsButton") {
                b.on = lp;
            }
            if let Some(b) = page.find_mut("LockPatternsButton") {
                b.on = lt;
            }
        }
        let auto = self.auto_load;
        let Some(page) = app.ctrl_mut(&self.page) else { return };
        if let Some(b) = page.find_mut("AutoAllocateButton") {
            b.on = auto;
        }
        if let Some(b) = page.find_mut("SliceModeButton") {
            b.on = self.slice_mode;
        }
        if let Some(b) = page.find_mut("PreviewButton") {
            b.on = self.preview_mode;
        }
        if let Some(b) = page.find_mut("PreviewVolume") {
            b.value = self.preview_volume;
        }
    }

    fn open(&mut self, i: usize, model: &mut Model, out: &mut Vec<Command>) -> bool {
        let Some(en) = self.entries.get(i).cloned() else { return false };
        if en.dir {
            self.go(en.path, true);
            return true;
        }
        let ext = en.path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        self.loading = Some((en.path.clone(), 0.0));
        // Guru files (`DoLoadFile`): preset → a new document, kit → the
        // current engine, pattern → the current slot, bank → all slots.
        if let Some(k) = crate::guru::kind(&en.path) {
            let e = model.engine;
            let r = match k {
                crate::guru::Kind::Preset => {
                    out.push(Command::LoadProject(en.path));
                    return true;
                }
                crate::guru::Kind::Kit => crate::guru::import_kit(&mut model.doc, e, &en.path),
                crate::guru::Kind::Bank => crate::guru::import_bank(&mut model.doc, e, &en.path),
                crate::guru::Kind::Pattern => {
                    let slot = model.current_pattern(e);
                    crate::guru::import_pattern(&mut model.doc, e, slot, &en.path)
                }
            };
            if r.is_ok() {
                out.push(model.rebuild_command());
            }
            return r.is_ok();
        }
        let kind = dreist_formats::profile::doc_kind(&ext);
        match ext.as_str() {
            _ if kind == Some(dreist_formats::profile::DocKind::Project) => out.push(Command::LoadProject(en.path)),
            _ if kind == Some(dreist_formats::profile::DocKind::Kit) => {
                if load_kit_into_doc(model, &en.path) {
                    out.push(model.rebuild_command());
                }
            }
            "wav" | "aif" | "aiff" if self.slice_mode && en.path.parent().is_none_or(slicing_allowed) => self.slice_request = Some(en.path),
            "wav" | "aif" | "aiff" if self.preview_mode && !self.auto_load => self.preview(&en.path, out),
            "wav" | "aif" | "aiff" => {
                load_sample_into_doc(model, &en.path);
                out.push(model.rebuild_command());
            }
            _ => return false,
        }
        true
    }

    pub fn handle(&mut self, app: &mut App, model: &mut Model, ev: &UiEvent, out: &mut Vec<Command>) -> bool {
        if let Some(a) = self.pane.handle(app, &self.page, ev) {
            match a {
                crate::search::PaneAction::Handled => {}
                crate::search::PaneAction::Quick(t) => self.run_search(&t),
                crate::search::PaneAction::Run(o, m) => self.run_advanced(&o, m),
            }
            return true;
        }
        match ev {
            UiEvent::TextCommitted { path, name, text } if name == "SearchBox" && path.starts_with(&self.page) => {
                let t = text.trim().to_string();
                if t.is_empty() {
                    if let Some(d) = self.dir.clone() {
                        self.go(d, false);
                    }
                } else {
                    self.pane.add_recent(&t);
                    self.run_search(&t);
                }
                true
            }
            UiEvent::ValueChanged { path, name, value, .. } if name == "PreviewVolume" && path.starts_with(&self.page) => {
                self.preview_volume = *value;
                out.push(crate::command::msg(dreist_engine::binding::id::SET_PREVIEW_VOLUME, &[], &[*value]));
                true
            }
            // Path navigator buttons: a click navigates
            // (`HandleControlEvent` [re:0bd1bd] → `NavigateToPathIndex` [re:77c628]),
            // a right press opens the folder menu ($_0).
            UiEvent::Mouse { overload, kind: MouseKind::Down, x, .. } if overload == "BrowserPathNavigator" => {
                self.nav_pressed = self.nav_hit(*x);
                false
            }
            UiEvent::Mouse { overload, kind: MouseKind::Up, x, .. } if overload == "BrowserPathNavigator" => {
                let Some(i) = self.nav_pressed.take().filter(|&i| self.nav_hit(*x) == Some(i)) else { return false };
                let index = self.nav.borrow()[i].index;
                match self.nav_target(index) {
                    Some(p) => {
                        self.go(p, true);
                        true
                    }
                    None => false,
                }
            }
            UiEvent::Mouse { path, overload, kind: MouseKind::RightDown, x, .. } if overload == "BrowserPathNavigator" => {
                let Some(i) = self.nav_hit(*x) else { return false };
                let index = self.nav.borrow()[i].index;
                let Some(dir) = self.nav_target(index) else { return false };
                let (labels, actions): (Vec<String>, Vec<NavAction>) = self.nav_items(&dir).into_iter().unzip();
                self.nav_menu = Some((dir, actions));
                app.open_menu(path, labels, None);
                true
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.nav_menu.is_some() && anchor.starts_with(&self.page) => {
                let Some((dir, actions)) = self.nav_menu.take() else { return false };
                let a = actions.get(*index).copied().unwrap_or(NavAction::None);
                let anchor = anchor.clone();
                self.nav_action(app, &anchor, dir, a)
            }
            // Source list item menu ([re:4b8b2d]).
            UiEvent::Mouse { path, overload, kind: MouseKind::RightDown, y, .. } if overload == "BrowserSourceList" => {
                let i = (*y / SOURCE_ROW) as usize;
                let items = self.source_items(i);
                if items.is_empty() {
                    return false;
                }
                let (labels, actions): (Vec<String>, Vec<SourceAction>) = items.into_iter().unzip();
                self.source_menu = Some((i, actions));
                app.open_menu(path, labels, None);
                true
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.source_menu.is_some() && anchor.starts_with(&self.page) => {
                let Some((i, actions)) = self.source_menu.take() else { return false };
                let a = actions.get(*index).copied().unwrap_or(SourceAction::None);
                let anchor = anchor.clone();
                self.source_action(app, &anchor, i, a)
            }
            UiEvent::TextCommitted { path, name, text } if name == SOURCE_RENAME_EDIT => {
                let (path, text) = (path.clone(), text.clone());
                self.commit_source_name(app, &path, &text);
                true
            }
            // File list context menu (`DisplayFileInfo` $_6).
            UiEvent::Mouse { path, overload, kind: MouseKind::RightDown, y, .. } if overload == "BrowserFileList" => {
                if self.dir.is_none() && self.entries.is_empty() {
                    return false;
                }
                let i = self.scroll + (*y / FILE_ROW) as usize;
                // A row outside the selection becomes the only selection.
                if i < self.entries.len() && !self.selection().contains(&i) {
                    self.selected = Some(i);
                    self.multi.clear();
                }
                self.menu_entry = Some(i);
                let (labels, actions): (Vec<String>, Vec<FileAction>) = self.file_menu(i).into_iter().unzip();
                self.menu = actions;
                app.open_menu(path, labels, None);
                true
            }
            UiEvent::MenuSelected { anchor, index, .. } if anchor.starts_with(&self.page) && !self.menu.is_empty() => {
                let a = std::mem::take(&mut self.menu).get(*index).copied().unwrap_or(FileAction::None);
                let anchor = anchor.clone();
                let r = self.file_action(app, &anchor, a, model, out);
                if self.menu.is_empty() {
                    self.menu_entry = None;
                }
                r
            }
            UiEvent::Wheel { overload, dy, .. } if overload == "BrowserFileList" => {
                self.scroll_by(-(dy.round() as i32) * 3);
                true
            }
            UiEvent::Mouse { path, overload, kind: MouseKind::Up, x, y } if overload == "BrowserFileList" => {
                let inside = app.ctrl(path).is_some_and(|c| *x >= 0.0 && *y >= 0.0 && *x < c.rect.w as f32 && *y < c.rect.h as f32);
                if let Some(i) = self.pending_open.take().filter(|_| inside) {
                    return self.open(i, model, out);
                }
                if let Some(i) = self.collapse.take() {
                    self.multi.clear();
                    self.selected = Some(i);
                    return true;
                }
                false
            }
            UiEvent::Mouse { overload, kind: MouseKind::Down, y, .. } if overload == "BrowserSourceList" => {
                let i = (*y / SOURCE_ROW) as usize;
                if let Some(Source { path: Some(p), .. }) = self.sources.get(i).cloned() {
                    self.go(p, true);
                    return true;
                }
                false
            }
            UiEvent::Mouse { overload, kind: MouseKind::Down, y, .. } if overload == "BrowserFileList" => {
                let i = self.scroll + (*y / FILE_ROW) as usize;
                if i >= self.entries.len() {
                    return false;
                }
                // cmd toggles the row, shift extends from the anchor.
                if app.mods.cmd || app.mods.shift {
                    let mut set = self.selection();
                    if app.mods.shift {
                        let a = self.selected.unwrap_or(i);
                        set = (a.min(i)..=a.max(i)).collect();
                    } else if let Some(k) = set.iter().position(|&s| s == i) {
                        set.remove(k);
                    } else {
                        set.push(i);
                        set.sort_unstable();
                    }
                    if !app.mods.shift {
                        self.selected = if set.contains(&i) { Some(i) } else { set.first().copied() };
                    }
                    self.multi = if set.len() > 1 { set } else { Vec::new() };
                    return true;
                }
                if self.multi.len() > 1 && self.multi.contains(&i) {
                    self.collapse = Some(i);
                    return true;
                }
                self.multi.clear();
                let again = self.selected == Some(i);
                self.selected = Some(i);
                // A second click / a folder opens on release, unless the
                // press turned into a drag.
                if again || self.entries[i].dir {
                    self.pending_open = Some(i);
                } else if self.auto_load {
                    self.open(i, model, out);
                }
                true
            }
            // Header Options ([re:5eecea] $_1 [re:ea2f28]).
            UiEvent::MenuSelected { index, .. } if self.header_menu == 2 => {
                self.header_menu = 0;
                self.preview_output = (*index + 1).min(32);
                out.push(crate::command::msg(dreist_engine::binding::id::SET_PREVIEW_OUTPUT, &[], &[self.preview_output as f32]));
                true
            }
            UiEvent::MenuSelected { index, .. } if self.header_menu == 1 => {
                self.header_menu = 0;
                match *index {
                    0 => {
                        self.new_shortlist(Vec::new());
                    }
                    1 => {
                        let files = self.drag_items();
                        self.new_shortlist(files);
                    }
                    3 => {
                        // Output pairs ([re:d686a0]).
                        if let Some(p) = app.root.path_of("ToolButton").filter(|p| p.starts_with(&self.page)) {
                            let items = (1..=32).map(|k| if k == 1 { "Main".to_string() } else { format!("Sub output {} / {}", 2 * k - 3, 2 * k - 2) }).collect();
                            self.header_menu = 2;
                            app.open_menu(&p, items, Some(self.preview_output - 1));
                        }
                        return false;
                    }
                    5 => toggle_lock_all(model, "Pad", 64),
                    6 => toggle_lock_all(model, "StepPattern", 24),
                    8 => {
                        self.pads_track = !self.pads_track;
                        let mut o = crate::options::Options::load();
                        o.set("PadsTrackBrowser", self.pads_track);
                        o.save();
                    }
                    _ => {}
                }
                true
            }
            UiEvent::Clicked { path, name, .. } if path.starts_with(&self.page) => match name.as_str() {
                "ToolButton" => {
                    let mut items: Vec<String> = HEADER_MENU.iter().map(|s| s.to_string()).collect();
                    if self.pads_track {
                        items[8] = format!("✓ {}", items[8]);
                    }
                    self.header_menu = 1;
                    app.open_menu(path, items, None);
                    false
                }
                "LockPadsButton" => {
                    toggle_lock_all(model, "Pad", 64);
                    true
                }
                "LockPatternsButton" => {
                    toggle_lock_all(model, "StepPattern", 24);
                    true
                }
                "ParentButton" => {
                    if let Some(p) = self.dir.as_ref().and_then(|d| d.parent()).map(Path::to_path_buf) {
                        self.go(p, true);
                    }
                    true
                }
                "EnterButton" => self.selected.is_some_and(|i| self.open(i, model, out)),
                "RefreshButton" => {
                    if let Some(d) = self.dir.clone() {
                        self.go(d, false);
                    }
                    true
                }
                "PrevLocnButton" => {
                    if let Some(p) = self.history.pop() {
                        if let Some(d) = self.dir.take() {
                            self.future.push(d);
                        }
                        self.go(p, false);
                    }
                    true
                }
                "NextLocnButton" => {
                    if let Some(p) = self.future.pop() {
                        if let Some(d) = self.dir.take() {
                            self.history.push(d);
                        }
                        self.go(p, false);
                    }
                    true
                }
                "NextFileButton" | "PrevFileButton" => {
                    let n = self.entries.len();
                    if n > 0 {
                        let cur = self.selected.unwrap_or(0);
                        let i = if name == "NextFileButton" { (cur + 1).min(n - 1) } else { cur.saturating_sub(1) };
                        self.selected = Some(i);
                        if (self.auto_load || self.preview_mode) && !self.entries[i].dir {
                            self.open(i, model, out);
                        }
                    }
                    true
                }
                "PreviewButton" => {
                    self.preview_mode = !self.preview_mode;
                    true
                }
                "StopButton" => {
                    out.push(Command::Preview(None));
                    true
                }
                "AutoAllocateButton" => {
                    self.auto_load = !self.auto_load;
                    true
                }
                "SliceModeButton" => {
                    self.slice_mode = !self.slice_mode;
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }

    pub fn draw(&self, r: &Renderer, p: &mut Painter, c: &Ctrl) -> bool {
        let s = r.styles;
        let (w, h) = (c.rect.w as f32, c.rect.h as f32);
        match c.overload.as_str() {
            "BrowserSourceList" => {
                p.fill_rect(RectF::new(0.0, 0.0, w, h), s.colour("BrowserSourceListStyle", "colFill", Colour::WHITE));
                let font = s.font(SOURCE_STYLE);
                let text = s.colour(SOURCE_STYLE, "colText", Colour::BLACK);
                let sel = s.colour(SOURCE_STYLE, "colAltFill", Colour::BLACK);
                let mut bold = font.clone();
                bold.bold = true;
                for (i, src) in self.sources.iter().enumerate() {
                    let y = i as f32 * SOURCE_ROW;
                    if y > h {
                        break;
                    }
                    let current = src.path.is_some() && src.path == self.dir;
                    // [re:305087]: a colour label
                    // fills the row.
                    if let Some(c) = src.colour.checked_sub(1).and_then(|k| crate::padmenu::COLOURS.get(k)).and_then(|c| u32::from_str_radix(&c[1..], 16).ok()) {
                        p.fill_rect(RectF::new(0.0, y, w, SOURCE_ROW), Colour::rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, 255));
                    }
                    if current {
                        p.fill_rect(RectF::new(0.0, y, w, SOURCE_ROW), sel);
                    }
                    let col = if current { Colour::WHITE } else { text };
                    let (x, f) = if src.header { (4.0, &bold) } else { (16.0, &font) };
                    p.text(RectF::new(x, y, w - x - 4.0, SOURCE_ROW), &src.label, col, f, HAlign::Left, VAlign::Center);
                }
                true
            }
            "BrowserFileList" => {
                let font = s.font(ITEM_STYLE);
                let text = s.colour(ITEM_STYLE, "colText", Colour::BLACK);
                let sel = s.colour(ITEM_STYLE, "colAltFill", Colour::BLACK);
                let rows = (h / FILE_ROW) as usize + 1;
                for (k, en) in self.entries.iter().enumerate().skip(self.scroll).take(rows) {
                    let y = (k - self.scroll) as f32 * FILE_ROW;
                    let selected = if self.multi.is_empty() { self.selected == Some(k) } else { self.multi.contains(&k) };
                    if selected {
                        p.fill_rect(RectF::new(0.0, y, w, FILE_ROW), sel);
                    }
                    // The loading bar ([re:68977d]): colAltFill,
                    // (2, 2, (w − 4)·p, h − 2) while 0 < p ≤ 1.
                    if let Some((_, pr)) = self.loading.as_ref().filter(|(lp, pr)| *lp == en.path && *pr > 0.0 && *pr <= 1.0) {
                        p.fill_rect(RectF::new(2.0, y + 2.0, (w - 4.0) * pr, FILE_ROW - 2.0), sel.with_alpha(0.6));
                    }
                    let col = if selected { Colour::WHITE } else { text };
                    if en.dir {
                        // Folder glyph.
                        p.fill_rounded(RectF::new(4.0, y + 5.0, 12.0, 9.0), 1.5, [true; 4], col.with_alpha(0.6));
                        p.fill_rect(RectF::new(4.0, y + 3.5, 5.0, 2.0), col.with_alpha(0.6));
                    }
                    p.text(RectF::new(22.0, y, w - 26.0, FILE_ROW), &en.name, col, &font, HAlign::Left, VAlign::Center);
                }
                true
            }
            "BrowserPathNavigator" => {
                // [re:312200]: text at (2, 0,
                // w − 4, h); a chevron (w − 10, 0) → (w, h/2) → (w − 10, h)
                // after every button but the last, which gets a right edge
                // line and, for "(No slice)", a 10 point star at
                // (w − 10, h/2) (radii 0.4h / 0.15h) in the text colour.
                let st = "BrowserPathNavStyle";
                let font = s.font(st);
                let text = s.colour(st, "colText", Colour::BLACK);
                let border = s.colour(st, "colBorder", Colour::BLACK);
                let (parts, is_path) = self.nav_parts();
                let no_slice = is_path && self.dir.as_deref().is_some_and(|d| !slicing_allowed(d));
                let fonts = &mut *p.fonts;
                let buttons = nav_layout(&mut |t: &str| fonts.measure(&font, t).0, &parts, is_path, no_slice, w);
                for b in &buttons {
                    let x0 = b.x;
                    if b.last {
                        p.line(x0 + b.w + 0.5, 0.5, x0 + b.w + 0.5, h + 0.5, border, 1.0);
                        if b.display.contains("(No slice)") {
                            let (cx, cy) = (x0 + b.w - 10.0, h * 0.5);
                            let pts: Vec<(f32, f32)> = (0..10)
                                .map(|k| {
                                    let r = if k % 2 == 0 { 0.4 * h } else { 0.15 * h };
                                    let a = std::f32::consts::PI * (k as f32 / 5.0) - std::f32::consts::FRAC_PI_2;
                                    (cx + r * a.cos(), cy + r * a.sin())
                                })
                                .collect();
                            p.fill_polygon(&pts, text);
                        }
                    } else {
                        p.line(x0 + b.w - 10.0, 0.0, x0 + b.w, h * 0.5, border, 1.0);
                        p.line(x0 + b.w - 10.0, h, x0 + b.w, h * 0.5, border, 1.0);
                    }
                    p.text(RectF::new(x0 + 2.0, 0.0, (b.w - 4.0).max(0.0), h), &b.label, text, &font, HAlign::Left, VAlign::Center);
                }
                *self.nav.borrow_mut() = buttons;
                true
            }
            _ => false,
        }
    }

    /// `e_notifyFileLoadProgress` 0x143: the loading row's fraction; 1.5
    /// ends it. True when it changed.
    pub fn load_progress(&mut self, f: f32) -> bool {
        let Some((_, p)) = self.loading.as_mut() else { return false };
        if (f - 1.5).abs() <= 1e-6 {
            self.loading = None;
            return true;
        }
        let changed = (*p - f).abs() > 1e-6;
        *p = f;
        changed
    }

    /// Mouse wheel / scroll support for the file list.
    pub fn scroll_by(&mut self, rows: i32) {
        let max = self.entries.len().saturating_sub(1);
        self.scroll = (self.scroll as i32 + rows).clamp(0, max as i32) as usize;
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;

    #[test]
    fn path_navigator_layout() {
        // 6 px per character.
        let mut m = |t: &str| t.len() as f32 * 6.0;
        let parts: Vec<String> = ["Users", "me", "Kits"].iter().map(|s| s.to_string()).collect();
        let b = nav_layout(&mut m, &parts, true, false, 300.0);
        assert_eq!(b.iter().map(|b| (b.x, b.w, b.index, b.last)).collect::<Vec<_>>(), vec![(0.0, 40.0, 0, false), (40.0, 22.0, 1, false), (62.0, 34.0, 2, true)]);
        // Too narrow: ".." then the tail that fits in w − 10 − 12.
        let b = nav_layout(&mut m, &parts, true, true, 90.0);
        assert_eq!(b[0].label, "..");
        assert_eq!(b[0].index, 1);
        assert_eq!(b.last().map(|b| (b.label.as_str(), b.display.as_str())), Some(("Kits", "Kits (No slice)")));
        let b = nav_layout(&mut m, &["Search Results".into()], false, false, 300.0);
        assert_eq!((b.len(), b[0].w, b[0].index), (1, 89.0, -1));
    }

    #[test]
    fn shortlists_parse() {
        let v = parse_shortlists("[Kicks]\n/a/k1.wav\n/a/k2.wav\n\n[Empty]\n");
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].0, "Kicks");
        assert_eq!(v[0].1, vec![PathBuf::from("/a/k1.wav"), PathBuf::from("/a/k2.wav")]);
        assert!(v[1].1.is_empty());
    }

    #[test]
    fn random_sounds_are_distinct_audio_files() {
        let dir = content_dir();
        if !dir.is_dir() {
            return;
        }
        let v = random_sounds(&dir, 42);
        assert!(!v.is_empty() && v.len() <= 64);
        let set: std::collections::HashSet<_> = v.iter().collect();
        assert_eq!(set.len(), v.len());
        assert!(v.iter().all(|p| p.extension().is_some_and(|e| ["wav", "aif", "aiff"].iter().any(|x| e.eq_ignore_ascii_case(x)))));
    }

    #[test]
    fn noslice_file_blocks_slicing() {
        let d = std::env::temp_dir().join(format!("dreist-noslice-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(slicing_allowed(&d));
        std::fs::write(d.join(dreist_formats::legacy::NOSLICE_FILE), "No slice").unwrap();
        assert!(!slicing_allowed(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn lock_all_toggles_like_globally_lock() {
        let mut m = Model::new(crate::project::new_document());
        assert!(!all_locked(&m, "Pad", 64));
        toggle_lock_all(&mut m, "Pad", 64);
        assert!(all_locked(&m, "Pad", 64));
        toggle_lock_all(&mut m, "Pad", 64);
        assert!(!all_locked(&m, "Pad", 64));
        // One unlocked pad: the toggle locks all.
        toggle_lock_all(&mut m, "StepPattern", 24);
        let e = m.engine;
        m.engine_mut(e).child_mut_or_insert("StepPattern", "StepPattern3").attrs.retain(|(k, _)| k != "LockState");
        toggle_lock_all(&mut m, "StepPattern", 24);
        assert!(all_locked(&m, "StepPattern", 24));
    }
}
