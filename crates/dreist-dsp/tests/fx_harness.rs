//! Effect validation harness: every ported effect kind at several parameter
//! sets and sample rates is fed a fixed test signal (impulse, noise + sine
//! burst, silence) and must
//! - produce finite output, bounded in level,
//! - with every front-panel parameter at 0 (minimum feedback, resonance
//!   and time), fall silent after the input stops
//!   (except the freezer, which holds audio by design; at extreme settings
//!   resonant filters and feedback paths may self-oscillate, as in the
//!   original),
//! - render bit-identically to the stored fingerprints
//!   (`tests/golden/fx.txt`; `UPDATE_GOLDEN=1` rewrites them).

use dreist_dsp::fx::{Ctx, Effect, nodes};

use dreist_testkit::{Golden, fnv};

const KINDS: std::ops::RangeInclusive<usize> = 1..=47;

/// Effects that hold or loop audio on purpose (Audio Freezer).
const HOLDS_AUDIO: [usize; 1] = [38];

/// The Envelope Shaper's log-domain gains ([re:1800b5]) reach
/// +50 dB and more at its extreme attack / sustain / gain settings.
const EXTREME_GAIN: [usize; 1] = [12];

/// Deterministic test signal: an impulse, 0.4 s of noise plus a 220 Hz sine
/// at −6 dBFS, then silence; the right channel is offset so stereo paths
/// differ.
fn signal(sr: f32, seconds: f32) -> [Vec<f32>; 2] {
    let n = (sr * seconds) as usize;
    let burst = (sr * 0.4) as usize;
    let mut s = 0x1234_5678u32;
    let mut noise = move || {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (s >> 8) as f32 / 8_388_608.0 - 1.0
    };
    let mut l = vec![0.0; n];
    let mut r = vec![0.0; n];
    l[0] = 1.0;
    r[0] = 0.7;
    for i in 1..burst.min(n) {
        let t = i as f32 / sr;
        let sine = (std::f32::consts::TAU * 220.0 * t).sin();
        l[i] = 0.5 * (0.5 * noise() + 0.5 * sine);
        r[i] = 0.5 * (0.5 * noise() + 0.5 * (std::f32::consts::TAU * 330.0 * t).sin());
    }
    [l, r]
}

/// Parameter sets: the type defaults (as built), all 0, all 1, alternating
/// and two pseudo-random sets.
fn param_sets() -> Vec<(&'static str, Option<[f32; 12]>)> {
    let mut sets: Vec<(&'static str, Option<[f32; 12]>)> = vec![("default", None), ("zero", Some([0.0; 12])), ("one", Some([1.0; 12]))];
    sets.push(("alt", Some(std::array::from_fn(|i| if i % 2 == 0 { 1.0 } else { 0.0 }))));
    for (name, seed) in [("rand1", 7u32), ("rand2", 99u32)] {
        let mut s = seed;
        sets.push((
            name,
            Some(std::array::from_fn(|_| {
                s = s.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (s >> 16) as f32 / 65_536.0
            })),
        ));
    }
    sets
}

struct Run {
    out: [Vec<f32>; 2],
    peak: f32,
    /// Peak over the last 0.5 s.
    tail_peak: f32,
}

fn effect(kind: usize, sr: f32, params: Option<[f32; 12]>) -> Effect {
    nodes::reset_shared_rng();
    let mut fx = Effect::new(kind, sr);
    fx.mix = 1.0;
    if let Some(p) = params {
        for (i, v) in p.iter().enumerate() {
            fx.push_parameter(i, *v);
        }
    }
    fx
}

/// Render `seconds` (`None`: the input burst, the declared tail capped at
/// 30 s, 3 s to settle, then 0.5 s to measure).
fn run(kind: usize, sr: f32, params: Option<[f32; 12]>, block: usize, seconds: Option<f32>) -> Run {
    let mut fx = effect(kind, sr, params);
    let tail_s = fx.tail(&Ctx::new(sr)) as f32 / sr;
    let seconds = seconds.unwrap_or(0.4 + tail_s.min(30.0) + 3.5);
    let inp = signal(sr, seconds);
    let n = inp[0].len();
    let mut out = [vec![0.0; n], vec![0.0; n]];
    let ctx = Ctx::new(sr);
    let mut pos = 0;
    while pos < n {
        let end = (pos + block).min(n);
        let [ol, or] = &mut out;
        fx.process(&ctx, [&inp[0][pos..end], &inp[1][pos..end]], [&mut ol[pos..end], &mut or[pos..end]]);
        pos = end;
    }
    let all = || out[0].iter().chain(out[1].iter());
    let peak = all().fold(0.0f32, |m, x| if x.is_finite() { m.max(x.abs()) } else { f32::INFINITY });
    let tail = n - (sr * 0.5) as usize;
    let tail_iter = || out[0][tail..].iter().chain(out[1][tail..].iter());
    let tail_peak = tail_iter().fold(0.0f32, |m, x| m.max(x.abs()));
    Run { out, peak, tail_peak }
}

fn ported() -> impl Iterator<Item = usize> {
    KINDS.filter(|&k| nodes::is_ported(k))
}

#[test]
fn effects_stay_finite_bounded_and_decay() {
    let mut failures = Vec::new();
    for kind in ported() {
        for sr in [44_100.0f32, 96_000.0] {
            for (name, params) in param_sets() {
                let r = run(kind, sr, params, 256, None);
                let tag = format!("kind {kind} @{sr} {name}");
                if !r.peak.is_finite() {
                    failures.push(format!("{tag}: non-finite output"));
                    continue;
                }
                // The input peaks at 1 (impulse); gain stages reach +36 dB,
                // resonant EQs a little more: anything past 60 dB is a
                // blow-up.
                if r.peak > 1000.0 && !EXTREME_GAIN.contains(&kind) {
                    failures.push(format!("{tag}: peak {:.1}", r.peak));
                }
                if name == "zero" && !HOLDS_AUDIO.contains(&kind) && r.tail_peak > 1e-2 {
                    failures.push(format!("{tag}: still sounding {:.2e} after the input stopped", r.tail_peak));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{} problems:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn effects_survive_odd_and_large_blocks() {
    for kind in ported() {
        for block in [1usize, 31, 4096] {
            let r = run(kind, 44_100.0, Some([0.5; 12]), block, Some(1.0));
            assert!(r.peak.is_finite() && r.peak < 1000.0, "kind {kind} block {block}: peak {}", r.peak);
        }
    }
}

#[test]
fn effects_match_fingerprints() {
    let mut golden = Golden::open("fx");
    for kind in ported() {
        for (name, params) in param_sets().into_iter().filter(|(n, _)| matches!(*n, "default" | "one" | "rand1")) {
            let r = run(kind, 44_100.0, params, 256, Some(3.0));
            golden.check(&format!("kind{kind:02}-{name}"), fnv(r.out.iter().flatten()), &r.out[0]);
        }
    }
    golden.finish();
}

/// Render the test signal through `fx` (block 256).
fn render(fx: &mut Effect, sr: f32, seconds: f32) -> [Vec<f32>; 2] {
    let inp = signal(sr, seconds);
    let n = inp[0].len();
    let mut out = [vec![0.0; n], vec![0.0; n]];
    let ctx = Ctx::new(sr);
    let mut pos = 0;
    while pos < n {
        let end = (pos + 256).min(n);
        let [ol, or] = &mut out;
        fx.process(&ctx, [&inp[0][pos..end], &inp[1][pos..end]], [&mut ol[pos..end], &mut or[pos..end]]);
        pos = end;
    }
    out
}

/// Effects whose voice `Reset` leaves audio state running, so a panic keeps
/// their tails (as in the original): the base [re:1740cf] is empty (Urei 1176,
/// Shaped Distortion, Driver, Maul, the Breverbs); Bus Compressor zeroes
/// only its meter; Envelope Shaper resets its parameters; Drum Dynamics
/// its wet ramp; Chorus its modulation; Frequency Shifter its oscillator;
/// Reverb its wet ramp; Bloom clears only its delay buffer.
const RESET_KEEPS_TAILS: [usize; 15] = [8, 9, 12, 13, 17, 20, 21, 22, 25, 39, 40, 41, 42, 44, 45];

/// `Effect::panic` ([re:a63e3b]) forgets the audio in every
/// other effect: one that ran a loud burst and is then reset renders exactly
/// like one that ran silence for as long and was reset (control smoothers,
/// parameter state and the denormal offsets' phase match; nothing of the
/// burst is left in delay lines, filters, envelopes or followers).
#[test]
fn reset_forgets_the_audio() {
    let mut failures = Vec::new();
    for kind in ported().filter(|k| !RESET_KEEPS_TAILS.contains(k)) {
        for (name, params) in param_sets().into_iter().filter(|(n, _)| matches!(*n, "default" | "rand1")) {
            let sr = 44_100.0;
            let ctx = Ctx::new(sr);
            let junk: Vec<f32> = (0..(sr as usize)).map(|i| ((i * 7919) % 1000) as f32 / 500.0 - 1.0).collect();
            let silence = vec![0.0; junk.len()];
            let [a, b] = [&junk, &silence].map(|x| {
                let mut fx = effect(kind, sr, params);
                let mut o = [vec![0.0; x.len()], vec![0.0; x.len()]];
                let [ol, or] = &mut o;
                fx.process(&ctx, [x, x], [ol, or]);
                fx.panic();
                render(&mut fx, sr, 1.5)
            });
            if let Some(i) = a[0].iter().zip(&b[0]).position(|(x, y)| x.to_bits() != y.to_bits()) {
                failures.push(format!("kind {kind} {name}: differs at sample {i} ({} vs {})", a[0][i], b[0][i]));
            }
        }
    }
    assert!(failures.is_empty(), "{} problems:\n{}", failures.len(), failures.join("\n"));
}
