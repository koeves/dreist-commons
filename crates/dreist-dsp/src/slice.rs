//! Slice analysis ([re:a38083].., [re:cba966]..,
//! `SlicePoint`, [re:462ae1]): spectral-flux onset detection with an
//! adaptive threshold, equal-length chopping, sensitivity filtering and zero
//! crossings. See the dreist-re slicing notes.

use rustfft::{FftPlanner, num_complex::Complex32};

/// [re:83de95].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransientPoint {
    pub pos: i32,
    pub strength: f32,
    pub dyn_sense: f32,
    pub mean: f32,
}

/// [re:a11c15] parameters (setup; values from the slicing call).
#[derive(Debug, Clone, Copy)]
pub struct Detector {
    /// Mean multiplier.
    pub threshold: f32,
    /// FFT size.
    pub fft: usize,
    /// Hop in % of the FFT size.
    pub overlap_pct: f32,
    /// Running-mean window (frames).
    pub mean_window: usize,
    /// Mean floor.
    pub min_mean: f32,
    /// Dynamic-threshold multiplier.
    pub dyn_mult: f32,
    /// Dynamic-threshold decay per sample.
    pub decay: f32,
    /// Peak window and onset look-back (frames).
    pub peak_window: usize,
    pub look_back: usize,
}

impl Default for Detector {
    /// [re:c5ff4b]: setup with (1.0, 1024, 12.5, 100,
    /// 0.0025, 3.0, 0.0014); peak window 5 and look-back 6 at construction.
    fn default() -> Detector {
        Detector { threshold: 1.0, fft: 1024, overlap_pct: 12.5, mean_window: 100, min_mean: 0.0025, dyn_mult: 3.0, decay: 0.0014, peak_window: 5, look_back: 6 }
    }
}

impl Detector {
    fn hop(&self) -> usize {
        ((self.fft as f32 * self.overlap_pct * 0.01).floor() as usize).max(1)
    }

    /// Onset detection [re:6451fb]: half-wave rectified spectral flux of
    /// Hann-windowed frames (stereo averaged), frames start one FFT early.
    pub fn onset_function(&self, left: &[f32], right: Option<&[f32]>) -> Vec<f32> {
        let n = self.fft;
        let hop = self.hop();
        let frames = (left.len() + n * 2) / hop;
        let window: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (i as f32 * std::f32::consts::TAU / (n - 1) as f32).cos()).collect();
        let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
        let bins = n / 2 + 1;
        let mut prev = vec![0.0f32; bins];
        let mut cur = vec![0.0f32; bins];
        let mut buf = vec![Complex32::new(0.0, 0.0); n];
        let mut out = Vec::with_capacity(frames);
        for f in 0..frames {
            let start = (hop * f) as i64 - n as i64;
            for i in 0..n {
                let k = start + i as i64;
                let s = if k < 0 || k as usize >= left.len() {
                    0.0
                } else {
                    match right {
                        Some(r) => (left[k as usize] + r[k as usize]) * 0.5,
                        None => left[k as usize],
                    }
                };
                buf[i] = Complex32::new(s * window[i], 0.0);
            }
            fft.process(&mut buf);
            let mut sum = 0.0;
            for k in 0..bins {
                cur[k] = buf[k].norm();
                let d = cur[k] - prev[k];
                sum += (d.abs() + d) * 0.5;
            }
            std::mem::swap(&mut cur, &mut prev);
            out.push(sum);
        }
        out
    }

    /// Onset positioning [re:707fa1]: argmax of the third difference near `u`.
    fn position(&self, odf: &[f32], u: usize) -> usize {
        let lo = u.saturating_sub(self.look_back);
        let hi = (u + 2).min(odf.len().saturating_sub(1));
        if hi <= lo || hi - lo <= 2 {
            return lo;
        }
        let mut d: Vec<f32> = odf[lo..hi].to_vec();
        for _ in 0..3 {
            d = d.windows(2).map(|w| w[1] - w[0]).collect();
        }
        let arg = d.iter().enumerate().fold((0, f32::MIN), |b, (i, v)| if *v > b.1 { (i, *v) } else { b }).0;
        (u + 1 + arg).saturating_sub(self.look_back)
    }

    /// [re:79ac65] + peak picking [re:0a87af].
    pub fn detect(&self, left: &[f32], right: Option<&[f32]>) -> Vec<TransientPoint> {
        let hop = self.hop();
        let mut odf = self.onset_function(left, right);
        let max = odf.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        if max >= 1e-6 {
            odf.iter_mut().for_each(|v| *v /= max);
        }
        let w = self.mean_window.max(1);
        let n = odf.len();
        let mut ring = vec![0.0f32; w];
        let mut since = 1000i32;
        let mut dyn_thr = 0.0f32;
        let mut out = Vec::new();
        for t in 0..(w / 2 + n) {
            ring[t % w] = if t < n { odf[t] } else { 0.0 };
            let mean = ring.iter().sum::<f32>() / w as f32;
            if t >= w / 2 {
                let u = t - w / 2;
                let decay = (-((since - 1) as f32 * self.decay * hop as f32)).exp();
                let m = mean.max(self.min_mean);
                if m * self.threshold + decay * dyn_thr < odf[u] {
                    let lo = u.saturating_sub(self.peak_window);
                    let hi = (u + self.peak_window).min(n - 1);
                    let local = odf[lo..hi.max(lo)].iter().fold(f32::MIN, |a, v| a.max(*v)).max(0.0);
                    let dyn_sense = decay * dyn_thr;
                    dyn_thr = local * self.dyn_mult;
                    let pos = (self.position(&odf, u) * hop) as i32;
                    out.push(TransientPoint { pos: pos.max(0), strength: dyn_thr / self.dyn_mult, dyn_sense, mean });
                    since = 0;
                }
            }
            since += 1;
        }
        out
    }
}

/// `SlicePoint` (saved by [re:567a97] as `Start Locked Deleted Pos Strength
/// DynSense Mean`; manual slices keep −50 in the three analysis fields).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlicePoint {
    pub start: u32,
    pub pos: i32,
    pub locked: bool,
    pub deleted: bool,
    pub strength: f32,
    pub dyn_sense: f32,
    pub mean: f32,
}

impl SlicePoint {
    pub fn manual(start: u32, locked: bool) -> SlicePoint {
        SlicePoint { start, pos: start as i32, locked, deleted: false, strength: -50.0, dyn_sense: -50.0, mean: -50.0 }
    }

    /// Usable at a sensitivity [re:ba60a9], with the onset evaluation [re:4b4787].
    pub fn usable_at(&self, threshold: f32) -> bool {
        if self.locked {
            return true;
        }
        let manual = (self.strength + 50.0).abs() < 1e-6 && (self.dyn_sense + 50.0).abs() < 1e-6 && (self.mean + 50.0).abs() < 1e-6;
        if manual {
            return true;
        }
        self.mean.max(0.0025) * threshold + self.dyn_sense < self.strength
    }
}

/// Threshold for a 0..1 sensitivity ([re:1500d9]): `(1 − s^0.6)·29 + 0.5`.
pub fn sensitivity_threshold(sensitivity: f32) -> f32 {
    (1.0 - sensitivity.powf(0.6)) * 29.0 + 0.5
}

/// [re:3c8117]: detected transients inside the
/// buffer become unlocked slices carrying their analysis values, sorted.
pub fn analyze(left: &[f32], right: Option<&[f32]>) -> Vec<SlicePoint> {
    let mut v: Vec<SlicePoint> = Detector::default()
        .detect(left, right)
        .into_iter()
        .filter(|p| (p.pos as usize) < left.len())
        .map(|p| SlicePoint { start: p.pos as u32, pos: p.pos, locked: false, deleted: false, strength: p.strength, dyn_sense: p.dyn_sense, mean: p.mean })
        .collect();
    v.sort_by_key(|s| s.start);
    v
}

/// [re:751b5c]: `n` equal slices over the
/// region (`step = ceil(len/n)`).
pub fn chop(start: u32, len: u32, n: f32) -> Vec<SlicePoint> {
    let step = ((len as f32 / n).ceil() as u32).max(1);
    let mut v = Vec::new();
    let mut p = start;
    loop {
        v.push(SlicePoint::manual(p, false));
        p += step;
        if p >= start + len {
            break;
        }
    }
    v
}

/// Slices usable at `sensitivity`, sorted.
pub fn for_sensitivity(slices: &[SlicePoint], sensitivity: f32) -> Vec<SlicePoint> {
    let thr = sensitivity_threshold(sensitivity);
    let mut v: Vec<SlicePoint> = slices.iter().filter(|s| s.usable_at(thr)).copied().collect();
    v.sort_by_key(|s| s.start);
    v
}

/// [re:682495] (either channel crossing).
pub fn zero_crossings(left: &[f32], right: Option<&[f32]>) -> Vec<u32> {
    let cross = |a: f32, b: f32| (a >= 0.0 && b < 0.0) || (a < 0.0 && b >= 0.0);
    (1..left.len()).filter(|&i| cross(left[i - 1], left[i]) || right.is_some_and(|r| cross(r[i - 1], r[i]))).map(|i| i as u32).collect()
}

/// [re:43fb4b]: a slice spans to
/// the next slice (the last one to the region end).
pub fn slice_markers(slices: &[SlicePoint], i: usize, region_end: u32) -> (u32, u32) {
    let end = slices.get(i + 1).map(|s| s.start).unwrap_or(region_end);
    (slices[i].start, end)
}

/// One sliced hit placed by the pattern extraction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliceHit {
    pub slice: usize,
    pub pad: usize,
    pub layer: usize,
    pub step: usize,
    /// Shift lane (0.5 = on the grid).
    pub shift: f32,
    /// ForceLayer lane (normalized layer index; 0 single-layer).
    pub force_layer: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SlicedPattern {
    pub steps: u32,
    pub hits: Vec<SliceHit>,
}

/// [re:4f7f22].
pub fn layer_index_value(layer: usize) -> f32 {
    ((layer + 1) as f32 * 0.125 + 0.05).min(1.0)
}

/// [re:183c6d] for unlocked pads:
/// usable slices inside `[start, end)` go to consecutive pads (and layers
/// when `multi_layer`), on the nearest 16th of a loop `bars` long (or
/// `beats/4` bars when `bars < 1`), with the sub-step offset in Shift (0.5
/// when `quantize`, option `AllocatorQuantizeREX`). Steps = `bars·16`, grown
/// in 16s (or `round(4·beats)`) to fit the last slice, at most 1024.
pub fn extract_pattern(slices: &[SlicePoint], start: u32, end: u32, bars: f32, beats: f32, multi_layer: bool, quantize: bool) -> SlicedPattern {
    extract_pattern_with_locks(slices, start, end, bars, beats, multi_layer, quantize, &|_, _| false)
}

/// [`extract_pattern`] skipping pads whose pad, layer or track is locked
/// (`blocked(pad, layer)`, `AssignSampleToPadNotUsingSliceType{Single,
/// Multiple}Layers` [re:505fcb] / [re:49bc92]: the next free pad; with multiple
/// layers, after pad 63 the next layer restarts at pad 0 unchecked).
#[allow(clippy::too_many_arguments)]
pub fn extract_pattern_with_locks(slices: &[SlicePoint], start: u32, end: u32, bars: f32, beats: f32, multi_layer: bool, quantize: bool, blocked: &dyn Fn(usize, usize) -> bool) -> SlicedPattern {
    let len = end.saturating_sub(start).max(1);
    let frac_bar = bars < 1.0 && beats >= 1.0;
    let e = if frac_bar { beats as f64 * 0.25 } else { 1.0 } / len as f64;
    let loop_bars = (bars as f64).max(1.0);
    let g = 0.0625 / loop_bars;
    let usable = |s: &SlicePoint| !s.deleted && start <= s.start && s.start < end;
    let allocatable = slices.iter().filter(|s| !s.deleted && start <= s.start && s.start <= end).count().min(if multi_layer { 512 } else { 64 });
    // Nearest grid index and shift for a loop position d (0..1).
    let place = |d: f64| -> Option<(i64, f32)> {
        let k = loop_bars * 16.0;
        let (mut t, mut f) = (0.0f64, 0i64);
        let mut best: Option<(i64, f32)> = None;
        loop {
            if t == d {
                return Some((f, 0.5));
            }
            let t2 = t + g;
            if t2 == d {
                return Some((f + 1, 0.5));
            }
            if d < t2 && t - g < d {
                best = Some(if t <= d {
                    if t2 - d < d - t { (f + 1, 0.5 - ((t2 - d) * k) as f32 * 0.5) } else { (f, ((d - t) * k) as f32 * 0.5 + 0.5) }
                } else {
                    let dd = d - (t - g);
                    if t - d < dd { (f, 0.5 - ((t - d) * k) as f32 * 0.5) } else { (f - 1, (dd * k) as f32 * 0.5 + 0.5) }
                });
            }
            f += 1;
            t = t2;
            if t2 >= 1.0 {
                return best;
            }
        }
    };
    let mut last = -1i64;
    for s in slices.iter().filter(|s| usable(s)) {
        if let Some((i, _)) = place((s.start - start) as f64 * e) {
            if i <= 0x3ff {
                last = last.max(i);
            }
        }
    }
    let mut steps = (bars.floor() as i64).max(1) * 16;
    if last < steps {
        let unit = if frac_bar { ((beats * 4.0).round() as i64).max(4) } else { 16 };
        let mut v = last;
        while v <= 0x3ff && v % unit != 0 {
            v += 1;
        }
        steps = v;
    }
    let steps = steps.max(1) as u32;
    let mut hits = Vec::new();
    let (mut pad, mut layer) = (-1i64, -1i64);
    let mut taken = 0;
    for (si, s) in slices.iter().enumerate() {
        if taken >= allocatable {
            break;
        }
        if !usable(s) {
            continue;
        }
        taken += 1;
        let Some((mut idx, mut shift)) = place((s.start - start) as f64 * e) else { continue };
        if idx >= steps as i64 {
            shift = 1.0 - shift;
            idx -= 1;
        }
        if !(0..0x400).contains(&idx) {
            continue;
        }
        // AssignSampleToPadNotUsingSliceType{Single,Multiple}Layers.
        pad += 1;
        let check_layer = if multi_layer { layer.max(0) as usize } else { 0 };
        while pad < 64 && blocked(pad as usize, check_layer) {
            pad += 1;
        }
        let force;
        if pad >= 64 {
            if !multi_layer {
                continue;
            }
            layer += 1;
            if layer > 7 {
                continue;
            }
            pad = 0;
            force = layer_index_value(layer as usize);
        } else if multi_layer {
            if layer < 0 {
                layer = 0;
            }
            force = layer_index_value(layer as usize);
        } else {
            force = 0.0;
        }
        hits.push(SliceHit {
            slice: si,
            pad: pad as usize,
            layer: if multi_layer { layer.max(0) as usize } else { 0 },
            step: idx as usize,
            shift: if quantize { 0.5 } else { shift },
            force_layer: force,
        });
    }
    SlicedPattern { steps, hits }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clicks(sr: usize, at: &[usize]) -> Vec<f32> {
        let mut v = vec![0.0f32; sr * 2];
        for &p in at {
            for i in 0..2000 {
                let env = (-(i as f32) / 300.0).exp();
                v[p + i] += (i as f32 * 0.3).sin() * env;
            }
        }
        v
    }

    #[test]
    fn detects_clear_hits_near_their_onsets() {
        let hits = [11025, 33075, 55125, 77175];
        let s = analyze(&clicks(44100, &hits), None);
        for h in hits {
            assert!(s.iter().any(|p| (p.start as i64 - h as i64).abs() < 512), "hit {h} in {:?}", s.iter().map(|p| p.start).collect::<Vec<_>>());
        }
        // High sensitivity keeps them; zero sensitivity drops weak ones.
        assert!(for_sensitivity(&s, 1.0).len() >= hits.len());
    }

    #[test]
    fn extract_places_slices_on_sixteenths() {
        // One bar of 16 equal slices → pads 0..15 on steps 0..15, on the grid.
        let len = 16 * 1000;
        let sl = chop(0, len, 16.0);
        let p = extract_pattern(&sl, 0, len, 1.0, 0.0, false, false);
        assert_eq!(p.steps, 16);
        assert_eq!(p.hits.len(), 16);
        for (i, h) in p.hits.iter().enumerate() {
            assert_eq!((h.pad, h.step), (i, i));
            assert!((h.shift - 0.5).abs() < 1e-6);
        }
        // A slice a quarter-16th late shifts right of the grid.
        let late = [SlicePoint::manual(250, false)];
        let q = extract_pattern(&late, 0, len, 1.0, 0.0, false, false);
        assert_eq!(q.hits[0].step, 0);
        assert!((q.hits[0].shift - 0.625).abs() < 1e-4);
        assert_eq!(slice_markers(&sl, 15, len), (15000, len));
    }

    #[test]
    fn chop_and_crossings() {
        let c = chop(100, 1000, 4.0);
        assert_eq!(c.iter().map(|s| s.start).collect::<Vec<_>>(), vec![100, 350, 600, 850]);
        assert_eq!(zero_crossings(&[1.0, -1.0, -0.5, 0.5], None), vec![1, 3]);
        assert_eq!(sensitivity_threshold(1.0), 0.5);
    }
}
