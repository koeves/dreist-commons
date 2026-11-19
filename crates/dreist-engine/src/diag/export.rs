//! The diagnostics export: one shareable text file of the system, the
//! versions, the audio setup, the run so far, the recent log lines and the
//! app's settings, with paths outside the app's own folders and the user's
//! name taken out ([`Redact`]).
//!
//! The format is line-based: `# ` section headings, `key=value` lines, and
//! the log's own lines under `# log` (see the crate README, Diagnostics).

use std::path::PathBuf;
use std::time::SystemTime;

use super::{Event, SESSION, Value, bar_beat, iso_utc, log_dir, system_info};

/// Takes paths and the user's name out of text for an export: a path in
/// one of the kept folders shows from `~`, any other becomes `<path>`, the
/// login name `<user>`.
pub struct Redact {
    home: Option<String>,
    user: Option<String>,
    keep: Vec<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).filter(|p| p.is_absolute())
}

impl Redact {
    /// Keep paths in `keep` (the app's own folders).
    pub fn new(keep: &[PathBuf]) -> Redact {
        let home = home_dir();
        let user = std::env::var(if cfg!(windows) { "USERNAME" } else { "USER" })
            .ok()
            .or_else(|| home.as_ref().and_then(|h| h.file_name()).map(|n| n.to_string_lossy().into_owned()))
            .filter(|u| u.chars().count() >= 3);
        let keep = keep.iter().map(|p| p.to_string_lossy().trim_end_matches(['/', '\\']).to_string()).filter(|p| !p.is_empty()).collect();
        Redact { home: home.map(|h| h.to_string_lossy().trim_end_matches(['/', '\\']).to_string()), user, keep }
    }

    /// For tests: a fixed home, user and kept folders.
    pub fn with(home: &str, user: &str, keep: &[&str]) -> Redact {
        Redact { home: Some(home.into()), user: Some(user.into()), keep: keep.iter().map(|s| s.to_string()).collect() }
    }

    fn looks_like_path(s: &str) -> bool {
        let b = s.as_bytes();
        s.starts_with('/') || s.starts_with("~/") || s.starts_with("\\\\") || b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
    }

    /// One path: from `~` in a kept folder, else `<path>`.
    fn path(&self, p: &str) -> String {
        let full = match (p.strip_prefix("~/"), &self.home) {
            (Some(rest), Some(h)) => format!("{h}/{rest}"),
            _ => p.to_string(),
        };
        let under = |root: &str| full == root || full.strip_prefix(root).is_some_and(|r| r.starts_with(['/', '\\']));
        if self.keep.iter().any(|k| under(k)) {
            if let Some(h) = self.home.as_deref().filter(|h| under(h)) {
                return format!("~{}", &full[h.len()..]);
            }
            return full;
        }
        "<path>".into()
    }

    /// A field value or a line: a whole path (it may hold spaces) or words,
    /// each path among them redacted; then the user's name.
    pub fn text(&self, s: &str) -> String {
        let t = s.trim();
        let out = if Self::looks_like_path(t) {
            self.path(t)
        } else {
            s.split(' ')
                .map(|w| {
                    // A path after `key=` or a quote, as in a log line.
                    let cut = w.char_indices().find(|&(i, _)| (i == 0 || w[..i].ends_with(['=', '"', '\'', '('])) && Self::looks_like_path(&w[i..])).map_or(w.len(), |(i, _)| i);
                    let (head, tail) = w.split_at(cut);
                    let (tail, end) = match tail.strip_suffix(['"', '\'', ')', ',']) {
                        Some(t) => (t, &w[w.len() - 1..]),
                        None => (tail, ""),
                    };
                    if Self::looks_like_path(tail) { format!("{head}{}{end}", self.path(tail)) } else { w.to_string() }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        match &self.user {
            Some(u) => replace_ci(&out, u, "<user>"),
            None => out,
        }
    }

    /// A log line's [`Event::body`] (`kind key=value key="a b" ...`) with
    /// each value redacted, as [`Redact::event`] would.
    pub fn body(&self, body: &str) -> String {
        let (kind, mut rest) = body.split_once(' ').unwrap_or((body, ""));
        let mut out = kind.to_string();
        while let Some((key, after)) = rest.split_once('=') {
            let (value, next) = match after.strip_prefix('"') {
                Some(q) => {
                    let end = q.find('"').unwrap_or(q.len());
                    (&q[..end], q.get(end + 1..).unwrap_or("").trim_start())
                }
                None => {
                    let end = after.find(' ').unwrap_or(after.len());
                    (&after[..end], after[end..].trim_start())
                }
            };
            let v = self.text(value);
            let v = if v.contains(' ') || v.is_empty() { format!("\"{v}\"") } else { v };
            out.push_str(&format!(" {key}={v}"));
            rest = next;
        }
        out
    }

    /// An event with its text fields redacted.
    pub fn event(&self, e: &Event) -> Event {
        Event {
            kind: e.kind.clone(),
            fields: e
                .fields
                .iter()
                .map(|(k, v)| {
                    let v = match v {
                        Value::Text(t) => Value::Text(self.text(t)),
                        v => v.clone(),
                    };
                    (k.clone(), v)
                })
                .collect(),
        }
    }
}

/// `s` with every case-insensitive `needle` replaced by `with`.
fn replace_ci(s: &str, needle: &str, with: &str) -> String {
    let (ls, ln) = (s.to_lowercase(), needle.to_lowercase());
    if ls.len() != s.len() || ln.is_empty() {
        return s.replace(needle, with);
    }
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(k) = ls[i..].find(&ln) {
        out.push_str(&s[i..i + k]);
        out.push_str(with);
        i += k + ln.len();
    }
    out.push_str(&s[i..]);
    out
}

fn line(r: &Redact, key: &str, v: &Value) -> String {
    let v = match v {
        Value::Text(t) => r.text(t),
        Value::Number(n) => format!("{n:.3}").trim_end_matches('0').trim_end_matches('.').to_string(),
        Value::Int(i) => i.to_string(),
    };
    format!("{key}={v}")
}

/// The export's text: the session's (when one runs) system, versions,
/// audio setup, summary and recent lines, then `settings`; paths outside
/// `keep` (plus the log folder) and the user's name redacted.
pub fn report(settings: &[(String, String)], keep: &[PathBuf]) -> String {
    let session = SESSION.get();
    let app = session.map_or("", |s| s.app.as_str());
    let mut keep = keep.to_vec();
    keep.extend(log_dir(app));
    let r = Redact::new(&keep);
    let mut out = vec![format!("# {} diagnostics", if app.is_empty() { "Dreist" } else { app }), "format=1".to_string(), format!("created={}", iso_utc(SystemTime::now()))];
    if let Some(s) = session {
        out.push(format!("app={}", s.app));
        out.push(format!("version={}", s.version));
    }
    out.push(String::new());
    out.push("# system".into());
    for (k, v) in &system_info().fields {
        out.push(line(&r, k, v));
    }
    let rec = session.and_then(|s| s.rec.lock().ok());
    if let Some(rec) = &rec {
        out.push(String::new());
        out.push("# audio".into());
        if let Some(a) = rec.audio() {
            out.extend(a.fields.iter().map(|(k, v)| line(&r, k, v)));
        }
        let s = rec.summary_so_far();
        out.push(String::new());
        out.push("# run".into());
        out.push(format!("seconds={:.0}", s.seconds));
        out.push(format!("late={}", s.late));
        out.push(format!("slow={}", s.slow));
        out.push(format!("late_at={}", s.late_at.join(",")));
        out.push(format!("worst_second={:.1}", s.worst_second * 100.0));
        out.push(format!("worst_second_at={}", bar_beat(s.worst_second_at)));
        out.push(format!("worst_block={:.1}", s.worst_block * 100.0));
        out.push(format!("worst_block_at={}", bar_beat(s.worst_block_at)));
        out.push(format!("peak_rss_mb={:.1}", s.peak_rss as f64 / (1 << 20) as f64));
    }
    out.push(String::new());
    out.push("# settings".into());
    for (k, v) in settings {
        out.push(format!("{k}={}", r.text(v)));
    }
    if let Some(rec) = &rec {
        out.push(String::new());
        out.push("# log".into());
        let first = rec.recent().next().map(|(t, _)| *t);
        for (t, body) in rec.recent() {
            let since = first.and_then(|f| t.duration_since(f).ok()).unwrap_or_default();
            out.push(format!("{} +{:.3}s {}", iso_utc(*t), since.as_secs_f64(), r.body(body)));
        }
    }
    out.push(String::new());
    out.join("\n")
}

/// Write [`report`] to `<log folder>/<app>-diagnostics-<UTC time>.txt` and
/// return its path.
pub fn export(settings: &[(String, String)], keep: &[PathBuf]) -> std::io::Result<PathBuf> {
    let app = SESSION.get().map_or("dreist", |s| s.app.as_str()).to_lowercase();
    let dir = log_dir(&app).ok_or_else(|| std::io::Error::other("no log folder"))?;
    // The folder's real name keeps the app's capitals.
    let dir = SESSION.get().and_then(|s| log_dir(&s.app)).unwrap_or(dir);
    std::fs::create_dir_all(&dir)?;
    let stamp: String = iso_utc(SystemTime::now()).chars().take(19).filter(|c| c.is_ascii_digit() || *c == 'T').collect();
    let path = dir.join(format!("{app}-diagnostics-{stamp}.txt"));
    std::fs::write(&path, report(settings, keep))?;
    prune_exports(&dir, &app, KEEP_EXPORTS);
    Ok(path)
}

/// Exports kept in the log folder; older ones go when a new one is written.
pub const KEEP_EXPORTS: usize = 5;

/// Remove all but the newest `keep` `<app>-diagnostics-*.txt` in `dir`
/// (their names sort by time).
fn prune_exports(dir: &std::path::Path, app: &str, keep: usize) {
    let prefix = format!("{app}-diagnostics-");
    let mut names: Vec<_> =
        std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with(&prefix) && n.ends_with(".txt")).collect();
    names.sort();
    let old = names.len().saturating_sub(keep);
    for n in &names[..old] {
        let _ = std::fs::remove_file(dir.join(n));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_outside_the_apps_folders_and_the_user_are_taken_out() {
        let r = Redact::with("/Users/kim", "kim", &["/Users/kim/Library/Application Support/Einst", "/Users/kim/Library/Logs/Einst"]);
        assert_eq!(r.text("/Users/kim/Music/My Song.einst"), "<path>");
        assert_eq!(r.text("/Users/kim/Library/Application Support/Einst/Settings.xml"), "~/Library/Application Support/Einst/Settings.xml");
        assert_eq!(r.text("~/Library/Logs/Einst/einst.log"), "~/Library/Logs/Einst/einst.log");
        assert_eq!(r.text("--play /tmp/song.geist --sample-rate 48000"), "--play <path> --sample-rate 48000");
        assert_eq!(r.text("log=\"/Users/kim/Documents/x.wav\""), "log=\"<path>\"");
        assert_eq!(r.text("C:\\Users\\kim\\x.wav"), "<path>");
        assert_eq!(r.text("Kim's AirPods"), "<user>'s AirPods");
        assert_eq!(r.text("48000"), "48000");
        // A kept folder's look-alike is not kept.
        assert_eq!(r.text("/Users/kim/Library/Logs/Einst2/x"), "<path>");
    }

    #[test]
    fn stored_lines_redact_as_their_events() {
        let r = Redact::with("/Users/kim", "kim", &["/Users/kim/Library/Logs/Einst"]);
        for e in [
            Event::new("start").with("args", "--play /Users/kim/My Song.geist --sample-rate 48000").with("cpus", 8u64).with("cpu", "Apple M3"),
            Event::new("run").with("log", "/Users/kim/Library/Logs/Einst/einst.log"),
            Event::new("audio").with("host", "Kim's Host").with("rate", 48_000.0f32).with("empty", ""),
            Event::new("stats").with("pos", "3.2").with("dsp", 31.25f64),
            Event::new("summary"),
        ] {
            assert_eq!(r.body(&e.body()), r.event(&e).body(), "{e:?}");
        }
    }

    #[test]
    fn only_the_newest_exports_stay() {
        let dir = std::env::temp_dir().join(format!("dreist-prune-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..8 {
            std::fs::write(dir.join(format!("einst-diagnostics-20261119T0800{i:02}.txt")), "x").unwrap();
        }
        std::fs::write(dir.join("einst.log"), "x").unwrap();
        prune_exports(&dir, "einst", 5);
        let mut left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(left.len(), 6, "{left:?}");
        assert!(left.contains(&"einst.log".to_string()) && left[0] == "einst-diagnostics-20261119T080003.txt", "{left:?}");
    }

    #[test]
    fn events_keep_their_numbers() {
        let r = Redact::with("/home/kim", "kim", &[]);
        let e = r.event(&Event::new("start").with("args", "--play /home/kim/a.geist").with("cpus", 8u64));
        assert_eq!(e.fields, vec![("args".to_string(), Value::from("--play <path>")), ("cpus".to_string(), Value::Int(8))]);
    }

    #[test]
    fn a_session_exports_its_log_without_the_users_paths() {
        let home = std::env::var("HOME").unwrap_or_default();
        super::super::start("Diagtest", "1.2.3", vec![("note".into(), Value::from(format!("{home}/private/song.geist")))]);
        super::super::audio(vec![("format".into(), "CLAP".into()), ("host".into(), "Some Host".into()), ("rate".into(), 48_000.0f32.into()), ("max_block".into(), 512u32.into())]);
        let p = export(&[("Content".into(), format!("{home}/Music/Samples"))], &[]).unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        if let Some(d) = log_dir("Diagtest") {
            let _ = std::fs::remove_dir_all(d);
        }
        assert!(s.starts_with("# Diagtest diagnostics\nformat=1\n") && s.contains("\nversion=1.2.3\n"), "{s}");
        assert!(s.contains("\n# audio\nformat=CLAP\nhost=\"Some Host\"\n") || s.contains("\n# audio\nformat=CLAP\nhost=Some Host\n"), "{s}");
        assert!(s.contains("\n# run\nseconds=") && s.contains("\nContent=<path>\n") && s.contains(" start os="), "{s}");
        assert!(home.len() < 2 || !s.contains(&home), "{s}");
    }

    #[test]
    fn the_report_has_its_sections() {
        let s = report(&[("Folder".into(), "/nowhere/else".into()), ("Buffer".into(), "512".into())], &[]);
        assert!(s.starts_with("# ") && s.contains("\n# system\nos=") && s.contains("\n# settings\nFolder=<path>\nBuffer=512\n"), "{s}");
    }
}
