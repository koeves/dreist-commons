//! [re:71cd73] (port setup [re:e2cf51], processing
//! [re:4799a8], reset [re:d640a7]): tanh soft clipper above a knee.

use super::math::clip01;
use crate::fx::{Ctx, Node, Port, ports, ports_from};

pub struct Limiter {
    ports: Vec<Port>,
    wet: f32,
    pre: f32,
    post: f32,
    /// Gain-reduction meter (control-out "GR"), 0..1.
    pub gr: f32,
}

impl Limiter {
    pub fn new() -> Limiter {
        Limiter { ports: ports_from(ports::LIMITER), wet: 1.0, pre: 1.0, post: 1.0, gr: 1.0 }
    }
}

impl Default for Limiter {
    fn default() -> Self {
        Limiter::new()
    }
}

impl Node for Limiter {
    fn ports(&mut self) -> &mut Vec<Port> {
        &mut self.ports
    }

    /// The gain-reduction meter (control output 0, 1 = none), which the
    /// panel's meter shows ([re:f22615]).
    fn output(&self, index: usize) -> Option<f32> {
        (index == 0).then_some(self.gr)
    }

    fn process(&mut self, ctx: &Ctx, inp: [&[f32]; 2], out: [&mut [f32]; 2]) {
        let n = inp[0].len();
        if n == 0 {
            return;
        }
        let inv = 1.0 / n as f32;
        let p = |i: usize| self.ports[i].value;
        let knee = (clip01(p(1)) * 10.0).powf(3.0);
        let d_wet = (clip01(p(0)) - self.wet) * inv;
        let pre3 = p(2) * p(2) * p(2);
        let d_pre = (((pre3.abs() + 8.0) - (pre3 - 8.0).abs()) * 0.5 - self.pre) * inv;
        let post3 = p(3) * p(3) * p(3);
        let d_post = (clip01(post3) - self.post) * inv;
        let k1 = knee + 1.0;
        let inv_k1 = 1.0 / k1;
        let mut peak_in = 0.0f32;
        let mut peak_out = 0.0f32;
        let [ol, or] = out;
        let shape = |y: f32, peak_out: &mut f32, peak_in: &mut f32| -> f32 {
            let a = k1 * y.abs();
            if knee < a {
                let m = ((a - knee).tanh() + knee) * inv_k1;
                if *peak_out < m {
                    *peak_in = y.abs();
                    *peak_out = m;
                }
                m * if 0.0 < y { 1.0 } else { -1.0 }
            } else {
                y
            }
        };
        for i in 0..n {
            self.wet += d_wet;
            self.post += d_post;
            self.pre += d_pre;
            let dry = (1.0 - self.wet) * self.post;
            let (xl, xr) = (inp[0][i], inp[1][i]);
            let yl = shape(self.pre * xl, &mut peak_out, &mut peak_in);
            ol[i] = xl * dry + yl * self.post * self.wet;
            let yr = shape(self.pre * xr, &mut peak_out, &mut peak_in);
            or[i] = dry * xr + self.post * self.wet * yr;
        }
        let mut g = 1.0;
        if peak_out != 0.0 && peak_in != 0.0 {
            let db = ((peak_in.log10() - peak_out.log10()) * 20.0).min(48.0);
            g = 1.0 - db / 48.0;
        }
        if self.gr <= g {
            let k = (n as f32 * (1.0 / ctx.sample_rate) * -12.063716).exp();
            g = (g - self.gr) * (1.0 - k) + self.gr;
        }
        self.gr = g;
    }

    fn reset(&mut self) {
        self.wet = 1.0;
        self.pre = 1.0;
        self.post = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_clips_above_the_knee() {
        let mut l = Limiter::new();
        // Hardness 0.5 → knee (0.5·10)^3 = 125 — effectively transparent at unity gain.
        let x = vec![0.5f32; 64];
        let (mut a, mut b) = (vec![0.0; 64], vec![0.0; 64]);
        l.process(&Ctx::new(48000.0), [&x, &x], [&mut a, &mut b]);
        assert!((a[63] - 0.5).abs() < 1e-3);
        // Hardness 0 → knee 0: tanh-shaped, never above 1.
        l.ports[1].set01(0.0);
        let x = vec![3.0f32; 64];
        l.process(&Ctx::new(48000.0), [&x, &x], [&mut a, &mut b]);
        assert!(a.iter().all(|v| *v <= 1.0) && a[63] > 0.9);
    }
}
