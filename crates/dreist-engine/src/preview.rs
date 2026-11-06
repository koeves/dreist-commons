//! File previews for the editors' browsers: the decode runs on a worker
//! thread (the original loads a browser preview on a job too, see
//! [re:8e7ba7]), and the last files previewed stay decoded, so clicking
//! through a folder and back starts at once.
//!
//! [`PreviewLoader::request`] answers from the cache, or waits a short
//! while for the worker and then leaves the file to
//! [`PreviewLoader::take_ready`]; only the latest request is decoded (a
//! newer one cancels the decode in progress).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use dreist_dsp::voice::SampleData;

/// Decoded audio kept by default (bytes of samples).
pub const CACHE_BYTES: usize = 64 << 20;

/// File size and modification time: a changed file decodes again.
type Stamp = Option<(u64, SystemTime)>;

fn stamp(path: &Path) -> Stamp {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

fn bytes(d: &SampleData) -> usize {
    (d.left.len() + d.right.as_ref().map_or(0, Vec::len)) * std::mem::size_of::<f32>()
}

struct Entry {
    path: PathBuf,
    stamp: Stamp,
    data: Arc<SampleData>,
}

/// Least recently used last-out list, bounded by sample bytes (the newest
/// entry always stays).
struct Lru {
    entries: Vec<Entry>,
    budget: usize,
}

impl Lru {
    fn get(&mut self, path: &Path) -> Option<Arc<SampleData>> {
        let i = self.entries.iter().position(|e| e.path == path)?;
        let e = self.entries.remove(i);
        if e.stamp.is_none() || e.stamp != stamp(path) {
            return None;
        }
        let data = e.data.clone();
        self.entries.push(e);
        Some(data)
    }

    fn insert(&mut self, path: PathBuf, stamp: Stamp, data: Arc<SampleData>) {
        self.entries.retain(|e| e.path != path);
        self.entries.push(Entry { path, stamp, data });
        let mut total: usize = self.entries.iter().map(|e| bytes(&e.data)).sum();
        while total > self.budget && self.entries.len() > 1 {
            total -= bytes(&self.entries.remove(0).data);
        }
    }
}

#[derive(Default)]
struct State {
    /// The file the worker is to decode, with its request number.
    job: Option<(u64, PathBuf)>,
    /// The latest request's outcome (`None`: the file did not decode).
    ready: Option<(u64, Option<Arc<SampleData>>)>,
    quit: bool,
}

struct Shared {
    state: Mutex<State>,
    /// The worker waits here for a job, the editor for a result.
    job_posted: Condvar,
    job_done: Condvar,
    /// The latest request number (a decode for an older one stops).
    latest: AtomicU64,
    cache: Mutex<Lru>,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn cache(&self) -> MutexGuard<'_, Lru> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn worker(shared: Arc<Shared>) {
    loop {
        let (n, path) = {
            let mut s = shared.state();
            loop {
                if s.quit {
                    return;
                }
                if let Some(j) = s.job.take() {
                    break j;
                }
                s = shared.job_posted.wait(s).unwrap_or_else(|e| e.into_inner());
            }
        };
        let st = stamp(&path);
        let stale = || shared.latest.load(Ordering::Acquire) != n;
        let data = crate::audio::load_unless(&path, &stale).ok().map(Arc::new);
        if stale() {
            continue;
        }
        if let Some(d) = &data {
            shared.cache().insert(path, st, d.clone());
        }
        let mut s = shared.state();
        if shared.latest.load(Ordering::Acquire) == n {
            s.ready = Some((n, data));
            shared.job_done.notify_all();
        }
    }
}

/// Decodes browser previews off the editor thread and keeps the recent
/// ones (see the module docs).
pub struct PreviewLoader {
    shared: Arc<Shared>,
    started: bool,
    /// The request whose result is still to come.
    pending: Option<u64>,
}

impl Default for PreviewLoader {
    fn default() -> Self {
        PreviewLoader::new(CACHE_BYTES)
    }
}

impl PreviewLoader {
    /// A loader keeping up to `budget` bytes of decoded audio (and always
    /// the latest file).
    pub fn new(budget: usize) -> Self {
        let shared = Shared { state: Mutex::default(), job_posted: Condvar::new(), job_done: Condvar::new(), latest: AtomicU64::new(0), cache: Mutex::new(Lru { entries: Vec::new(), budget }) };
        PreviewLoader { shared: Arc::new(shared), started: false, pending: None }
    }

    /// The decoded file, from the cache or when the worker finishes it
    /// within `wait`; otherwise `None`, and [`take_ready`](Self::take_ready)
    /// hands it over later. Replaces any request still pending.
    pub fn request(&mut self, path: &Path, wait: Duration) -> Option<Arc<SampleData>> {
        let n = self.shared.latest.fetch_add(1, Ordering::AcqRel) + 1;
        self.pending = None;
        if let Some(d) = self.shared.cache().get(path) {
            self.shared.state().job = None;
            return Some(d);
        }
        if !self.started {
            let shared = self.shared.clone();
            if std::thread::Builder::new().name("preview-decode".into()).spawn(move || worker(shared)).is_err() {
                // No thread: decode here.
                let st = stamp(path);
                let d = crate::audio::load(path).ok().map(Arc::new)?;
                self.shared.cache().insert(path.to_owned(), st, d.clone());
                return Some(d);
            }
            self.started = true;
        }
        let deadline = Instant::now() + wait;
        let mut s = self.shared.state();
        s.ready = None;
        s.job = Some((n, path.to_owned()));
        self.shared.job_posted.notify_all();
        loop {
            if let Some((m, _)) = &s.ready {
                if *m == n {
                    return s.ready.take().and_then(|(_, d)| d);
                }
            }
            let now = Instant::now();
            if now >= deadline {
                self.pending = Some(n);
                return None;
            }
            s = self.shared.job_done.wait_timeout(s, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// The pending request's file once decoded (once; `None` while it is
    /// still decoding, or when it did not decode).
    pub fn take_ready(&mut self) -> Option<Arc<SampleData>> {
        let n = self.pending?;
        let mut s = self.shared.state();
        match &s.ready {
            Some((m, _)) if *m == n => {
                self.pending = None;
                s.ready.take().and_then(|(_, d)| d)
            }
            _ => None,
        }
    }

    /// Whether a request is still decoding.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Drop the pending request (the preview was stopped).
    pub fn cancel(&mut self) {
        self.shared.latest.fetch_add(1, Ordering::AcqRel);
        self.pending = None;
        let mut s = self.shared.state();
        s.job = None;
        s.ready = None;
    }

    /// Files held decoded.
    pub fn cached(&self) -> usize {
        self.shared.cache().entries.len()
    }
}

impl Drop for PreviewLoader {
    /// The worker stops after its current packet (not joined: the editor
    /// does not wait for it).
    fn drop(&mut self) {
        self.shared.latest.fetch_add(1, Ordering::AcqRel);
        self.shared.state().quit = true;
        self.shared.job_posted.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mono 16-bit WAV of `frames` frames.
    fn wav(path: &Path, frames: usize) {
        let data: Vec<u8> = (0..frames).flat_map(|i| ((i % 2000) as i16 * 10).to_le_bytes()).collect();
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&44100u32.to_le_bytes());
        f.extend_from_slice(&88200u32.to_le_bytes());
        f.extend_from_slice(&2u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&(data.len() as u32).to_le_bytes());
        f.extend_from_slice(&data);
        std::fs::write(path, f).unwrap();
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dreist-preview-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn second_request_comes_from_the_cache() {
        let d = dir("cache");
        let a = d.join("a.wav");
        wav(&a, 1000);
        let mut l = PreviewLoader::default();
        let first = l.request(&a, Duration::from_secs(10)).expect("decoded within the wait");
        assert_eq!(first.left.len(), 1000);
        let again = l.request(&a, Duration::ZERO).expect("cached");
        assert!(Arc::ptr_eq(&first, &again));
        // A changed file decodes again.
        wav(&a, 500);
        let t = SystemTime::now() + Duration::from_secs(5);
        std::fs::File::options().write(true).open(&a).unwrap().set_modified(t).unwrap();
        assert_eq!(l.request(&a, Duration::from_secs(10)).unwrap().left.len(), 500);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_slow_decode_is_handed_over_later() {
        let d = dir("later");
        let a = d.join("long.wav");
        wav(&a, 2_000_000);
        let mut l = PreviewLoader::default();
        let t = Instant::now();
        assert!(l.request(&a, Duration::ZERO).is_none());
        assert!(t.elapsed() < Duration::from_millis(50), "the request does not wait for the decode");
        assert!(l.is_pending());
        let deadline = Instant::now() + Duration::from_secs(20);
        let got = loop {
            if let Some(g) = l.take_ready() {
                break g;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(got.left.len(), 2_000_000);
        assert!(l.take_ready().is_none(), "handed over once");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_newer_request_or_cancel_wins() {
        let d = dir("newer");
        let (a, b) = (d.join("a.wav"), d.join("b.wav"));
        wav(&a, 2_000_000);
        wav(&b, 100);
        let mut l = PreviewLoader::default();
        assert!(l.request(&a, Duration::ZERO).is_none());
        let got = l.request(&b, Duration::from_secs(10)).expect("b decodes");
        assert_eq!(got.left.len(), 100);
        assert!(l.take_ready().is_none(), "a's result is never handed over");
        assert!(l.request(&a, Duration::ZERO).is_none());
        l.cancel();
        std::thread::sleep(Duration::from_millis(50));
        assert!(!l.is_pending() && l.take_ready().is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_cache_stays_within_its_budget() {
        let d = dir("budget");
        // 1000 mono frames = 4000 bytes each; room for two.
        let mut l = PreviewLoader::new(8000);
        for i in 0..5 {
            let p = d.join(format!("{i}.wav"));
            wav(&p, 1000);
            assert!(l.request(&p, Duration::from_secs(10)).is_some());
        }
        assert_eq!(l.cached(), 2);
        // One file over the budget is still kept.
        let big = d.join("big.wav");
        wav(&big, 10_000);
        assert!(l.request(&big, Duration::from_secs(10)).is_some());
        assert_eq!(l.cached(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_file_gives_nothing() {
        let mut l = PreviewLoader::default();
        assert!(l.request(Path::new("/nonexistent/x.wav"), Duration::from_secs(10)).is_none());
        assert!(!l.is_pending());
    }
}
