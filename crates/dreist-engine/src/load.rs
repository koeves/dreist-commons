//! Audio load as the audio thread sees it, for an in-app readout: the time
//! each block takes against the block's own duration, averaged and at its
//! worst over each second, and blocks that start late (a dropout before
//! them). The audio thread only stores atomics; the editor reads the last
//! finished second.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use std::time::Instant;

/// One second of audio load (shares of real time, 1.0 = the whole block).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoadReading {
    /// Processing time over audio time, for the second.
    pub average: f32,
    /// The slowest block's time over the stream's period (the longest
    /// block).
    pub peak: f32,
    /// Blocks that started later than 1.5 periods (plus 2 ms) after the
    /// previous one, since the start (not in the first second).
    pub late: u32,
}

/// Shared between the audio thread ([`DspLoad::begin`] / [`DspLoad::end`])
/// and the editor ([`DspLoad::reading`]).
#[derive(Debug)]
pub struct DspLoad {
    base: Instant,
    /// Start of the block in progress and of the one before (ns after `base`).
    start: AtomicU64,
    prev_start: AtomicU64,
    /// The second being collected: processing and audio time (ns), the
    /// worst block (share in millionths).
    busy: AtomicU64,
    audio: AtomicU64,
    worst: AtomicU32,
    late: AtomicU32,
    /// The stream's period (ns): the longest block seen. Audio servers
    /// slip in short callbacks between full ones, so lateness and a block's
    /// share are judged against this, not each block's own length.
    period: AtomicU64,
    /// Audio time processed so far (ns): the first second's callback gaps
    /// (the stream starting) are not late blocks.
    total: AtomicU64,
    /// The last finished second (millionths), and how many seconds finished.
    average_out: AtomicU32,
    peak_out: AtomicU32,
    seconds: AtomicU32,
}

impl Default for DspLoad {
    fn default() -> DspLoad {
        DspLoad::new()
    }
}

impl DspLoad {
    pub fn new() -> DspLoad {
        DspLoad {
            base: Instant::now(),
            start: AtomicU64::new(0),
            prev_start: AtomicU64::new(0),
            busy: AtomicU64::new(0),
            audio: AtomicU64::new(0),
            worst: AtomicU32::new(0),
            late: AtomicU32::new(0),
            total: AtomicU64::new(0),
            period: AtomicU64::new(0),
            average_out: AtomicU32::new(0),
            peak_out: AtomicU32::new(0),
            seconds: AtomicU32::new(0),
        }
    }

    fn now(&self) -> u64 {
        self.base.elapsed().as_nanos() as u64
    }

    /// The audio thread, as a block starts.
    pub fn begin(&self) {
        let now = self.now().max(1);
        self.prev_start.store(self.start.load(Relaxed), Relaxed);
        self.start.store(now, Relaxed);
    }

    /// The audio thread, as the block of `samples` at `rate` ends; true
    /// when the block started late.
    pub fn end(&self, samples: usize, rate: f32) -> bool {
        let (start, prev) = (self.start.load(Relaxed), self.prev_start.load(Relaxed));
        if start == 0 || samples == 0 || rate <= 0.0 {
            return false;
        }
        let busy = self.now().saturating_sub(start);
        let audio = (samples as f64 * 1e9 / rate as f64) as u64;
        let warm = self.total.fetch_add(audio, Relaxed) >= 1_000_000_000;
        let period = self.period.fetch_max(audio, Relaxed).max(audio);
        let late = warm && prev != 0 && start - prev > period * 3 / 2 + 2_000_000;
        if late {
            self.late.fetch_add(1, Relaxed);
        }
        let share = (busy as f64 / period.max(1) as f64 * 1e6).min(u32::MAX as f64) as u32;
        self.worst.fetch_max(share, Relaxed);
        let b = self.busy.fetch_add(busy, Relaxed) + busy;
        let a = self.audio.fetch_add(audio, Relaxed) + audio;
        if a >= 1_000_000_000 {
            self.average_out.store((b as f64 / a as f64 * 1e6).min(u32::MAX as f64) as u32, Relaxed);
            self.peak_out.store(self.worst.swap(0, Relaxed), Relaxed);
            self.busy.store(0, Relaxed);
            self.audio.store(0, Relaxed);
            self.seconds.fetch_add(1, Relaxed);
        }
        late
    }

    /// [`DspLoad::begin`] now and [`DspLoad::end`] when the guard drops (a
    /// `process` with several returns).
    pub fn scope(&self, samples: usize, rate: f32) -> LoadScope<'_> {
        self.begin();
        LoadScope { load: self, samples, rate }
    }

    /// The stream's period so far (the longest block, ns).
    pub fn period_ns(&self) -> u64 {
        self.period.load(Relaxed)
    }

    /// The last finished second; `None` before the first.
    pub fn reading(&self) -> Option<LoadReading> {
        if self.seconds.load(Relaxed) == 0 {
            return None;
        }
        Some(LoadReading { average: self.average_out.load(Relaxed) as f32 / 1e6, peak: self.peak_out.load(Relaxed) as f32 / 1e6, late: self.late.load(Relaxed) })
    }

    /// Seconds finished so far (a reading is new when this changes).
    pub fn seconds(&self) -> u32 {
        self.seconds.load(Relaxed)
    }
}

/// Ends a block when dropped (see [`DspLoad::scope`]).
pub struct LoadScope<'a> {
    load: &'a DspLoad,
    samples: usize,
    rate: f32,
}

impl Drop for LoadScope<'_> {
    fn drop(&mut self) {
        let _ = self.load.end(self.samples, self.rate);
    }
}

/// The process's resident memory in bytes, where the platform tells.
pub fn rss_bytes() -> Option<u64> {
    rss::bytes()
}

#[cfg(target_os = "linux")]
mod rss {
    pub fn bytes() -> Option<u64> {
        let s = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = s.lines().find(|l| l.starts_with("VmRSS:"))?;
        let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kb * 1024)
    }
}

#[cfg(target_os = "macos")]
mod rss {
    /// `mach_task_basic_info` (`MACH_TASK_BASIC_INFO` = 20).
    #[repr(C)]
    #[derive(Default)]
    struct BasicInfo {
        virtual_size: u64,
        resident_size: u64,
        resident_size_max: u64,
        user_time: [i32; 2],
        system_time: [i32; 2],
        policy: i32,
        suspend_count: i32,
    }

    unsafe extern "C" {
        static mach_task_self_: u32;
        fn task_info(task: u32, flavor: i32, info: *mut BasicInfo, count: *mut u32) -> i32;
    }

    pub fn bytes() -> Option<u64> {
        let mut info = BasicInfo::default();
        let mut count = (std::mem::size_of::<BasicInfo>() / 4) as u32;
        // SAFETY: `info` is a `mach_task_basic_info` and `count` its size in
        // 32-bit words, as `task_info` expects for flavour 20.
        let r = unsafe { task_info(mach_task_self_, 20, &mut info, &mut count) };
        (r == 0).then_some(info.resident_size)
    }
}

#[cfg(target_os = "windows")]
mod rss {
    /// `PROCESS_MEMORY_COUNTERS`.
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }

    pub fn bytes() -> Option<u64> {
        let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, ..Counters::default() };
        // SAFETY: `c` is a `PROCESS_MEMORY_COUNTERS` with `cb` set to its size.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
        (ok != 0).then_some(c.working_set_size as u64)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod rss {
    pub fn bytes() -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_of_blocks_gives_a_reading() {
        let l = DspLoad::new();
        assert_eq!(l.reading(), None);
        for _ in 0..44 {
            l.begin();
            std::thread::sleep(std::time::Duration::from_millis(1));
            l.end(1024, 44100.0);
        }
        let r = l.reading().expect("a second of audio");
        assert!(r.average > 0.02 && r.average < 1.0, "{r:?}");
        assert!(r.peak >= r.average, "{r:?}");
        assert_eq!(l.seconds(), 1);
    }

    #[test]
    fn a_late_block_counts() {
        let l = DspLoad::new();
        // Past the first second of audio (the stream starting).
        l.total.store(1_000_000_000, Relaxed);
        l.begin();
        l.end(64, 48000.0);
        std::thread::sleep(std::time::Duration::from_millis(10));
        l.begin();
        l.end(64, 48000.0);
        assert_eq!(l.late.load(Relaxed), 1);
    }

    #[test]
    fn short_callbacks_between_full_ones_are_not_late() {
        let l = DspLoad::new();
        l.total.store(1_000_000_000, Relaxed);
        // A full period (21 ms), a 5-frame callback, the next full period
        // a period later: none late, and the short one's share is of the period.
        l.begin();
        l.end(1024, 48000.0);
        l.begin();
        assert!(!l.end(5, 48000.0));
        std::thread::sleep(std::time::Duration::from_millis(21));
        l.begin();
        assert!(!l.end(1024, 48000.0));
        assert!(l.worst.load(Relaxed) < 500_000, "{}", l.worst.load(Relaxed));
    }

    /// The stream's period (ns), for judging a block's share.
    #[test]
    fn the_period_is_the_longest_block() {
        let l = DspLoad::new();
        l.begin();
        l.end(1024, 48000.0);
        l.begin();
        l.end(5, 48000.0);
        assert_eq!(l.period_ns(), (1024.0 * 1e9 / 48000.0) as u64);
    }

    #[test]
    fn this_process_has_resident_memory() {
        if cfg!(any(target_os = "linux", target_os = "macos", target_os = "windows")) {
            assert!(rss_bytes().is_some_and(|b| b > 1 << 20));
        }
    }
}
