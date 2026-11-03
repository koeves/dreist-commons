//! Validation harness helpers shared by the crates' integration tests.
//!
//! - [`Golden`]: named fingerprints of rendered audio stored under the
//!   calling crate's `tests/golden/<set>.txt`; a mismatch fails with the
//!   first differing sample. `UPDATE_GOLDEN=1` rewrites the file.
//! - [`fnv`]: a 64-bit FNV-1a hash over the exact `f32` bits.
//! - [`Stats`]: finiteness, peak, RMS and subnormal counts of a signal.
//! - [`CountingAlloc`]: a global allocator that counts allocations made
//!   while a thread-local flag is set (real-time safety checks).

use std::collections::BTreeMap;
use std::path::PathBuf;

/// FNV-1a over the bit patterns of `samples`.
pub fn fnv<'a>(samples: impl IntoIterator<Item = &'a f32>) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for s in samples {
        for b in s.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// FNV-1a over raw bytes (rendered images).
pub fn fnv_bytes(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Level statistics of a signal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub finite: bool,
    pub peak: f32,
    pub rms: f32,
    pub subnormals: usize,
}

impl Stats {
    pub fn of<'a>(samples: impl IntoIterator<Item = &'a f32>) -> Stats {
        let (mut finite, mut peak, mut sum, mut n, mut subnormals) = (true, 0.0f32, 0.0f64, 0usize, 0usize);
        for &x in samples {
            if !x.is_finite() {
                finite = false;
                continue;
            }
            peak = peak.max(x.abs());
            sum += (x as f64) * (x as f64);
            n += 1;
            subnormals += x.is_subnormal() as usize;
        }
        Stats { finite, peak, rms: if n == 0 { 0.0 } else { (sum / n as f64).sqrt() as f32 }, subnormals }
    }
}

/// A set of stored fingerprints for one test file.
pub struct Golden {
    path: PathBuf,
    stored: BTreeMap<String, u64>,
    seen: BTreeMap<String, u64>,
    update: bool,
    failures: Vec<String>,
}

impl Golden {
    /// The set `name` in the crate under test (`CARGO_MANIFEST_DIR`).
    pub fn open(name: &str) -> Golden {
        let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("run under cargo")).join("tests/golden");
        let path = dir.join(format!("{name}.txt"));
        let stored = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let (k, v) = l.split_once(' ')?;
                Some((k.to_string(), u64::from_str_radix(v.trim(), 16).ok()?))
            })
            .collect();
        Golden { path, stored, seen: BTreeMap::new(), update: std::env::var_os("UPDATE_GOLDEN").is_some(), failures: Vec::new() }
    }

    /// Record `key`'s fingerprint; `preview` (a channel) is reported on a
    /// mismatch to help locating the change.
    pub fn check(&mut self, key: &str, hash: u64, preview: &[f32]) {
        self.seen.insert(key.to_string(), hash);
        if self.update {
            return;
        }
        match self.stored.get(key) {
            None => self.failures.push(format!("{key}: no stored fingerprint (run with UPDATE_GOLDEN=1)")),
            Some(&h) if h != hash => {
                let s = Stats::of(preview);
                self.failures.push(format!("{key}: fingerprint {hash:016x} ≠ stored {h:016x} (peak {:.4}, rms {:.5})", s.peak, s.rms));
            }
            _ => {}
        }
    }

    /// Write the file (update mode) or fail with every mismatch.
    pub fn finish(self) {
        if self.update {
            let _ = std::fs::create_dir_all(self.path.parent().unwrap());
            let text: String = self.seen.iter().map(|(k, v)| format!("{k} {v:016x}\n")).collect();
            std::fs::write(&self.path, text).expect("write golden file");
            return;
        }
        assert!(self.failures.is_empty(), "{} fingerprint mismatches in {}:\n{}", self.failures.len(), self.path.display(), self.failures.join("\n"));
    }
}

/// Counts allocator calls (allocations, reallocations and frees) made on threads that set [`CountingAlloc::track`].
///
/// ```ignore
/// #[global_allocator]
/// static A: dreist_testkit::CountingAlloc = dreist_testkit::CountingAlloc;
/// let n = CountingAlloc::count(|| engine.process(&mut l, &mut r));
/// ```
pub struct CountingAlloc;

thread_local! {
    static TRACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Inside the tracer (its own allocations are not counted).
    static TRACING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Call sites seen with `ALLOC_TRACE=1` (innermost `dreist` frame → count).
static SITES: std::sync::Mutex<BTreeMap<String, usize>> = std::sync::Mutex::new(BTreeMap::new());

impl CountingAlloc {
    /// Allocator calls made by `f` on this thread.
    pub fn count<R>(f: impl FnOnce() -> R) -> (R, usize) {
        COUNT.with(|c| c.set(0));
        TRACK.with(|t| t.set(true));
        let r = f();
        TRACK.with(|t| t.set(false));
        (r, COUNT.with(|c| c.get()))
    }

    /// The traced call sites, most frequent first (`ALLOC_TRACE=1`).
    pub fn sites() -> Vec<(String, usize)> {
        let mut v: Vec<(String, usize)> = SITES.lock().map(|m| m.iter().map(|(k, v)| (k.clone(), *v)).collect()).unwrap_or_default();
        v.sort_by_key(|x| std::cmp::Reverse(x.1));
        v
    }

    /// [`CountingAlloc::sites`], forgetting them (to attribute sites to
    /// one call).
    pub fn take_sites() -> Vec<(String, usize)> {
        let v = Self::sites();
        if let Ok(mut m) = SITES.lock() {
            m.clear();
        }
        v
    }

    fn note(kind: &str) {
        // `try_with`: the allocator also runs during thread teardown.
        let tracked = TRACK.try_with(|t| t.get()).unwrap_or(false);
        if !tracked || TRACING.try_with(|t| t.get()).unwrap_or(true) {
            return;
        }
        let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        TRACING.with(|t| t.set(true));
        if let Some(mode) = std::env::var_os("ALLOC_TRACE") {
            let bt = std::backtrace::Backtrace::force_capture().to_string();
            // `ALLOC_TRACE=all`: every frame (symbol lines) up to the
            // nearest workspace one; otherwise the innermost workspace frames.
            let frames: Vec<&str> = if mode == "all" {
                let syms = bt.lines().map(str::trim).filter(|l| !l.starts_with("at ") && !l.contains("testkit") && !l.contains("backtrace") && !l.contains("alloc::alloc"));
                let mut v = Vec::new();
                for l in syms {
                    v.push(l);
                    if l.contains("dreist") || v.len() >= 12 {
                        break;
                    }
                }
                v
            } else {
                bt.lines().map(str::trim).filter(|l| l.contains("dreist") && !l.contains("testkit") && !l.contains("tests/")).take(3).collect()
            };
            if let Ok(mut m) = SITES.lock() {
                *m.entry(format!("{kind}: {}", frames.join(" <- "))).or_default() += 1;
            }
        }
        TRACING.with(|t| t.set(false));
    }
}

unsafe impl std::alloc::GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        Self::note("alloc");
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        Self::note("free");
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        Self::note("realloc");
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }
}
