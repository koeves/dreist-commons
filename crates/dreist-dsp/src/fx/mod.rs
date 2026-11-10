//! Effects: the [re:f2c879] node framework ([re:102308] ports, [re:a199c0]),
//! `Effect` (trim → node → wet/dry → level) and the node DSP ported one by
//! one. See the dreist-re effects notes.

pub mod convert;
pub mod display;
pub mod nodes;
// Generated table: kept as written.
#[rustfmt::skip]
pub mod ports;

use convert::Conv;
use ports::PortDef;

/// [re:74a6d3] values the nodes read.
#[derive(Debug, Clone, Copy)]
pub struct Ctx {
    pub sample_rate: f32,
    pub bpm: f32,
    /// Song position in quarter notes at the start of the block.
    pub ppq: f64,
    pub playing: bool,
    /// [re:c912bd] (tuning, 440 Hz).
    pub tune: f32,
}

impl Ctx {
    pub fn new(sample_rate: f32) -> Ctx {
        Ctx { sample_rate, bpm: 120.0, ppq: 0.0, playing: true, tune: 440.0 }
    }
}

/// [re:5dc2fd]: value = `min + (max − min)·v01` in the converter's domain.
/// Port values copied into a fixed array (no allocation on the audio
/// path); node tables have at most 18 ports.
pub fn port_values(ports: &[Port]) -> [f32; 32] {
    std::array::from_fn(|i| ports.get(i).map_or(0.0, |p| p.value))
}

#[derive(Debug, Clone, Copy)]
pub struct Port {
    pub min: f64,
    pub max: f64,
    pub value: f32,
}

impl Port {
    pub fn new(default: f32, min: f32, max: f32) -> Port {
        Port { min: min as f64, max: max as f64, value: default }
    }

    /// String-form bounds go through the converter; float-form bounds are
    /// internal values. Unresolvable bounds (enum counts) parse as 0 and are
    /// fixed up by the node.
    pub fn from_def(d: &PortDef) -> Port {
        let (_, def, lo, hi, conv, text) = *d;
        let c = if conv == Conv::Custom { Conv::Float } else { conv };
        let v = |s: &str| if text { c.from_text(s) } else { s.parse::<f32>().unwrap_or(0.0) };
        Port::new(v(def), v(lo), v(hi))
    }

    /// Normalized value set [re:6bb8cb].
    pub fn set01(&mut self, v: f32) {
        let v = v.clamp(0.0, 1.0) as f64;
        self.value = ((self.max - self.min) * v + self.min) as f32;
    }
}

pub fn ports_from(defs: &[PortDef]) -> Vec<Port> {
    defs.iter().map(Port::from_def).collect()
}

/// Largest block a node is handed (the engine's 32-frame sub-block).
pub const NODE_BLOCK: usize = 32;

/// A per-block work buffer: holds a [`NODE_BLOCK`] without reallocating.
pub fn scratch() -> Vec<f32> {
    Vec::with_capacity(NODE_BLOCK)
}

pub trait Node: Send {
    fn ports(&mut self) -> &mut Vec<Port>;
    /// One block of at most [`NODE_BLOCK`] frames (longer ones work but may
    /// grow the work buffers); no allocation once `prepare_rate` ran.
    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]);
    /// The voice's reset ([re:b33786] calls it on every voice, from
    /// [re:404233] and voice creation): whatever running state the
    /// original class clears, without allocating. The base
    /// [re:0363ae] is empty, so tails survive a panic in
    /// nodes that do not override it.
    fn reset(&mut self) {}
    /// Size the rate-dependent buffers up front (what the first `process`
    /// at this rate would do), keeping allocation off the audio thread.
    fn prepare_rate(&mut self, _sample_rate: f32) {}
    /// A control port's value without `&mut` ([re:875910]
    /// [re:294c79]), for the nodes whose tail length depends on their ports.
    fn port_value(&self, _index: usize) -> Option<f32> {
        None
    }
    /// A control output ([re:414b35]: the effect's out
    /// parameter, e.g. a compressor's gain-reduction meter, 1 = none).
    fn output(&self, _index: usize) -> Option<f32> {
        None
    }
}

/// `Gain` [re:d073e0] used for the effect trim: linear target ramped per sample.
#[derive(Debug, Clone, Copy)]
struct Trim {
    last: f32,
}

impl Trim {
    fn process(&mut self, target: f32, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let n = inp[0].len();
        let inc = (target - self.last) * (1.0 / n as f32);
        let [ol, or] = out;
        if inc.abs() < 1e-6 {
            for i in 0..n {
                ol[i] = inp[0][i] * self.last;
                or[i] = inp[1][i] * self.last;
            }
            return;
        }
        for i in 0..n {
            self.last += inc;
            ol[i] = inp[0][i] * self.last;
            or[i] = inp[1][i] * self.last;
        }
    }
}

/// `Effect` ([re:384686]): one slot of an effect chain.
pub struct Effect {
    pub kind: usize,
    pub node: Option<Box<dyn Node>>,
    trim: Trim,
    /// Trimmed input of the current sub-block.
    buf: [[f32; NODE_BLOCK]; 2],
    /// Values for this block.
    pub active: bool,
    pub mix: f32,
    pub level: f32,
    pub trim_target: f32,
    /// Last pushed front-panel values.
    pub params: [f32; 12],
}

/// The 2^x the effect nodes and the voice filter cores use for their
/// parameter conversions (cutoffs, pitch, times, gains): `f32::exp2` unless the app installs another one
/// ([`install_exp2`]; Einst's is the original v1's fast approximation).
#[inline]
pub fn exp2(x: f32) -> f32 {
    match EXP2.get() {
        Some(f) => f(x),
        None => x.exp2(),
    }
}

static EXP2: std::sync::OnceLock<fn(f32) -> f32> = std::sync::OnceLock::new();

/// Install the app's 2^x for [`exp2`] (once, at start-up; later calls are
/// ignored and return `false`).
pub fn install_exp2(f: fn(f32) -> f32) -> bool {
    EXP2.set(f).is_ok()
}

/// An app's own effect nodes: `create(kind)` returns the node for an
/// effect type it implements differently (`None`: the built-in node), and
/// `internal_ports(kind)` the hidden ports to set on it, as `(port, value
/// 0..1)`.
#[derive(Clone, Copy)]
pub struct NodeFactory {
    pub create: fn(usize) -> Option<Box<dyn Node>>,
    pub internal_ports: fn(usize) -> &'static [(usize, f32)],
}

static FACTORY: std::sync::OnceLock<NodeFactory> = std::sync::OnceLock::new();

/// Install the app's node factory (once, at start-up; later calls are
/// ignored and return `false`).
pub fn install_factory(f: NodeFactory) -> bool {
    FACTORY.set(f).is_ok()
}

impl Effect {
    pub fn new(kind: usize, sample_rate: f32) -> Effect {
        // The app's own node for this type (see [`install_factory`]), else
        // the built-in one; then the hidden ports and the rate.
        let custom = FACTORY.get().and_then(|f| (f.create)(kind).map(|n| (n, (f.internal_ports)(kind))));
        let mut node = match custom {
            Some((mut n, set)) => {
                for &(i, v) in set {
                    if let Some(p) = n.ports().get_mut(i) {
                        p.set01(v);
                    }
                }
                Some(n)
            }
            None => nodes::create(kind).map(|mut n| {
                nodes::setup_internal(kind, n.as_mut());
                n
            }),
        };
        if let Some(n) = node.as_mut() {
            n.prepare_rate(sample_rate);
        }
        Effect { kind, node, trim: Trim { last: 1.0 }, buf: [[0.0; NODE_BLOCK]; 2], active: true, mix: 0.5, level: 1.0, trim_target: 1.0, params: [0.0; 12] }
    }

    /// [re:967f49] → [re:330ad9] →
    /// [re:572a93]: each voice's own reset ([`Node::reset`]),
    /// in place on the audio thread.
    pub fn panic(&mut self) {
        if let Some(n) = self.node.as_mut() {
            n.reset();
        }
    }

    /// Front-panel parameter → node port(s).
    pub fn push_parameter(&mut self, index: usize, v01: f32) {
        if let Some(p) = self.params.get_mut(index) {
            *p = v01;
        }
        let kind = self.kind;
        if let Some(n) = self.node.as_mut() {
            let p = nodes::node_port(kind, index);
            if let Some(port) = n.ports().get_mut(p) {
                port.set01(v01);
            }
        }
    }

    /// [re:f32c07] for stereo. The node runs in sub-blocks of at
    /// most [`NODE_BLOCK`] frames (the engine's own sub-block): several
    /// nodes ramp their controls linearly across a block and overshoot on
    /// longer ones.
    pub fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let [ol, or] = out;
        let mut pos = 0;
        while pos < inp[0].len() {
            let end = (pos + NODE_BLOCK).min(inp[0].len());
            self.process_block(ctx, [&inp[0][pos..end], &inp[1][pos..end]], [&mut ol[pos..end], &mut or[pos..end]]);
            pos = end;
        }
    }

    fn process_block(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let n = inp[0].len();
        let [ol, or] = out;
        let runs = self.active && self.mix != 0.0 && self.kind != 0 && self.node.is_some();
        if !runs {
            ol.copy_from_slice(inp[0]);
            or.copy_from_slice(inp[1]);
        } else {
            let [bl, br] = &mut self.buf;
            let (bl, br) = (&mut bl[..n], &mut br[..n]);
            self.trim.process(self.trim_target, inp, [&mut *bl, &mut *br]);
            self.node.as_mut().unwrap().process(ctx, [bl, br], [&mut *ol, &mut *or]);
            // [re:fa096b](out, wet, dry, mix).
            let m = self.mix;
            for i in 0..n {
                ol[i] = inp[0][i] + (ol[i] - inp[0][i]) * m;
                or[i] = inp[1][i] + (or[i] - inp[1][i]) * m;
            }
        }
        if self.active && self.kind != 0 && (self.level - 1.0).abs() > 1e-6 {
            ol.iter_mut().for_each(|x| *x *= self.level);
            or.iter_mut().for_each(|x| *x *= self.level);
        }
    }

    /// The node's control output `index` ([`Node::output`]).
    pub fn output(&self, index: usize) -> Option<f32> {
        self.node.as_deref().and_then(|n| n.output(index))
    }

    pub fn tail(&self, ctx: &Ctx) -> usize {
        if !self.active {
            return 0;
        }
        match self.node.as_deref() {
            Some(node) => nodes::tail(self.kind, &self.params, node, ctx),
            None => 0,
        }
    }
}

/// `EffectChain` ([re:3b4d65]): six effects in series and a chain mix.
pub struct EffectChain {
    pub active: bool,
    pub mix: f32,
    pub slots: Vec<Option<Effect>>,
    a: [[f32; NODE_BLOCK]; 2],
    b: [[f32; NODE_BLOCK]; 2],
}

impl EffectChain {
    pub fn new() -> EffectChain {
        EffectChain { active: true, mix: 1.0, slots: (0..6).map(|_| None).collect(), a: [[0.0; NODE_BLOCK]; 2], b: [[0.0; NODE_BLOCK]; 2] }
    }

    /// [re:373932]: every slot's effect.
    pub fn panic(&mut self) {
        for fx in self.slots.iter_mut().flatten() {
            fx.panic();
        }
    }

    /// Whether processing is needed [re:b32927].
    pub fn needs_to_process(&self) -> bool {
        self.active && self.mix.abs() >= 1e-6 && self.slots.iter().flatten().any(|e| e.kind != 0 && e.active)
    }

    /// [re:eb214b] in place (in [`NODE_BLOCK`] chunks;
    /// the slots are independent and causal, so chunking is transparent).
    pub fn process(&mut self, ctx: &Ctx, l: &mut [f32], r: &mut [f32]) {
        if !self.needs_to_process() {
            return;
        }
        for (l, r) in l.chunks_mut(NODE_BLOCK).zip(r.chunks_mut(NODE_BLOCK)) {
            self.process_block(ctx, l, r);
        }
    }

    fn process_block(&mut self, ctx: &Ctx, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        self.a[0][..n].copy_from_slice(l);
        self.a[1][..n].copy_from_slice(r);
        for slot in self.slots.iter_mut() {
            let [al, ar] = &mut self.a;
            let [bl, br] = &mut self.b;
            let (al, ar, bl, br) = (&al[..n], &ar[..n], &mut bl[..n], &mut br[..n]);
            match slot {
                Some(e) => e.process(ctx, [al, ar], [bl, br]),
                None => {
                    bl.copy_from_slice(al);
                    br.copy_from_slice(ar);
                }
            }
            std::mem::swap(&mut self.a, &mut self.b);
        }
        let m = self.mix;
        for i in 0..n {
            l[i] = l[i] * (1.0 - m) + m * self.a[0][i];
            r[i] = r[i] * (1.0 - m) + m * self.a[1][i];
        }
    }

    pub fn tail(&self, ctx: &Ctx) -> usize {
        self.slots.iter().flatten().map(|e| e.tail(ctx)).sum()
    }
}

impl Default for EffectChain {
    fn default() -> Self {
        EffectChain::new()
    }
}
