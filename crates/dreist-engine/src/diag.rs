//! Diagnostics: what the audio thread and the editor report while a
//! standalone app runs, written as timestamped lines (text or JSON) to a
//! rotating log in the platform's log folder and, when asked, to stderr.
//!
//! - [`Monitor`]: one per process, enabled by the app ([`enable`]); the
//!   audio thread times its blocks and reports the song position with
//!   atomics only, and late blocks (a dropout before them) queue as events.
//! - [`Log`]: the log file and stderr, one line per [`Event`].
//! - [`Recorder`]: a thread that writes the start-up information, the
//!   stats of every second and the late-block events, and the summary at
//!   the end.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::load::{DspLoad, LoadReading, rss_bytes};

/// Late-block events kept until the recorder takes them.
const EVENTS: usize = 256;

/// The process's diagnostics state, shared by the audio thread, the editor
/// and the [`Recorder`].
pub struct Monitor {
    pub load: DspLoad,
    /// Song position (quarter notes, `f64` bits) and transport.
    position: AtomicU64,
    playing: AtomicBool,
    /// The editor window's share of a core (millionths, +1; 0 = unknown).
    ui: AtomicU32,
    /// A start of playback asked for (taken by the audio thread).
    play: AtomicBool,
    /// Late blocks: (nanoseconds since the monitor started, position bits),
    /// written at `head % EVENTS`.
    events: [(AtomicU64, AtomicU64); EVENTS],
    head: AtomicU64,
    started: Instant,
}

static MONITOR: OnceLock<Monitor> = OnceLock::new();

/// Turn diagnostics on for this process (idempotent).
pub fn enable() -> &'static Monitor {
    MONITOR.get_or_init(|| Monitor {
        load: DspLoad::new(),
        position: AtomicU64::new(0),
        playing: AtomicBool::new(false),
        ui: AtomicU32::new(0),
        play: AtomicBool::new(false),
        events: std::array::from_fn(|_| (AtomicU64::new(0), AtomicU64::new(0))),
        head: AtomicU64::new(0),
        started: Instant::now(),
    })
}

/// The monitor, when diagnostics are on.
pub fn monitor() -> Option<&'static Monitor> {
    MONITOR.get()
}

/// Ends a block when dropped (see [`Monitor::block`]).
pub struct BlockScope<'a> {
    monitor: &'a Monitor,
    samples: usize,
    rate: f32,
}

impl Drop for BlockScope<'_> {
    fn drop(&mut self) {
        if self.monitor.load.end(self.samples, self.rate) {
            let m = self.monitor;
            let i = m.head.fetch_add(1, Relaxed) as usize % EVENTS;
            m.events[i].0.store(m.started.elapsed().as_nanos() as u64, Relaxed);
            m.events[i].1.store(m.position.load(Relaxed), Relaxed);
        }
    }
}

/// A late block: when (since the monitor started) and where in the song.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Late {
    pub at: Duration,
    pub position: f64,
}

impl Monitor {
    /// The audio thread, as a block of `samples` at `rate` starts; the
    /// block ends when the guard drops.
    pub fn block(&self, samples: usize, rate: f32) -> BlockScope<'_> {
        self.load.begin();
        BlockScope { monitor: self, samples, rate }
    }

    /// The audio thread: the song position (quarter notes) and transport.
    pub fn set_transport(&self, position: f64, playing: bool) {
        self.position.store(position.to_bits(), Relaxed);
        self.playing.store(playing, Relaxed);
    }

    pub fn position(&self) -> f64 {
        f64::from_bits(self.position.load(Relaxed))
    }

    pub fn playing(&self) -> bool {
        self.playing.load(Relaxed)
    }

    /// The editor: its window's share of a core.
    pub fn set_ui_load(&self, share: f32) {
        self.ui.store((share.max(0.0) * 1e6) as u32 + 1, Relaxed);
    }

    pub fn ui_load(&self) -> Option<f32> {
        match self.ui.load(Relaxed) {
            0 => None,
            v => Some((v - 1) as f32 / 1e6),
        }
    }

    /// Ask the audio thread to start playback.
    pub fn request_play(&self) {
        self.play.store(true, Relaxed);
    }

    /// The audio thread: a start of playback was asked for.
    pub fn take_play(&self) -> bool {
        self.play.swap(false, Relaxed)
    }

    /// Late blocks from number `from` on (the next `from` is returned);
    /// older ones than the queue holds are skipped.
    pub fn late_since(&self, from: u64) -> (Vec<Late>, u64) {
        let head = self.head.load(Relaxed);
        let first = from.max(head.saturating_sub(EVENTS as u64));
        let out = (first..head)
            .map(|n| {
                let e = &self.events[n as usize % EVENTS];
                Late { at: Duration::from_nanos(e.0.load(Relaxed)), position: f64::from_bits(e.1.load(Relaxed)) }
            })
            .collect();
        (out, head)
    }
}

/// A song position (quarter notes) as bar.beat (4/4, from 1.1).
pub fn bar_beat(qn: f64) -> String {
    let qn = qn.max(0.0);
    format!("{}.{}", (qn / 4.0).floor() as u64 + 1, (qn % 4.0).floor() as u64 + 1)
}

/// A field value of an [`Event`].
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Text(String),
    Number(f64),
    Int(i64),
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::Text(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Value {
        Value::Text(s)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Value {
        Value::Number(v)
    }
}

impl From<f32> for Value {
    fn from(v: f32) -> Value {
        Value::Number(v as f64)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Value {
        Value::Int(v)
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Value {
        Value::Int(v as i64)
    }
}

impl From<u32> for Value {
    fn from(v: u32) -> Value {
        Value::Int(v as i64)
    }
}

/// One log line: a kind (`start`, `stats`, `late`, `summary`, ...) and its
/// fields in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub kind: String,
    pub fields: Vec<(String, Value)>,
}

impl Event {
    pub fn new(kind: &str) -> Event {
        Event { kind: kind.to_string(), fields: Vec::new() }
    }

    pub fn with(mut self, key: &str, v: impl Into<Value>) -> Event {
        self.fields.push((key.to_string(), v.into()));
        self
    }

    /// `2026-11-15T16:20:01.123Z +12.345s stats dsp=31.2 ...`
    pub fn text(&self, wall: SystemTime, elapsed: Duration) -> String {
        let mut s = format!("{} +{:.3}s {}", iso_utc(wall), elapsed.as_secs_f64(), self.kind);
        for (k, v) in &self.fields {
            let v = match v {
                Value::Text(t) if t.contains(' ') || t.is_empty() => format!("\"{}\"", t.replace('"', "'")),
                Value::Text(t) => t.clone(),
                Value::Number(n) => format!("{n:.3}").trim_end_matches('0').trim_end_matches('.').to_string(),
                Value::Int(i) => i.to_string(),
            };
            s.push_str(&format!(" {k}={v}"));
        }
        s
    }

    /// One JSON object: `t`, `elapsed`, `kind` and the fields.
    pub fn json(&self, wall: SystemTime, elapsed: Duration) -> String {
        let mut s = format!("{{\"t\":\"{}\",\"elapsed\":{:.3},\"kind\":{}", iso_utc(wall), elapsed.as_secs_f64(), json_str(&self.kind));
        for (k, v) in &self.fields {
            let v = match v {
                Value::Text(t) => json_str(t),
                Value::Number(n) if n.is_finite() => format!("{n}"),
                Value::Number(_) => "null".to_string(),
                Value::Int(i) => i.to_string(),
            };
            s.push_str(&format!(",{}:{v}", json_str(k)));
        }
        s.push('}');
        s
    }
}

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// UTC wall time as ISO 8601 with milliseconds.
pub fn iso_utc(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z", rem / 3600, rem / 60 % 60, rem % 60, d.subsec_millis())
}

/// The platform's folder for `app`'s logs: `~/Library/Logs/<App>` on
/// macOS, `$XDG_STATE_HOME/<App>` (or `~/.local/state/<App>`) on Linux,
/// `%LOCALAPPDATA%\<App>\Logs` on Windows.
pub fn log_dir(app: &str) -> Option<PathBuf> {
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        Some(home()?.join("Library/Logs").join(app))
    } else if cfg!(windows) {
        Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join(app).join("Logs"))
    } else {
        let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| Some(home()?.join(".local/state")))?;
        Some(state.join(app))
    }
}

/// Log size before it rotates, and the rotated files kept.
const ROTATE_BYTES: u64 = 5 << 20;
const KEEP: usize = 3;

/// Where events go: a log file (rotated by size) and stderr, as text or
/// JSON lines.
pub struct Log {
    file: Option<(PathBuf, File, u64)>,
    stderr: bool,
    json: bool,
    started: Instant,
}

impl Log {
    /// Events to `path` (appended; `None`: no file) and to stderr when
    /// `stderr`.
    pub fn open(path: Option<&Path>, stderr: bool, json: bool) -> Log {
        let file = path.and_then(|p| {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let f = std::fs::OpenOptions::new().create(true).append(true).open(p).ok()?;
            let len = f.metadata().map(|m| m.len()).unwrap_or(0);
            Some((p.to_path_buf(), f, len))
        });
        Log { file, stderr, json, started: Instant::now() }
    }

    /// The log file, when one is open.
    pub fn path(&self) -> Option<&Path> {
        self.file.as_ref().map(|f| f.0.as_path())
    }

    pub fn write(&mut self, e: &Event) {
        let line = if self.json { e.json(SystemTime::now(), self.started.elapsed()) } else { e.text(SystemTime::now(), self.started.elapsed()) };
        if self.stderr {
            eprintln!("{line}");
        }
        if let Some((path, f, len)) = self.file.as_mut() {
            if writeln!(f, "{line}").is_ok() {
                *len += line.len() as u64 + 1;
            }
            if *len > ROTATE_BYTES {
                let path = path.clone();
                rotate(&path);
                if let Ok(nf) = File::create(&path) {
                    self.file = Some((path, nf, 0));
                }
            }
        }
    }
}

/// `log` → `log.1` → ... → `log.<KEEP>` (the oldest dropped).
fn rotate(path: &Path) {
    let n = |k: usize| PathBuf::from(format!("{}.{k}", path.display()));
    let _ = std::fs::remove_file(n(KEEP));
    for k in (1..KEEP).rev() {
        let _ = std::fs::rename(n(k), n(k + 1));
    }
    let _ = std::fs::rename(path, n(1));
}

/// What the start-up line says about the system, beside the app's own
/// fields.
pub fn system_info() -> Event {
    let cpus = std::thread::available_parallelism().map(|n| n.get() as u64).unwrap_or(0);
    let mut e = Event::new("start").with("os", std::env::consts::OS).with("arch", std::env::consts::ARCH).with("cpus", cpus);
    if let Ok(k) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        e = e.with("kernel", k.trim());
    }
    if let Some(model) = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|s| s.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|m| m.trim().to_string())) {
        e = e.with("cpu", model);
    }
    if let Ok(g) = std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor") {
        e = e.with("governor", g.trim());
    }
    e
}

/// What a run came to (see [`Recorder::summary`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub seconds: f64,
    pub late: u64,
    /// Where the first late blocks fell (bar.beat).
    pub late_at: Vec<String>,
    pub worst_second: f32,
    pub worst_second_at: f64,
    pub worst_block: f32,
    pub worst_block_at: f64,
    pub peak_rss: u64,
}

/// Writes the monitor's stats once a second and its late blocks as they
/// come, until [`Recorder::summary`].
pub struct Recorder {
    log: Log,
    monitor: &'static Monitor,
    seen_second: u32,
    late_from: u64,
    summary: Summary,
    started: Instant,
}

impl Recorder {
    pub fn new(log: Log, monitor: &'static Monitor) -> Recorder {
        Recorder { log, monitor, seen_second: 0, late_from: 0, summary: Summary::default(), started: Instant::now() }
    }

    pub fn log(&mut self) -> &mut Log {
        &mut self.log
    }

    /// Write what is new (call about every 100 ms).
    pub fn poll(&mut self) {
        let m = self.monitor;
        let (late, next) = m.late_since(self.late_from);
        self.late_from = next;
        for l in late {
            self.summary.late += 1;
            if self.summary.late_at.len() < 32 {
                self.summary.late_at.push(bar_beat(l.position));
            }
            let e = Event::new("late").with("pos", bar_beat(l.position)).with("qn", l.position).with("at", l.at.as_secs_f64());
            self.log.write(&e);
        }
        let second = m.load.seconds();
        if second != self.seen_second {
            self.seen_second = second;
            let r = m.load.reading().unwrap_or_default();
            self.stats(&r);
        }
    }

    fn stats(&mut self, r: &LoadReading) {
        let m = self.monitor;
        let rss = rss_bytes().unwrap_or(0);
        let pos = m.position();
        let s = &mut self.summary;
        s.peak_rss = s.peak_rss.max(rss);
        if r.average > s.worst_second {
            (s.worst_second, s.worst_second_at) = (r.average, pos);
        }
        if r.peak > s.worst_block {
            (s.worst_block, s.worst_block_at) = (r.peak, pos);
        }
        let mut e = Event::new("stats")
            .with("pos", bar_beat(pos))
            .with("playing", if m.playing() { "yes" } else { "no" })
            .with("dsp", r.average * 100.0)
            .with("peak", r.peak * 100.0)
            .with("late", r.late)
            .with("rss_mb", rss as f64 / (1 << 20) as f64);
        if let Some(u) = m.ui_load() {
            e = e.with("ui", u * 100.0);
        }
        self.log.write(&e);
    }

    /// Write the summary and return it.
    pub fn summary(&mut self) -> Summary {
        self.poll();
        self.summary.seconds = self.started.elapsed().as_secs_f64();
        let s = self.summary.clone();
        let e = Event::new("summary")
            .with("seconds", s.seconds)
            .with("late", s.late)
            .with("late_at", s.late_at.join(","))
            .with("worst_second", s.worst_second * 100.0)
            .with("worst_second_at", bar_beat(s.worst_second_at))
            .with("worst_block", s.worst_block * 100.0)
            .with("worst_block_at", bar_beat(s.worst_block_at))
            .with("peak_rss_mb", s.peak_rss as f64 / (1 << 20) as f64);
        self.log.write(&e);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_read_as_text_and_json() {
        let e = Event::new("stats").with("pos", "12.3").with("dsp", 31.25f64).with("late", 2u64).with("note", "a \"b\"");
        let t = UNIX_EPOCH + Duration::from_millis(1_763_223_601_123);
        assert_eq!(iso_utc(t), "2025-11-15T16:20:01.123Z");
        assert_eq!(e.text(t, Duration::from_millis(12_345)), "2025-11-15T16:20:01.123Z +12.345s stats pos=12.3 dsp=31.25 late=2 note=\"a 'b'\"");
        assert_eq!(e.json(t, Duration::from_millis(12_345)), r#"{"t":"2025-11-15T16:20:01.123Z","elapsed":12.345,"kind":"stats","pos":"12.3","dsp":31.25,"late":2,"note":"a \"b\""}"#);
    }

    #[test]
    fn positions_read_as_bars_and_beats() {
        assert_eq!(bar_beat(0.0), "1.1");
        assert_eq!(bar_beat(5.5), "2.2");
        assert_eq!(bar_beat(-1.0), "1.1");
    }

    #[test]
    fn late_blocks_queue_with_their_position() {
        let m = enable();
        let (_, from) = m.late_since(0);
        m.set_transport(16.0, true);
        drop(m.block(64, 48_000.0));
        std::thread::sleep(Duration::from_millis(10));
        drop(m.block(64, 48_000.0));
        let (late, _) = m.late_since(from);
        assert_eq!(late.len(), 1);
        assert_eq!(bar_beat(late[0].position), "5.1");
        assert!(!m.take_play());
        m.request_play();
        assert!(m.take_play() && !m.take_play());
    }

    #[test]
    fn the_log_rotates_and_keeps_its_lines() {
        let dir = std::env::temp_dir().join(format!("dreist-diag-test-{}", std::process::id()));
        let path = dir.join("app.log");
        let mut log = Log::open(Some(&path), false, true);
        log.write(&Event::new("start").with("os", "test"));
        let s = std::fs::read_to_string(&path).unwrap();
        assert!(s.starts_with("{\"t\":") && s.contains("\"kind\":\"start\""), "{s}");
        rotate(&path);
        assert!(dir.join("app.log.1").exists() && !path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn logs_go_to_the_platform_folder() {
        let d = log_dir("Dreist").unwrap();
        assert!(d.ends_with("Dreist") || d.ends_with("Dreist/Logs") || d.ends_with("Dreist\\Logs"), "{d:?}");
    }
}
