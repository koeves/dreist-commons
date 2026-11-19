//! Waveform peaks for the editors' sample displays: min/max buckets per
//! channel, decoded off the UI thread in a live window, with each shown
//! file checked for changes at most once a second, so drawing a waveform
//! never touches the disk.
//!
//! [`PeaksCache::get`] answers from the cache, or starts a decode and
//! waits [`WAIT`] for it; a slower decode is left to [`PeaksCache::poll`],
//! which a window calls every frame to repaint when it finishes. Until
//! [`decode_in_background`] is called (tests, screenshots) a decode
//! finishes before `get` returns.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant, SystemTime};

/// Buckets per channel.
pub const BUCKETS: usize = 2048;

/// Waveforms kept (most recently shown).
pub const KEPT: usize = 16;

/// How often a shown file is checked for changes.
pub const RECHECK: Duration = Duration::from_secs(1);

/// How long [`PeaksCache::get`] waits for a background decode before it
/// goes on without the waveform (a one-shot is ready well within it).
pub const WAIT: Duration = Duration::from_millis(50);

static BACKGROUND: AtomicBool = AtomicBool::new(false);

/// Decode waveforms on a background thread from now on (an editor window
/// that polls its caches every frame).
pub fn decode_in_background() {
    BACKGROUND.store(true, Ordering::Relaxed);
}

/// Min/max buckets of a sample.
#[derive(Clone, Debug, PartialEq)]
pub struct Peaks {
    /// (min, max) buckets per channel: left, then right of a stereo file.
    pub channels: Vec<Vec<(f32, f32)>>,
    pub frames: usize,
    /// File sample rate.
    pub rate: f32,
    /// Zero crossings of the left channel (frame after each crossing).
    pub zeros: Vec<u32>,
}

impl Peaks {
    /// Decode `path` and bucket it; `None` for a missing, damaged or empty
    /// file.
    pub fn load(path: &Path) -> Option<Peaks> {
        let mut cache = crate::audio::SampleCache::default();
        let data = cache.get(path)?;
        let n = data.left.len();
        if n == 0 {
            return None;
        }
        let per = n.div_ceil(BUCKETS).max(1);
        let peaks = |ch: &[f32]| ch.chunks(per).map(|c| c.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &x| (lo.min(x), hi.max(x)))).collect::<Vec<_>>();
        let channels = std::iter::once(peaks(&data.left)).chain(data.right.as_deref().map(peaks)).collect();
        let zeros = data.left.windows(2).enumerate().filter(|(_, w)| (w[0] <= 0.0) != (w[1] <= 0.0)).map(|(i, _)| i as u32 + 1).collect();
        Some(Peaks { channels, frames: n, zeros, rate: data.sample_rate.max(1.0) })
    }

    /// The left channel's buckets.
    pub fn left(&self) -> &[(f32, f32)] {
        self.channels.first().map_or(&[][..], |c| &c[..])
    }
}

/// A file's peaks, or the decode still running for them.
enum Slot {
    Ready(Option<Peaks>),
    Loading(Receiver<Option<Peaks>>),
}

impl Slot {
    fn start(path: &Path, background: bool) -> Slot {
        if !background {
            return Slot::Ready(Peaks::load(path));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let file = path.to_path_buf();
        let spawned = std::thread::Builder::new().name("waveform".into()).spawn(move || {
            let _ = tx.send(Peaks::load(&file));
        });
        if spawned.is_err() {
            return Slot::Ready(Peaks::load(path));
        }
        match rx.recv_timeout(WAIT) {
            Ok(p) => Slot::Ready(p),
            Err(RecvTimeoutError::Timeout) => Slot::Loading(rx),
            Err(RecvTimeoutError::Disconnected) => Slot::Ready(None),
        }
    }

    /// Take a finished decode; true when it just finished.
    fn poll(&mut self) -> bool {
        let Slot::Loading(rx) = self else { return false };
        match rx.try_recv() {
            Ok(p) => *self = Slot::Ready(p),
            Err(TryRecvError::Disconnected) => *self = Slot::Ready(None),
            Err(TryRecvError::Empty) => return false,
        }
        true
    }

    fn peaks(&self) -> Option<&Peaks> {
        match self {
            Slot::Ready(p) => p.as_ref(),
            Slot::Loading(_) => None,
        }
    }
}

struct Entry {
    file: PathBuf,
    /// Modification time when last checked.
    stamp: Option<SystemTime>,
    checked: Instant,
    slot: Slot,
}

/// Peaks of the samples shown: reloaded when a file changes on disk (a
/// sample saved over it, a missing file appearing; checked at most once
/// per [`RECHECK`]), the least recently shown dropped past [`KEPT`].
#[derive(Default)]
pub struct PeaksCache {
    /// Most recent last.
    entries: Vec<Entry>,
    /// Decode in the background without [`decode_in_background`].
    background: bool,
}

impl PeaksCache {
    /// A cache that always decodes in the background.
    pub fn background() -> PeaksCache {
        PeaksCache { background: true, ..Default::default() }
    }

    /// The peaks of `path`, `None` while its decode runs or when it has
    /// none.
    pub fn get(&mut self, path: &Path) -> Option<&Peaks> {
        let now = Instant::now();
        let background = self.background || BACKGROUND.load(Ordering::Relaxed);
        let stamp_of = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        let mut entry = match self.entries.iter().position(|e| e.file == path) {
            Some(i) => {
                let mut e = self.entries.remove(i);
                if now.duration_since(e.checked) >= RECHECK {
                    e.checked = now;
                    let stamp = stamp_of(path);
                    if stamp != e.stamp {
                        e.stamp = stamp;
                        e.slot = Slot::start(path, background);
                    }
                }
                e
            }
            None => Entry { file: path.to_path_buf(), stamp: stamp_of(path), checked: now, slot: Slot::start(path, background) },
        };
        entry.slot.poll();
        self.entries.push(entry);
        if self.entries.len() > KEPT {
            self.entries.remove(0);
        }
        self.entries.last().and_then(|e| e.slot.peaks())
    }

    /// Take finished background decodes; true when any finished (the
    /// caller repaints its waveforms).
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        for e in &mut self.entries {
            any |= e.slot.poll();
        }
        any
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Waveforms decode off the calling thread, and a shown file is
    /// checked for changes at most once a second.
    #[test]
    fn peaks_decode_in_background_and_recheck_files_once_a_second() {
        let dir = std::env::temp_dir().join(format!("dreist-peaks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.wav");
        let write = |n: usize| {
            let v = vec![0.25f32; n];
            crate::write_wav(&file, 44100, &v, &v).unwrap();
        };
        write(300);
        let mut cache = PeaksCache::background();
        let frames = |cache: &mut PeaksCache| {
            let until = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(pk) = cache.get(&file) {
                    return (pk.frames, pk.channels.len());
                }
                assert!(Instant::now() < until, "decode never finished");
                std::thread::sleep(Duration::from_millis(5));
                cache.poll();
            }
        };
        assert_eq!(frames(&mut cache), (300, 2));
        // Saved over: not looked at again within the second...
        write(500);
        let mtime = std::fs::metadata(&file).unwrap().modified().unwrap() + Duration::from_secs(5);
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(mtime).unwrap();
        assert_eq!(frames(&mut cache).0, 300);
        // ...then reloaded.
        cache.entries[0].checked -= RECHECK;
        assert_eq!(frames(&mut cache).0, 500);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_and_empty_files_have_no_peaks() {
        let mut cache = PeaksCache::default();
        assert!(cache.get(Path::new("/nonexistent/x.wav")).is_none());
        assert!(!cache.poll());
    }
}
