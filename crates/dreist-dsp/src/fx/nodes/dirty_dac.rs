//! [re:403241] (Dirty DAC, kind 46; reset [re:7302b2],
//! processing [re:26214d]): low-pass at `(1.5 − AntiAlias)·Freq`,
//! plus a copy high-passed three times at `0.7·Freq` and multiplied by a
//! band-limited square derived from a triangle running at `Freq` — the
//! stair-step error of a zero-order-hold DAC.
//!
//! Only the left filters' coefficients are used for both channels (the right
//! and the fourth high-pass are computed but never read).

use crate::fx::{Ctx, Node, Port, ports, ports_from};

use super::blocks::{PolySect as Sect, highpass_poly as highpass, lowpass_poly as lowpass};

pub struct DirtyDac {
    ports: Vec<Port>,
    lp: Sect,
    hp: [Sect; 3],
    phase: f32,
    inc: f32,
}

impl DirtyDac {
    pub fn new() -> DirtyDac {
        DirtyDac { ports: ports_from(ports::DIRTY_DAC), lp: Sect::new(), hp: [Sect::new(); 3], phase: 0.0, inc: f32::from_bits(0x38d1b717) }
    }
}

impl Default for DirtyDac {
    fn default() -> Self {
        DirtyDac::new()
    }
}

impl Node for DirtyDac {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let n = inp[0].len();
        let freq = self.ports[0].value;
        let isr = 1.0 / ctx.sample_rate;
        let c_lp = lowpass((1.5 - self.ports[1].value) * isr * freq);
        let c_hp = highpass(isr * 0.7 * freq);
        let start = self.inc;
        let mut inc = if 1e-08 <= start { start } else { 1e-08 };
        if n == 0 {
            self.inc = inc;
            return;
        }
        let mix = ((self.ports[2].value.abs() + 1.0) - (self.ports[2].value - 1.0).abs()) * 0.5;
        let inv_n = 1.0 / n as f32;
        let k = inc + inc + inc + inc;
        let off = (k + -1.0) * (1.0 / k);
        let mut ph = self.phase;
        let [ol, or] = out;
        for i in 0..n {
            let tri = if 0.5 < ph { (1.0 - (ph + ph)) + ph } else { ph };
            let t = (tri + -0.25) * 4.0;
            let u = (1.0 / k) * t;
            let a = (off + u).max(0.0);
            let a = if a <= 0.0 { 0.0 } else { a };
            let x = [inp[0][i], inp[1][i]];
            let low = self.lp.run(&c_lp, x);
            let h1 = self.hp[0].run(&c_hp, low);
            let h2 = self.hp[1].run(&c_hp, h1);
            let h3 = self.hp[2].run(&c_hp, h2);
            let b = u - off;
            let b = if 0.0 <= b { 0.0 } else { b };
            let sq = (b * b * k + (t - a * a * k)) * 4.0;
            ph += inc;
            if 1.0 < ph {
                ph += -1.0;
            }
            ol[i] = (h3[0] * sq + low[0]) * mix + x[0] * (1.0 - mix);
            inc += (freq * isr - start) * inv_n;
            or[i] = (h3[1] * sq + low[1]) * mix + x[1] * (1.0 - mix);
        }
        self.phase = ph;
        self.inc = inc;
    }

    fn reset(&mut self) {
        self.lp.clear();
        for s in &mut self.hp {
            s.clear();
        }
        self.phase = 0.0;
        self.inc = f32::from_bits(0x38d1b717);
    }
}
