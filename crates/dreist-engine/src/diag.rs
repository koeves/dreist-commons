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

/// Late or slow blocks kept until the recorder takes them.
const EVENTS: usize = 256;

/// A block is slow (and recorded in detail) above this share of its time.
pub const SLOW: f32 = 0.8;

/// Fields of a recorded block (see [`BlockEvent`]).
const FIELDS: usize = 13;

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
    /// The main output's peak since the last stats line (`f32` bits).
    out_peak: AtomicU32,
    /// Late or slow blocks ([`BlockEvent`] fields), written at
    /// `head % EVENTS`.
    events: [[AtomicU64; FIELDS]; EVENTS],
    /// Voices sounding and timestretching (set by the audio thread).
    voices: AtomicU32,
    stretch_voices: AtomicU32,
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
        out_peak: AtomicU32::new(0),
        events: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU64::new(0))),
        voices: AtomicU32::new(0),
        stretch_voices: AtomicU32::new(0),
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
    wall: Instant,
    thread: thread::Stats,
}

impl Drop for BlockScope<'_> {
    fn drop(&mut self) {
        let m = self.monitor;
        let wall = self.wall.elapsed();
        let late = m.load.end(self.samples, self.rate);
        // Of the stream's period (short callbacks come between full ones).
        let audio = self.samples as f64 / self.rate as f64;
        let share = wall.as_secs_f64() / audio.max(m.load.period_ns() as f64 * 1e-9).max(1e-9);
        if !late && share < SLOW as f64 {
            return;
        }
        let t = thread::Stats::now().since(&self.thread);
        let i = m.head.fetch_add(1, Relaxed) as usize % EVENTS;
        let fields = [
            m.started.elapsed().as_nanos() as u64,
            m.position.load(Relaxed),
            self.samples as u64,
            wall.as_nanos() as u64,
            t.cpu_ns.map_or(0, |v| v + 1),
            t.minflt.map_or(0, |v| v + 1),
            t.majflt.map_or(0, |v| v + 1),
            t.nvcsw.map_or(0, |v| v + 1),
            t.nivcsw.map_or(0, |v| v + 1),
            thread::cpu().map_or(0, |c| c as u64 + 1),
            m.voices.load(Relaxed) as u64,
            m.stretch_voices.load(Relaxed) as u64,
            late as u64,
        ];
        for (slot, v) in m.events[i].iter().zip(fields) {
            slot.store(v, Relaxed);
        }
    }
}

/// A late or slow block: when (since the monitor started) and where in the
/// song, its length, its wall and thread CPU time, the audio thread's page
/// faults and context switches during it, the CPU it ran on, and the voices
/// sounding. `None`: the platform does not tell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockEvent {
    pub at: Duration,
    pub position: f64,
    pub frames: u64,
    pub wall: Duration,
    pub cpu: Option<Duration>,
    pub minflt: Option<u64>,
    pub majflt: Option<u64>,
    pub nvcsw: Option<u64>,
    pub nivcsw: Option<u64>,
    pub cpu_number: Option<u32>,
    pub voices: u32,
    pub stretch_voices: u32,
    pub late: bool,
}

/// The audio thread's own counters (Linux; CPU time also on macOS).
mod thread {
    #[derive(Clone, Copy, Default)]
    pub struct Stats {
        pub cpu_ns: Option<u64>,
        pub minflt: Option<u64>,
        pub majflt: Option<u64>,
        pub nvcsw: Option<u64>,
        pub nivcsw: Option<u64>,
    }

    impl Stats {
        pub fn now() -> Stats {
            let mut s = Stats::default();
            #[cfg(unix)]
            {
                let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
                // SAFETY: `ts` is a valid timespec for the call to fill.
                if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) } == 0 {
                    s.cpu_ns = Some(ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64);
                }
            }
            #[cfg(target_os = "linux")]
            {
                // SAFETY: `ru` is a valid rusage for the call to fill.
                let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
                if unsafe { libc::getrusage(libc::RUSAGE_THREAD, &mut ru) } == 0 {
                    s.minflt = Some(ru.ru_minflt as u64);
                    s.majflt = Some(ru.ru_majflt as u64);
                    s.nvcsw = Some(ru.ru_nvcsw as u64);
                    s.nivcsw = Some(ru.ru_nivcsw as u64);
                }
            }
            s
        }

        /// What changed since `before`.
        pub fn since(&self, before: &Stats) -> Stats {
            let d = |a: Option<u64>, b: Option<u64>| Some(a?.saturating_sub(b?));
            Stats {
                cpu_ns: d(self.cpu_ns, before.cpu_ns),
                minflt: d(self.minflt, before.minflt),
                majflt: d(self.majflt, before.majflt),
                nvcsw: d(self.nvcsw, before.nvcsw),
                nivcsw: d(self.nivcsw, before.nivcsw),
            }
        }
    }

    /// The CPU this thread runs on (Linux).
    pub fn cpu() -> Option<u32> {
        #[cfg(target_os = "linux")]
        {
            // SAFETY: no arguments; returns -1 on failure.
            let c = unsafe { libc::sched_getcpu() };
            if c >= 0 {
                return Some(c as u32);
            }
        }
        None
    }
}

/// A CPU's current frequency in MHz (Linux), read off the audio thread.
pub fn cpu_mhz(cpu: u32) -> Option<u64> {
    let khz: u64 = std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq")).ok()?.trim().parse().ok()?;
    Some(khz / 1000)
}

impl Monitor {
    /// The audio thread, as a block of `samples` at `rate` starts; the
    /// block ends when the guard drops.
    pub fn block(&self, samples: usize, rate: f32) -> BlockScope<'_> {
        self.load.begin();
        BlockScope { monitor: self, samples, rate, wall: Instant::now(), thread: thread::Stats::now() }
    }

    /// The audio thread: voices sounding and timestretching.
    pub fn set_voices(&self, voices: usize, stretching: usize) {
        self.voices.store(voices as u32, Relaxed);
        self.stretch_voices.store(stretching as u32, Relaxed);
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

    /// The audio thread: the main output of a block.
    pub fn output(&self, left: &[f32], right: &[f32]) {
        let peak = left.iter().chain(right).fold(0.0f32, |m, s| m.max(s.abs()));
        self.out_peak.fetch_max(peak.to_bits(), Relaxed);
    }

    /// The main output's peak since the last call (dBFS; `-inf` silent).
    pub fn take_output_db(&self) -> f32 {
        20.0 * f32::from_bits(self.out_peak.swap(0, Relaxed)).log10()
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

    /// Late or slow blocks from number `from` on (the next `from` is
    /// returned); older ones than the queue holds are skipped.
    pub fn blocks_since(&self, from: u64) -> (Vec<BlockEvent>, u64) {
        let head = self.head.load(Relaxed);
        let first = from.max(head.saturating_sub(EVENTS as u64));
        let opt = |v: u64| v.checked_sub(1);
        let out = (first..head)
            .map(|n| {
                let e: [u64; FIELDS] = std::array::from_fn(|k| self.events[n as usize % EVENTS][k].load(Relaxed));
                BlockEvent {
                    at: Duration::from_nanos(e[0]),
                    position: f64::from_bits(e[1]),
                    frames: e[2],
                    wall: Duration::from_nanos(e[3]),
                    cpu: opt(e[4]).map(Duration::from_nanos),
                    minflt: opt(e[5]),
                    majflt: opt(e[6]),
                    nvcsw: opt(e[7]),
                    nivcsw: opt(e[8]),
                    cpu_number: opt(e[9]).map(|c| c as u32),
                    voices: e[10] as u32,
                    stretch_voices: e[11] as u32,
                    late: e[12] != 0,
                }
            })
            .collect();
        (out, head)
    }
}

/// A `late` or `slow` line for block `b`: position, length, wall and
/// thread CPU time (ms), faults, context switches, CPU (and its MHz),
/// voices; fields the platform does not tell are left out.
fn block_event(b: &BlockEvent) -> Event {
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    let mut e =
        Event::new(if b.late { "late" } else { "slow" }).with("pos", bar_beat(b.position)).with("qn", b.position).with("at", b.at.as_secs_f64()).with("frames", b.frames).with("wall_ms", ms(b.wall));
    if let Some(c) = b.cpu {
        e = e.with("cpu_ms", ms(c));
    }
    for (k, v) in [("minflt", b.minflt), ("majflt", b.majflt), ("nvcsw", b.nvcsw), ("nivcsw", b.nivcsw)] {
        if let Some(v) = v {
            e = e.with(k, v);
        }
    }
    if let Some(c) = b.cpu_number {
        e = e.with("cpu", c);
        if let Some(mhz) = cpu_mhz(c) {
            e = e.with("mhz", mhz);
        }
    }
    e.with("voices", b.voices).with("stretch", b.stretch_voices)
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
                Value::Number(n) if n.is_finite() => format!("{}", (n * 1000.0).round() / 1000.0),
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
    /// Blocks over [`SLOW`] of their time that were not late.
    pub slow: u64,
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
        let (blocks, next) = m.blocks_since(self.late_from);
        self.late_from = next;
        for b in blocks {
            if b.late {
                self.summary.late += 1;
                if self.summary.late_at.len() < 32 {
                    self.summary.late_at.push(bar_beat(b.position));
                }
            } else {
                self.summary.slow += 1;
            }
            self.log.write(&block_event(&b));
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
            .with("rss_mb", rss as f64 / (1 << 20) as f64)
            .with("out_db", m.take_output_db().max(-120.0));
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
            .with("slow", s.slow)
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
        let (_, from) = m.blocks_since(0);
        m.set_transport(16.0, true);
        // Past the first second of audio (the stream starting).
        for _ in 0..50 {
            drop(m.block(1024, 48_000.0));
        }
        let (_, from) = m.blocks_since(from);
        m.set_voices(12, 3);
        // The period is 21 ms (the warm-up blocks): a short callback, then
        // the next block 40 ms later is late.
        drop(m.block(64, 48_000.0));
        std::thread::sleep(Duration::from_millis(40));
        drop(m.block(64, 48_000.0));
        // A slow block: 25 ms of a 21 ms period.
        {
            let _b = m.block(64, 48_000.0);
            std::thread::sleep(Duration::from_millis(25));
        }
        let (blocks, _) = m.blocks_since(from);
        let late: Vec<_> = blocks.iter().filter(|b| b.late).collect();
        assert_eq!(late.len(), 1);
        assert_eq!(bar_beat(late[0].position), "5.1");
        let slow = blocks.last().unwrap();
        assert!(slow.wall >= Duration::from_millis(25) && slow.frames == 64 && slow.voices == 12 && slow.stretch_voices == 3);
        // Sleeping is not computing: the thread CPU time stays well below.
        if let Some(cpu) = slow.cpu {
            assert!(cpu < slow.wall / 2, "{slow:?}");
        }
        let line = block_event(slow).text(SystemTime::UNIX_EPOCH, Duration::ZERO);
        assert!(line.contains("frames=64") && line.contains("voices=12") && line.contains("stretch=3"), "{line}");
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

/// The standalone launcher's debug options (taken out of the command line
/// before the audio wrapper reads it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DebugArgs {
    /// `--debug`: diagnostics on, events to stderr and the log file.
    pub debug: bool,
    /// `--debug-json`: JSON lines instead of text.
    pub json: bool,
    /// `--debug-log <path>`: the log file (default: the platform's log
    /// folder, `<app>-debug.log`).
    pub log: Option<PathBuf>,
    /// `--play <file>`: open this project and start playback.
    pub play: Option<PathBuf>,
    /// `--duration <seconds>`: quit after this long.
    pub duration: Option<f64>,
    /// `--until-end`: quit when playback stops or a looping song jumps
    /// back to its start.
    pub until_end: bool,
    /// `--fail-on-late <n>`: exit with status 3 after more late blocks.
    pub fail_on_late: Option<u64>,
}

impl DebugArgs {
    /// Split `args` (with the program name first) into the debug options
    /// and the rest; an option missing its value or with a bad one is an
    /// error. Any of them implies `--debug`.
    pub fn split(args: Vec<String>) -> Result<(DebugArgs, Vec<String>), String> {
        let mut d = DebugArgs::default();
        let mut rest = Vec::with_capacity(args.len());
        let mut it = args.into_iter();
        while let Some(a) = it.next() {
            let (flag, inline) = match a.split_once('=') {
                Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
                _ => (a.clone(), None),
            };
            let mut value = |name: &str| inline.clone().or_else(|| it.next()).ok_or_else(|| format!("{name} needs a value"));
            match flag.as_str() {
                "--debug" => d.debug = true,
                "--debug-json" => d.json = true,
                "--debug-log" => d.log = Some(PathBuf::from(value("--debug-log")?)),
                "--play" => d.play = Some(PathBuf::from(value("--play")?)),
                "--duration" => d.duration = Some(value("--duration")?.parse().map_err(|_| "--duration takes seconds".to_string())?),
                "--until-end" => d.until_end = true,
                "--fail-on-late" => d.fail_on_late = Some(value("--fail-on-late")?.parse().map_err(|_| "--fail-on-late takes a count".to_string())?),
                _ => {
                    rest.push(a);
                    continue;
                }
            }
            d.debug = true;
        }
        Ok((d, rest))
    }

    /// The exit status a run with `s` ends with.
    pub fn status(&self, s: &Summary) -> i32 {
        if self.fail_on_late.is_some_and(|n| s.late > n) { 3 } else { 0 }
    }
}

/// A debug run's recorder, shared by its thread and the end of `main`.
static RUN: OnceLock<(std::sync::Mutex<Recorder>, DebugArgs)> = OnceLock::new();

/// Start a debug run for `app` (`args` from [`DebugArgs::split`], `launch`
/// the remaining command line for the start-up line): the monitor, the log
/// and a thread that records every 100 ms and ends the process after
/// `--duration` or, with `--until-end`, once playback stopped.
pub fn start_run(app: &str, version: &str, args: DebugArgs, launch: &[String]) -> &'static Monitor {
    let monitor = enable();
    let path = args.log.clone().or_else(|| log_dir(app).map(|d| d.join(format!("{}-debug.log", app.to_lowercase()))));
    let mut log = Log::open(path.as_deref(), true, args.json);
    let mut e = system_info().with("app", app).with("version", version).with("args", launch.get(1..).unwrap_or_default().join(" "));
    if let Some(p) = log.path() {
        e = e.with("log", p.display().to_string());
    }
    if let Some(p) = &args.play {
        e = e.with("play", p.display().to_string());
    }
    log.write(&e);
    if args.play.is_some() {
        monitor.request_play();
    }
    let _ = RUN.set((std::sync::Mutex::new(Recorder::new(log, monitor)), args));
    std::thread::Builder::new()
        .name("diagnostics".into())
        .spawn(|| {
            let Some((rec, args)) = RUN.get() else { return };
            let started = Instant::now();
            let (mut was_playing, mut last) = (false, 0.0f64);
            signals::catch();
            loop {
                std::thread::sleep(Duration::from_millis(100));
                if let Ok(mut r) = rec.lock() {
                    r.poll();
                }
                let (playing, pos) = (monitor.playing(), monitor.position());
                let ended = args.until_end && was_playing && (!playing || pos < last - 1.0);
                (was_playing, last) = (was_playing || playing, pos);
                if let Some(sig) = signals::caught() {
                    if let Ok(mut r) = rec.lock() {
                        r.log().write(&Event::new("signal").with("signal", sig));
                    }
                    let _ = end_run();
                    std::process::exit(128 + sig as i32);
                }
                if ended || args.duration.is_some_and(|d| started.elapsed().as_secs_f64() >= d) {
                    std::process::exit(end_run().unwrap_or(0));
                }
            }
        })
        .ok();
    monitor
}

/// End the debug run: write the summary; its exit status (`None` without
/// a run).
pub fn end_run() -> Option<i32> {
    let (rec, args) = RUN.get()?;
    let s = rec.lock().ok()?.summary();
    Some(args.status(&s))
}

/// SIGINT and SIGTERM end a debug run with its summary (Unix).
mod signals {
    use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

    static CAUGHT: AtomicU32 = AtomicU32::new(0);

    #[cfg(unix)]
    extern "C" fn on_signal(sig: i32) {
        CAUGHT.store(sig as u32, Relaxed);
    }

    #[cfg(unix)]
    unsafe extern "C" {
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }

    pub fn catch() {
        #[cfg(unix)]
        for sig in [2, 15] {
            // SAFETY: the handler only stores an atomic.
            unsafe { signal(sig, on_signal) };
        }
    }

    pub fn caught() -> Option<u32> {
        match CAUGHT.load(Relaxed) {
            0 => None,
            s => Some(s),
        }
    }
}

#[cfg(test)]
mod launcher_tests {
    use super::*;

    fn split(a: &[&str]) -> Result<(DebugArgs, Vec<String>), String> {
        DebugArgs::split(a.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn debug_options_come_out_of_the_command_line() {
        let (d, rest) = split(&["dreist", "-p", "1024", "--play", "song.geist", "--until-end", "--fail-on-late=0", "--backend", "alsa", "--debug-json"]).unwrap();
        assert_eq!(rest, ["dreist", "-p", "1024", "--backend", "alsa"]);
        assert!(d.debug && d.json && d.until_end);
        assert_eq!(d.play.as_deref(), Some(Path::new("song.geist")));
        assert_eq!(d.fail_on_late, Some(0));
        let (d, rest) = split(&["dreist", "--duration", "90", "--debug-log=/tmp/x.log"]).unwrap();
        assert_eq!(rest, ["dreist"]);
        assert_eq!((d.duration, d.log.as_deref()), (Some(90.0), Some(Path::new("/tmp/x.log"))));
        let (d, rest) = split(&["dreist", "--sample-rate", "44100"]).unwrap();
        assert!(!d.debug && rest.len() == 3);
        assert!(split(&["dreist", "--play"]).is_err());
        assert!(split(&["dreist", "--duration", "soon"]).is_err());
    }

    #[test]
    fn too_many_late_blocks_fail_the_run() {
        let d = DebugArgs { fail_on_late: Some(2), ..DebugArgs::default() };
        assert_eq!(d.status(&Summary { late: 2, ..Summary::default() }), 0);
        assert_eq!(d.status(&Summary { late: 3, ..Summary::default() }), 3);
        assert_eq!(DebugArgs::default().status(&Summary { late: 9, ..Summary::default() }), 0);
    }
}
