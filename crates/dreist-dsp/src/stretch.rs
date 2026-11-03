//! Timestretch for `Timestretch` layers. The original drives DIRAC
//! ([re:e6eaa3], timestretch parameter update
//! [re:e9b827], source callback [re:5dc8dd]); the port substitutes
//! Signalsmith Stretch (MIT), so output differs from the original by design.
//! See the dreist-re voice notes ("Timestretch").

use signalsmith_stretch::Stretch;

/// Stretch factor (output duration / input duration) from the normalized
/// `TimestretchAmount`: `2v` below 0.5, `14v − 6` above (×1 at 0.5, ×8 at 1).
pub fn amount_factor(v: f32) -> f32 {
    if v < 0.5 { v + v } else { v * 14.0 - 6.0 }
}

pub struct Stretcher {
    s: Stretch,
    /// `Fast` mode (the preset it was built with).
    pub fast: bool,
    acc: f64,
    primed: bool,
    inter_in: Vec<f32>,
    inter_out: Vec<f32>,
}

impl Stretcher {
    /// Mode 0 `Default`, 1 `Fast` (the layer's timestretch mode).
    pub fn new(sample_rate: f32, fast: bool) -> Stretcher {
        let sr = sample_rate.max(1.0) as u32;
        let s = if fast { Stretch::preset_cheaper(2, sr) } else { Stretch::preset_default(2, sr) };
        let mut st = Stretcher { s, fast, acc: 0.0, primed: false, inter_in: Vec::new(), inter_out: Vec::new() };
        let n = crate::fx::NODE_BLOCK;
        st.inter_in.reserve(2 * st.max_input(n));
        st.inter_out.reserve(2 * n);
        st
    }

    /// Most source frames a block of `n` output frames can ask for (the
    /// first block's pre-roll at the slowest factor).
    pub fn max_input(&self, n: usize) -> usize {
        self.s.input_latency() + (n as f64 / 0.05).ceil() as usize + 1
    }

    /// Back to the state after `new` (a voice reuses its instance: no
    /// allocation at note start).
    pub fn reset(&mut self) {
        self.s.reset();
        self.acc = 0.0;
        self.primed = false;
    }

    /// Source frames needed for the next `n` output frames at `factor`
    /// (plus the input latency on the first block, used to pre-roll).
    pub fn input_needed(&mut self, n: usize, factor: f32) -> usize {
        let f = (factor as f64).clamp(0.05, 16.0);
        self.acc += n as f64 / f;
        let k = self.acc.floor();
        self.acc -= k;
        k as usize + if self.primed { 0 } else { self.s.input_latency() }
    }

    /// Stretch `input` (k frames) into `out` (n frames), transposing by `pitch`.
    pub fn process(&mut self, input: [&[f32]; 2], out: [&mut [f32]; 2], pitch: f32, factor: f32) {
        self.s.set_transpose_factor(pitch.max(1e-3), None);
        let mut k0 = 0;
        if !self.primed {
            let lat = self.s.input_latency().min(input[0].len());
            interleave(&mut self.inter_in, [&input[0][..lat], &input[1][..lat]]);
            self.s.seek(&self.inter_in, 1.0 / factor.clamp(0.05, 16.0) as f64);
            self.primed = true;
            k0 = lat;
        }
        let n = out[0].len();
        interleave(&mut self.inter_in, [&input[0][k0..], &input[1][k0..]]);
        self.inter_out.clear();
        self.inter_out.resize(n * 2, 0.0);
        self.s.process(&self.inter_in, &mut self.inter_out);
        let [l, r] = out;
        for i in 0..n {
            l[i] = self.inter_out[2 * i];
            r[i] = self.inter_out[2 * i + 1];
        }
    }
}

fn interleave(buf: &mut Vec<f32>, ch: [&[f32]; 2]) {
    buf.clear();
    for i in 0..ch[0].len() {
        buf.push(ch[0][i]);
        buf.push(ch[1][i]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts() {
        assert_eq!(amount_factor(0.5), 1.0);
        assert_eq!(amount_factor(1.0), 8.0);
        assert_eq!(amount_factor(0.25), 0.5);
    }

    #[test]
    fn stretches_a_sine_to_the_requested_length() {
        let sr = 44100.0;
        let src: Vec<f32> = (0..44100 * 2).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / sr).sin() * 0.5).collect();
        let mut st = Stretcher::new(sr, false);
        let (mut pos, mut energy, mut out_frames) = (0usize, 0.0f64, 0usize);
        let n = 512;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        for _ in 0..100 {
            let k = st.input_needed(n, 2.0);
            if pos + k > src.len() {
                break;
            }
            let chunk = &src[pos..pos + k];
            st.process([chunk, chunk], [&mut l, &mut r], 1.0, 2.0);
            pos += k;
            out_frames += n;
            energy += l.iter().map(|x| (*x as f64).powi(2)).sum::<f64>();
        }
        // Half-speed: consumed ≈ half the output frames (plus pre-roll).
        assert!((pos as f64) < out_frames as f64 * 0.5 + 8192.0);
        assert!(energy / out_frames as f64 > 0.01, "stretched output is audible");
    }
}

#[cfg(test)]
mod reproducibility {
    use super::*;

    /// Phase randomisation (above 2× stretch) is seeded, and a reset
    /// instance restarts the sequence: a reused stretcher renders like a
    /// fresh one.
    #[test]
    fn reused_stretcher_renders_like_a_fresh_one() {
        let sr = 44100.0;
        let src: Vec<f32> = (0..44100).map(|i| (i as f32 * 0.03).sin() * 0.5).collect();
        let run_on = |st: &mut Stretcher| {
            let (mut pos, mut out) = (0, Vec::new());
            let (mut l, mut r) = (vec![0.0; 32], vec![0.0; 32]);
            for _ in 0..200 {
                let k = st.input_needed(32, 7.3);
                let c = &src[pos..pos + k];
                st.process([c, c], [&mut l, &mut r], 1.0, 7.3);
                pos += k;
                out.extend_from_slice(&l);
            }
            out
        };
        for fast in [false, true] {
            let a = run_on(&mut Stretcher::new(sr, fast));
            let mut st = Stretcher::new(sr, fast);
            run_on(&mut st);
            st.reset();
            let b = run_on(&mut st);
            assert_eq!(a.iter().zip(&b).position(|(x, y)| x.to_bits() != y.to_bits()), None, "fast={fast}");
        }
    }
}
