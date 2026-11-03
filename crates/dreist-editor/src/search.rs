//! Browser search ([re:50427c].., [re:536e2c]
//! [re:f2c679]..): options, locations and the file matcher, shared by the
//! header's quick search ([re:111944]) and
//! the advanced search pane ([re:6d3139]).

use std::path::{Path, PathBuf};

/// [re:6984ca] (type names [re:e10820]).
pub const TYPES: [&str; 4] = ["File name", "Audio length", "File size", "File type"];

/// Sub-options per search type [re:3fc5fd]. Audio length lists "Less than
/// (sec)" twice and never offers "Is (sec)" (kept as the original).
pub fn sub_types(kind: usize) -> &'static [&'static str] {
    match kind {
        0 => &["Contains", "Starts with", "Ends with", "Is", "Does not contain", "Does not start with", "Does not end with", "Is not"],
        1 => &["Greater than (sec)", "Less than (sec)", "Less than (sec)"],
        2 => &["Greater than (Mb)", "Less than (Mb)", "Is (Mb)"],
        3 => &["Audio", "MIDI / Patterns", "Project Files", "Folder"],
        _ => &[],
    }
}

/// Types that take a search term [re:cee6a3]: every type but File type.
pub fn takes_term(kind: usize) -> bool {
    kind < 3
}

/// Term validity [re:ef7094]: length and size terms must be > 0; the
/// error and the replacement term.
pub fn validate(kind: usize, term: &str) -> Result<(), (&'static str, &'static str)> {
    if (kind == 1 || kind == 2) && atof(term) <= 0.0 {
        return Err(("The value entered must be greater than zero", "1.0"));
    }
    Ok(())
}

/// C `atof`: the longest leading number, else 0.
pub(crate) fn atof(s: &str) -> f32 {
    let s = s.trim_start();
    let mut end = 0;
    for (i, ch) in s.char_indices() {
        let ok = ch.is_ascii_digit() || ch == '.' || ((ch == '-' || ch == '+') && i == 0) || ((ch == 'e' || ch == 'E') && i > 0);
        if !ok {
            break;
        }
        end = i + ch.len_utf8();
    }
    (0..=end).rev().find_map(|n| crate::conv::number(&s[..n])).unwrap_or(0.0)
}

/// [re:86d350].
pub fn is_audio(ext: &str) -> bool {
    ["wav", "aif", "aiff", "rcy", "rx2", "rex", "rex2", "rcso", "sampler"].iter().any(|x| ext.eq_ignore_ascii_case(x))
}

/// [re:d5ac15].
pub fn is_pattern(ext: &str) -> bool {
    use dreist_formats::profile::{DocKind, doc_kind};
    matches!(doc_kind(ext), Some(DocKind::Pattern | DocKind::Bank)) || ["mid", "G01", "G24"].iter().any(|x| ext.eq_ignore_ascii_case(x))
}

/// [re:5333c5].
pub fn is_project(ext: &str) -> bool {
    use dreist_formats::profile::{DocKind, doc_kind};
    matches!(doc_kind(ext), Some(DocKind::Pad | DocKind::Kit | DocKind::Pattern | DocKind::Engine | DocKind::Project)) || ["G01", "G24", "KIT", "GRU"].iter().any(|x| ext.eq_ignore_ascii_case(x))
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchOption {
    pub kind: usize,
    pub sub: String,
    pub term: String,
}

impl SearchOption {
    pub fn name_contains(term: &str) -> SearchOption {
        SearchOption { kind: 0, sub: "Contains".into(), term: term.into() }
    }

    /// [re:3c5155]: an
    /// option whose type takes a term counts only with a non-blank term.
    pub fn usable(&self) -> bool {
        !takes_term(self.kind) || !self.term.trim().is_empty()
    }

    /// [re:c1dcaf].
    pub fn matches(&self, path: &Path, dir: bool) -> bool {
        let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        match self.kind {
            0 => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let (n, t) = (name.to_lowercase(), self.term.to_lowercase());
                match self.sub.as_str() {
                    "Contains" => n.contains(&t),
                    "Starts with" => n.starts_with(&t),
                    "Ends with" => n.ends_with(&t),
                    "Is" => name == self.term,
                    "Does not contain" => !n.contains(&t),
                    "Does not start with" => !n.starts_with(&t),
                    "Does not end with" => !n.ends_with(&t),
                    "Is not" => name != self.term,
                    _ => false,
                }
            }
            1 => {
                if !is_audio(&ext) {
                    return false;
                }
                let Ok(s) = dreist_engine::audio::load(path) else { return false };
                let secs = s.left.len() as f32 / s.sample_rate;
                let v = atof(&self.term);
                match self.sub.as_str() {
                    "Greater than (sec)" => v < secs,
                    "Less than (sec)" => secs < v,
                    "Is (sec)" => secs == v,
                    _ => false,
                }
            }
            2 => {
                let mb = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) as f32 * 9.536743e-07;
                let v = atof(&self.term);
                match self.sub.as_str() {
                    "Greater than (Mb)" => v < mb,
                    "Less than (Mb)" => mb < v,
                    "Is (Mb)" => mb == v,
                    _ => false,
                }
            }
            3 => match self.sub.as_str() {
                "Audio" => is_audio(&ext),
                "MIDI / Patterns" => is_pattern(&ext),
                "Project Files" => is_project(&ext),
                "Folder" => dir,
                _ => false,
            },
            _ => false,
        }
    }
}

/// [re:78b552]: any option (or all with
/// `match_all`); no options match nothing.
pub fn matches(opts: &[SearchOption], path: &Path, dir: bool, match_all: bool) -> bool {
    if opts.is_empty() {
        return false;
    }
    if match_all { opts.iter().all(|o| o.matches(path, dir)) } else { opts.iter().any(|o| o.matches(path, dir)) }
}

/// The port's hit limit.
pub const MAX_HITS: usize = 2000;

/// [re:8f998f] over each (folder, sub folders)
/// location: every non-hidden entry is tested; folders are entered first
/// when the location searches sub folders. Paths, folder flag.
pub fn scan(locations: &[(PathBuf, bool)], opts: &[SearchOption], match_all: bool) -> Vec<(PathBuf, bool)> {
    fn walk(dir: &Path, deep: bool, opts: &[SearchOption], all: bool, out: &mut Vec<(PathBuf, bool)>) {
        let mut v: Vec<PathBuf> =
            std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect()).unwrap_or_default();
        v.sort();
        for p in v {
            if out.len() >= MAX_HITS {
                return;
            }
            let is_dir = p.is_dir();
            if is_dir && deep {
                walk(&p, deep, opts, all, out);
            }
            if out.len() < MAX_HITS && matches(opts, &p, is_dir, all) {
                out.push((p, is_dir));
            }
        }
    }
    let mut out = Vec::new();
    for (loc, deep) in locations {
        walk(loc, *deep, opts, match_all, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_and_type_matching() {
        let p = Path::new("/x/Kick Hard.WAV");
        let o = |k: usize, s: &str, t: &str| SearchOption { kind: k, sub: s.into(), term: t.into() };
        assert!(o(0, "Contains", "hard").matches(p, false));
        assert!(o(0, "Starts with", "KICK").matches(p, false));
        assert!(o(0, "Ends with", ".wav").matches(p, false));
        assert!(!o(0, "Is", "kick hard.wav").matches(p, false));
        assert!(o(0, "Is not", "kick hard.wav").matches(p, false));
        assert!(o(0, "Does not contain", "snare").matches(p, false));
        assert!(o(3, "Audio", "").matches(p, false));
        assert!(!o(3, "Folder", "").matches(p, false));
        assert!(o(3, "Folder", "").matches(Path::new("/x/Kicks"), true));
        assert!(o(3, "MIDI / Patterns", "").matches(Path::new("/x/a.mid"), false));
        assert!(matches(&[o(0, "Contains", "kick"), o(0, "Contains", "snare")], p, false, false));
        assert!(!matches(&[o(0, "Contains", "kick"), o(0, "Contains", "snare")], p, false, true));
        assert!(!matches(&[], p, false, false));
        assert_eq!(validate(1, "0"), Err(("The value entered must be greater than zero", "1.0")));
        assert_eq!(validate(2, "1.5mb"), Ok(()));
        assert!(!o(0, "Contains", "  ").usable());
        assert!(o(3, "Audio", "").usable());
    }

    #[test]
    fn scans_content() {
        let root = crate::browser::content_dir();
        if !root.is_dir() {
            return;
        }
        let hits = scan(&[(root.clone(), true)], &[SearchOption::name_contains("hollow")], false);
        assert!(hits.iter().any(|(p, _)| p.file_name().is_some_and(|n| n == "Hollow.geist")));
        // Without sub folders only the top level is searched.
        let top = scan(&[(root, false)], &[SearchOption::name_contains("hollow")], false);
        assert!(top.is_empty());
        let long = SearchOption { kind: 1, sub: "Greater than (sec)".into(), term: "1".into() };
        let kicks = crate::browser::content_dir().join(dreist_formats::legacy::DEMO_FACTORY).join("Hits/Kicks");
        if kicks.is_dir() {
            let all = scan(&[(kicks.clone(), false)], &[SearchOption { kind: 3, sub: "Audio".into(), term: String::new() }], false);
            let longer = scan(&[(kicks, false)], &[long], false);
            assert!(longer.len() < all.len());
        }
    }
}

/// Height of the open pane ([re:ede612]: 0x7d).
pub const PANE_HEIGHT: i32 = 125;
const RECENT: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq)]
enum PaneMenu {
    /// [re:9f3080] with
    /// [re:841604].
    Box,
    Recent,
    Type(usize),
    Sub(usize),
    Alert,
}

/// What the browser should do after a pane event.
#[derive(Clone, Debug, PartialEq)]
pub enum PaneAction {
    /// Consumed, nothing else to do.
    Handled,
    /// Run the quick search for this text (a recent item).
    Quick(String),
    /// Run an advanced search.
    Run(Vec<SearchOption>, bool),
}

/// [re:77145a] (`SearchPane.skin`, three
/// [re:cefbe2]s from `SearchOptionItem.skin`) and the
/// search box menu state.
pub struct SearchPane {
    pub open: bool,
    /// Options `SearchSubFolders` / `SearchFavourites` (default off).
    pub sub_folders: bool,
    pub favourites: bool,
    /// [re:0f4079] recent searches (newest first, 10).
    pub recent: Vec<String>,
    rows: [(usize, usize, String); 3],
    match_all: bool,
    menu: Option<PaneMenu>,
}

impl Default for SearchPane {
    fn default() -> Self {
        let o = crate::options::Options::load();
        SearchPane {
            open: false,
            sub_folders: o.bool("SearchSubFolders", false),
            favourites: o.bool("SearchFavourites", false),
            recent: Vec::new(),
            rows: Default::default(),
            match_all: false,
            menu: None,
        }
    }
}

fn row_of(app: &dreist_gui::app::App, path: &[usize]) -> Option<usize> {
    (1..=path.len()).rev().find_map(|n| app.ctrl(&path[..n]).and_then(|c| c.name.strip_prefix("Option")).and_then(|v| v.parse::<usize>().ok())).filter(|r| (1..=3).contains(r)).map(|r| r - 1)
}

impl SearchPane {
    /// Mount the pane skins into the browser's `Search` panel (height 0
    /// until opened).
    pub fn mount(skin: &mut dreist_gui::Skin, host: &mut dreist_gui::Ctrl) -> Result<(), dreist_gui::skin::SkinError> {
        let Some(s) = host.find_mut("Search") else { return Ok(()) };
        let h = s.rect.h;
        s.rect.h = PANE_HEIGHT;
        skin.mount(s, "SearchPane.skin")?;
        for i in 1..=3 {
            if let Some(o) = s.find_mut(&format!("Option{i}")) {
                skin.mount(o, "SearchOptionItem.skin")?;
            }
        }
        s.rect.h = h;
        s.visible = false;
        Ok(())
    }

    pub fn add_recent(&mut self, text: &str) {
        self.recent.retain(|r| r != text);
        self.recent.insert(0, text.to_string());
        self.recent.truncate(RECENT);
    }

    /// The pane's options that take part ([re:b7f0e2]).
    pub fn options(&self) -> Vec<SearchOption> {
        self.rows
            .iter()
            .map(|(k, s, t)| SearchOption { kind: *k, sub: sub_types(*k).get(*s).unwrap_or(&"").to_string(), term: if takes_term(*k) { t.clone() } else { String::new() } })
            .filter(SearchOption::usable)
            .collect()
    }

    /// Open or close the pane, moving the navigator row and the split view
    /// below it ([re:1814b1]: one animation block, 750 ms,
    /// logarithmic: the pane's height to 125 / 0, the navigator, the
    /// previous / next location buttons and the split view's top to the
    /// pane's bottom, the split view's height to the rest).
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_open(&mut self, app: &mut dreist_gui::app::App, page: &[usize], open: bool) {
        if self.open == open {
            return;
        }
        self.open = open;
        let d = if open { PANE_HEIGHT } else { -PANE_HEIGHT };
        let Some(pg) = app.ctrl(page) else { return };
        let find = |n: &str| pg.path_of(n).map(|p| [page, &p[..]].concat());
        let paths: Vec<(&str, Option<Vec<usize>>)> = ["Search", "Navigator", "PrevLocnButton", "NextLocnButton", "SplitView"].iter().map(|n| (*n, find(n))).collect();
        const MS: f32 = 750.0;
        for (n, p) in paths {
            let Some(p) = p else { continue };
            let Some(c) = app.ctrl_mut(&p) else { continue };
            let from = c.rect;
            let mut to = from;
            match n {
                "Search" => {
                    to.h = if open { PANE_HEIGHT } else { 0 };
                    c.visible = true;
                }
                "SplitView" => {
                    to.y += d;
                    to.h = (to.h - d).max(0);
                    c.anchors[1].1 += d as f32;
                }
                _ => {
                    to.y += d;
                    c.anchors[1].1 += d as f32;
                }
            }
            app.animate_rect(&p, from, to, MS, dreist_gui::collapse::curve, 0);
        }
    }

    pub fn sync(&self, app: &mut dreist_gui::app::App, page: &[usize]) {
        let Some(pg) = app.ctrl_mut(page) else { return };
        let Some(s) = pg.find_mut("Search") else { return };
        // Closing, it stays until its height reaches 0.
        s.visible = self.open || s.rect.h > 0;
        for (i, (k, sub, term)) in self.rows.iter().enumerate() {
            let Some(o) = s.find_mut(&format!("Option{}", i + 1)) else { continue };
            if let Some(c) = o.find_mut("SearchType") {
                c.text = TYPES[*k].to_string();
            }
            if let Some(c) = o.find_mut("SubSearchType") {
                c.text = sub_types(*k).get(*sub).unwrap_or(&"").to_string();
            }
            if let Some(c) = o.find_mut("SearchTermBox") {
                if c.attr("editing") != Some("true") {
                    c.text = term.clone();
                }
                c.enabled = takes_term(*k);
            }
        }
        if let Some(c) = s.find_mut("MatchAllButton") {
            c.on = self.match_all;
        }
        if let Some(c) = s.find_mut("ProgressBar") {
            c.visible = false;
        }
    }

    fn save_flags(&self) {
        let mut o = crate::options::Options::load();
        o.set("SearchSubFolders", self.sub_folders);
        o.set("SearchFavourites", self.favourites);
        o.save();
    }

    pub fn handle(&mut self, app: &mut dreist_gui::app::App, page: &[usize], ev: &dreist_gui::app::UiEvent) -> Option<PaneAction> {
        use dreist_gui::app::UiEvent;
        let check = |on: bool, t: &str| format!("{}{t}", if on { "✓ " } else { "" });
        // Another control was used: a menu of ours is no longer open.
        if matches!(ev, UiEvent::Clicked { .. } | UiEvent::Pressed { .. } | UiEvent::RightClicked { .. } | UiEvent::Mouse { .. }) {
            self.menu = None;
        }
        match ev {
            UiEvent::Pressed { path, name } if name == "SearchBox" && path.starts_with(page) => {
                let items = vec![
                    check(self.sub_folders, "Search within sub folders"),
                    check(self.favourites, "Search within favourites"),
                    check(self.open, "Advanced search..."),
                    String::new(),
                    "Recent items... ▸".to_string(),
                ];
                app.open_menu(path, items, None);
                self.menu = Some(PaneMenu::Box);
                Some(PaneAction::Handled)
            }
            UiEvent::MenuSelected { anchor, index, .. } if self.menu.is_some() && anchor.starts_with(page) => {
                let menu = self.menu.take();
                match menu {
                    Some(PaneMenu::Box) => match index {
                        0 => {
                            self.sub_folders = !self.sub_folders;
                            self.save_flags();
                        }
                        1 => {
                            self.favourites = !self.favourites;
                            self.save_flags();
                        }
                        2 => {
                            let open = !self.open;
                            self.set_open(app, page, open);
                        }
                        4 if !self.recent.is_empty() => {
                            app.open_menu(anchor, self.recent.clone(), None);
                            self.menu = Some(PaneMenu::Recent);
                        }
                        _ => {}
                    },
                    Some(PaneMenu::Recent) => {
                        if let Some(t) = self.recent.get(*index).cloned() {
                            if let Some(c) = app.ctrl_mut(anchor) {
                                c.text = t.clone();
                            }
                            return Some(PaneAction::Quick(t));
                        }
                    }
                    Some(PaneMenu::Type(r)) => {
                        // Control event [re:2f7d29] → sub types updated.
                        if *index < TYPES.len() {
                            self.rows[r] = (*index, 0, if takes_term(*index) { std::mem::take(&mut self.rows[r].2) } else { String::new() });
                        }
                    }
                    Some(PaneMenu::Sub(r)) => {
                        if *index < sub_types(self.rows[r].0).len() {
                            self.rows[r].1 = *index;
                        }
                    }
                    Some(PaneMenu::Alert) | None => {}
                }
                Some(PaneAction::Handled)
            }
            UiEvent::Clicked { path, name, on, .. } if path.starts_with(page) => match name.as_str() {
                "SearchType" | "SubSearchType" => {
                    let r = row_of(app, path)?;
                    let k = self.rows[r].0;
                    let (items, cur, menu): (Vec<String>, usize, PaneMenu) = if name == "SearchType" {
                        (TYPES.iter().map(|s| s.to_string()).collect(), k, PaneMenu::Type(r))
                    } else {
                        (sub_types(k).iter().map(|s| s.to_string()).collect(), self.rows[r].1, PaneMenu::Sub(r))
                    };
                    app.open_menu(path, items, Some(cur));
                    self.menu = Some(menu);
                    Some(PaneAction::Handled)
                }
                "MatchAllButton" => {
                    self.match_all = *on;
                    Some(PaneAction::Handled)
                }
                // Control event [re:b09f4f] → search start [re:6e1b99].
                "DoSearchButton" => {
                    let opts = self.options();
                    if opts.is_empty() {
                        app.open_menu(
                            path,
                            vec!["You cannot perform a search with no options / no locations. Please make sure that you have entered at least one location and one option".into()],
                            None,
                        );
                        self.menu = Some(PaneMenu::Alert);
                        return Some(PaneAction::Handled);
                    }
                    Some(PaneAction::Run(opts, self.match_all))
                }
                _ => None,
            },
            // Term validation [re:3472b3].
            UiEvent::TextCommitted { path, name, text } if name == "SearchTermBox" && path.starts_with(page) => {
                let r = row_of(app, path)?;
                let k = self.rows[r].0;
                self.rows[r].2 = text.clone();
                if !text.trim().is_empty() {
                    if let Err((msg, fix)) = validate(k, text) {
                        self.rows[r].2 = fix.to_string();
                        app.open_menu(path, vec![msg.to_string()], None);
                        self.menu = Some(PaneMenu::Alert);
                    }
                }
                Some(PaneAction::Handled)
            }
            _ => None,
        }
    }
}
