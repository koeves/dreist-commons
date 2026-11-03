//! Effect type → node construction ([re:c57d81]
//! [re:4f73e7]), hidden-port setup ([re:1160d2]
//! [re:141f96]) and front-panel → port mapping
//! (`GetEffectParameterIndexFromLayerParameterIndex` [re:15ee1b]).

use super::convert::time_beat_to_units;
use super::{Ctx, Node};

pub mod amber;
pub mod amp;
pub mod analog_filters;
pub mod audio_freeze;
pub mod audio_shaper;
pub mod auto_filter;
pub mod biquad;
pub mod bitcrusher;
pub mod blocks;
pub mod bloom;
pub mod breverb;
pub mod bus_comp;
pub mod chorus;
pub mod comb;
pub mod delay;
pub mod dirty_dac;
pub mod distortion;
pub mod drum_dynamics;
pub mod enhancer;
pub mod envelope_shaper;
pub mod flanger;
pub mod freq_shifter;
pub mod fxp_reverb;
pub mod guru;
pub mod hilo;
pub mod ladder;
pub mod limiter;
mod math;
pub mod maul;
pub mod mault;
// Generated table: kept as written.
#[rustfmt::skip]
mod mixbus_tables;
pub mod noise_gate;
pub mod phase_array;
pub mod phaser;
pub mod ring_mod;
pub mod ring_mod_analog;
pub mod ssl_eq;
pub mod ssl_filter;
pub mod stereo;
pub mod tin_can;
pub mod urei1176;
pub mod vari_eq;
pub mod vca_comp;

/// Build the node for an effect type; `None` = not ported yet (pass-through).
/// Reset the engine-wide random states (Breverb `_Randnext`, the
/// `Randomiser` / [re:b24a21] seed counters) before building an engine, so a
/// project renders the same every time it is loaded.
pub fn reset_shared_rng() {
    breverb::reset_randnext();
    blocks::reset_seeds();
}

pub fn create(kind: usize) -> Option<Box<dyn Node>> {
    // The shared lookup tables are built on first use: not on the audio thread.
    crate::analog::warm();
    let n: Box<dyn Node> = match kind {
        1 => Box::new(amp::Amp::new()),
        2 => Box::new(stereo::StereoMixer::new()),
        3 | 4 => Box::new(ssl_eq::SslEq::new()),
        5 => Box::new(vari_eq::VariEq::new()),
        6 => Box::new(hilo::HiLo::new()),
        7 => Box::new(enhancer::Enhancer::new()),
        8 => Box::new(urei1176::Urei1176::new()),
        9 => Box::new(bus_comp::BusComp::new()),
        10 => Box::new(vca_comp::VcaComp::new()),
        11 => Box::new(noise_gate::NoiseGate::new()),
        12 => Box::new(envelope_shaper::EnvelopeShaper::new()),
        13 => Box::new(drum_dynamics::DrumDynamics::new()),
        14 => Box::new(limiter::Limiter::new()),
        15 => Box::new(delay::Delay::new()),
        16 => Box::new(flanger::Flanger::new()),
        17 => Box::new(chorus::Chorus::new()),
        18 => Box::new(phaser::Phaser::new()),
        19 => Box::new(audio_shaper::AudioShaper::new()),
        20 => Box::new(mault::ShapedDistortion::new()),
        21 => Box::new(mault::Driver::new()),
        22 => Box::new(maul::Maul::new()),
        23 => Box::new(bitcrusher::BitCrusher::new()),
        24 => Box::new(ring_mod::RingMod::new()),
        25 => Box::new(freq_shifter::FreqShifter::new()),
        26 => Box::new(amber::Amber::new()),
        27 => Box::new(phase_array::PhaseArray::new()),
        28 => Box::new(ssl_filter::SslFilter::new()),
        29 => Box::new(auto_filter::AutoFilter::new()),
        30 => Box::new(comb::Comb::new()),
        31 => Box::new(ladder::Ladder::new(ladder::Kind::Tb303)),
        32 => Box::new(ladder::Ladder::new(ladder::Kind::Juno)),
        33 => Box::new(ladder::Ladder::new(ladder::Kind::Moog)),
        34 => Box::new(analog_filters::Ms20Filter::new()),
        35 => Box::new(ladder::Ladder::new(ladder::Kind::Prodigy)),
        36 => Box::new(analog_filters::Sh101Filter::new()),
        37 => Box::new(guru::Guru::new()),
        38 => Box::new(audio_freeze::AudioFreeze::new()),
        39 => Box::new(breverb::Breverb::new(breverb::Model::Hall)),
        40 => Box::new(breverb::Breverb::new(breverb::Model::Plate)),
        41 => Box::new(breverb::Breverb::new(breverb::Model::Room)),
        42 => Box::new(breverb::Breverb::new(breverb::Model::Inverse)),
        43 => Box::new(tin_can::TinCan::new()),
        44 => Box::new(fxp_reverb::FxpReverb::new()),
        45 => Box::new(bloom::Bloom::new()),
        46 => Box::new(dirty_dac::DirtyDac::new()),
        47 => Box::new(ring_mod_analog::RingModAnalog::new()),
        _ => return None,
    };
    Some(n)
}

pub fn is_ported(kind: usize) -> bool {
    matches!(kind, 1..=47)
}

/// `SetupInternalParameters` [re:141f96]: fixed values (normalized) for ports
/// the front panel does not expose.
pub fn setup_internal(kind: usize, node: &mut dyn Node) {
    let set: &[(usize, f32)] = match kind {
        3 => &[(3, 0.0), (7, 1.0), (11, 1.0), (15, 0.0), (16, 0.0), (17, 1.0)],
        4 => &[(3, 1.0), (7, 0.0), (11, 1.0), (15, 1.0), (0, 0.0), (12, 0.0), (1, 0.3), (13, 0.75), (8, 0.5), (16, 0.0), (17, 1.0)],
        6 | 25 | 46 => &[(2, 1.0)],
        7 | 17 | 24 => &[(3, 1.0)],
        8 => &[(7, 1.0), (8, 1.0), (3, 0.5)],
        9 => &[(5, 1.0), (6, 1.0), (7, 0.0)],
        10 => &[(5, 1.0), (7, 0.0)],
        11 => &[(8, 0.0), (9, 0.0), (4, 0.0), (5, 1.0)],
        12 | 14 | 27 => &[(0, 1.0)],
        15 => &[(1, 1.0), (7, 1.0), (4, 0.0)],
        16 => &[(6, 0.0), (7, 1.0)],
        18 => &[(6, 1.0), (7, 0.0), (8, 0.0)],
        19 => &[(7, 1.0), (4, 1.0)],
        20 => &[(11, 1.0), (5, 0.3), (9, 0.0), (10, 0.6), (1, 0.6)],
        23 => &[(8, 1.0), (5, 1.0), (6, 0.0)],
        26 => &[(5, 1.0), (0, 1.0)],
        28 | 47 => &[(4, 1.0)],
        29 => &[(5, 1.0), (6, 1.0), (7, 0.0), (11, 0.0)],
        31 | 32 | 34 | 35 => &[(4, 0.33262002), (6, 0.0), (5, 0.0)],
        33 => &[(4, 0.6010388), (5, 0.0), (6, 0.0)],
        36 => &[(5, 1.0), (4, 0.0)],
        38 => &[(3, 1.0), (8, 0.0), (7, 0.0), (9, 1.0)],
        39..=42 => &[(0, 1.0), (4, 0.5), (5, 0.5)],
        43 => &[(6, 1.0), (5, 0.0)],
        44 => &[(0, 1.0), (6, 0.5), (7, 0.5), (9, 0.5), (10, 0.5)],
        45 => &[(7, 1.0)],
        _ => &[],
    };
    let ports = node.ports();
    for &(i, v) in set {
        if let Some(p) = ports.get_mut(i) {
            p.set01(v);
        }
    }
}

/// Front-panel parameter index → node port (`params::effect_node_param`).
pub fn node_port(effect: usize, n: usize) -> usize {
    let t: &[usize] = match effect {
        3 => &[4, 5, 6, 8, 9, 10],
        4 => &[2, 10, 9, 14],
        8 => &[0, 1, 2, 5, 6],
        9 => &[0, 1, 2, 3, 4, 9],
        10 => &[0, 1, 2, 3, 4, 6],
        15 => &[0, 2, 5, 6, 3, 1],
        19 => &[0, 1, 2, 3, 6, 5],
        20 => &[3, 0, 6, 2, 4, 8],
        21 => &[2, 0, 1, 3, 4, 5],
        23 => &[0, 1, 2, 3, 4, 7],
        27 => &[1, 2, 3, 4, 5, 8],
        29 => &[1, 2, 3, 8, 9, 10],
        38 => &[0, 1, 2, 4, 5, 6],
        39..=42 => &[1, 2, 3, 6, 7, 8],
        44 => &[1, 2, 8, 3, 5, 4],
        45 => &[0, 3, 7, 11, 4, 12, 2, 13],
        47 => &[1, 0, 2, 3],
        12 | 14 => return n + 1,
        26 => return if n > 3 { n } else { n + 1 },
        _ => return n,
    };
    t.get(n).copied().unwrap_or(n)
}

/// `ValueToLeveldB` as `GetLengthOfTail` inlines it: `26.888343·ln(1.25·x)`
/// above `MappedLimit` (the double 0x3c9041ec40000000), else −1000.
fn value_to_level_db(x: f32) -> f32 {
    if x as f64 > f64::from_bits(0x3c90_41ec_4000_0000) { (x * 1.25).ln() * 26.888344 } else { -1000.0 }
}

/// Delay (15) and Bloom (45): the echo time (`ConvertToUnits(Time, Sync,
/// 60/bpm)·sr`) times one plus the number of repeats it takes the feedback
/// to fall to `kVoiceNoiseFloor` (5e-5), both in `ValueToLeveldB` terms.
fn feedback_tail(node: &dyn Node, ports: [usize; 3], ctx: &Ctx) -> f32 {
    let v = |i: usize| node.port_value(i).unwrap_or(0.0);
    let repeats = (value_to_level_db(5e-5) / value_to_level_db(v(ports[1]))).abs();
    let t = time_beat_to_units(v(ports[0]), v(ports[2]), 60.0 / ctx.bpm.max(1e-8)) * ctx.sample_rate;
    repeats * t + t
}

/// [re:9cc148] (samples), from the front-panel values
/// `p` and the node. Types not listed have no tail.
pub fn tail(kind: usize, p: &[f32; 12], node: &dyn Node, ctx: &Ctx) -> usize {
    let sr = ctx.sample_rate;
    let t = match kind {
        15 => feedback_tail(node, [0, 2, 1], ctx),
        45 => feedback_tail(node, [3, 7, 4], ctx),
        16 => sr * 0.01,
        17 => sr * 0.015,
        38 => (sr + sr) * p[0],
        39..=42 => (p[1] * 0.5 + p[0] * 19.400002 + 0.6) * sr,
        43 => (p[0] * 49.751003 + 0.24900001) * sr,
        _ => 0.0,
    };
    t.max(0.0) as usize
}
