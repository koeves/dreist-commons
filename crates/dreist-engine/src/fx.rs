//! Effect chains as stored in projects ([re:631e3c],
//! [re:9e37ad]) and their per-block parameter update
//! ([re:c517d2]). The DSP lives in `dreist_dsp::fx`.

use dreist_dsp::fx::{Ctx, Effect, EffectChain};
use dreist_dsp::voice::SOURCES;
use dreist_formats::params::{effect_param_names, effect_type_index};
use dreist_formats::{Node, TransmodSource};

use crate::engine::{Row, param_sum};

struct SlotRows {
    is_active: bool,
    mix: Row,
    level: Row,
    trim: Row,
    params: Vec<Row>,
}

fn row(n: &Node, names: &[String], default: f32) -> Row {
    let mut r = [0.0; SOURCES];
    r[0] = names.iter().find_map(|k| n.f32(k)).unwrap_or(default);
    for (i, s) in TransmodSource::ALL.iter().enumerate().skip(1) {
        r[i] = names.iter().find_map(|k| n.f32(&format!("{k}_{}", s.name()))).unwrap_or(0.0);
    }
    r
}

/// What an effect message sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxTarget {
    Param(usize),
    Mix,
    Level,
    Trim,
}

/// A loaded chain with its parameter rows.
pub struct FxChain {
    pub chain: EffectChain,
    rows: Vec<Option<SlotRows>>,
}

impl FxChain {
    pub fn from_node(n: Option<&Node>, sample_rate: f32) -> FxChain {
        let mut chain = EffectChain::new();
        let mut rows: Vec<Option<SlotRows>> = (0..6).map(|_| None).collect();
        if let Some(n) = n {
            chain.active = n.bool("IsActive").unwrap_or(true);
            chain.mix = n.f32("ChainMix").unwrap_or(1.0);
            for e in n.children_of("Effect") {
                let Some(slot) = e.index().map(|i| i as usize).filter(|i| *i < 6) else { continue };
                let Some(kind) = e.attr("EffectType").and_then(effect_type_index) else { continue };
                if kind == 0 {
                    continue;
                }
                let params = effect_param_names(kind)
                    .iter()
                    .enumerate()
                    .map(|(k, name)| {
                        let stripped: String = name.chars().filter(|c| !c.is_whitespace()).collect();
                        row(e, &[stripped, format!("Parameter{k}")], 0.0)
                    })
                    .collect();
                let r =
                    SlotRows { is_active: e.bool("IsActive").unwrap_or(true), mix: row(e, &["Mix".into()], 0.5), level: row(e, &["Level".into()], 1.0), trim: row(e, &["Trim".into()], 1.0), params };
                let mut fx = Effect::new(kind, sample_rate);
                fx.active = r.is_active;
                fx.mix = r.mix[0];
                fx.level = r.level[0];
                fx.trim_target = r.trim[0];
                for (k, p) in r.params.iter().enumerate() {
                    fx.push_parameter(k, p[0]);
                }
                chain.slots[slot] = Some(fx);
                rows[slot] = Some(r);
            }
        }
        FxChain { chain, rows }
    }

    pub fn is_empty(&self) -> bool {
        self.rows.iter().all(|r| r.is_none())
    }

    /// [re:585f9c]: modulation vector `m` and, for pad chains, the
    /// triggering step's lanes.
    pub fn update(&mut self, m: &[f32; SOURCES], lanes: Option<&[f32]>) {
        for (n, (slot, rows)) in self.chain.slots.iter_mut().zip(&self.rows).enumerate() {
            let (Some(fx), Some(r)) = (slot.as_mut(), rows.as_ref()) else { continue };
            let lane = |i: usize| lanes.map(|l| l.get(i).copied().unwrap_or(1.0)).unwrap_or(1.0);
            fx.active = match lanes {
                Some(l) => match (l[39 + 9 * n] * 2.0 + 1.0).floor() as i32 {
                    3 => false,
                    1 => r.is_active,
                    _ => true,
                },
                None => r.is_active,
            };
            fx.mix = param_sum(&r.mix, m) * lane(40 + 9 * n);
            fx.level = param_sum(&r.level, m) * lane(41 + 9 * n);
            fx.trim_target = param_sum(&r.trim, m);
            for (k, p) in r.params.iter().enumerate().take(12) {
                let modulated = p[1..].iter().any(|d| *d != 0.0);
                let v = if modulated { param_sum(p, m) } else { p[0] };
                fx.push_parameter(k, v * lane(42 + 9 * n + k));
            }
        }
    }

    /// [re:7dabbf] / mix / output level / input trim
    /// (messages 0x59, 0x5c–0x5e): store into the slot's rows; the next
    /// `update` applies it.
    pub fn set(&mut self, slot: usize, target: FxTarget, src: usize, value: f32) {
        let Some(Some(r)) = self.rows.get_mut(slot) else { return };
        if src >= SOURCES {
            return;
        }
        let row = match target {
            FxTarget::Param(k) => match r.params.get_mut(k) {
                Some(p) => p,
                None => return,
            },
            FxTarget::Mix => &mut r.mix,
            FxTarget::Level => &mut r.level,
            FxTarget::Trim => &mut r.trim,
        };
        row[src] = value;
    }

    /// Effect active (message 0x5b).
    pub fn set_active(&mut self, slot: usize, active: bool) {
        if let Some(Some(r)) = self.rows.get_mut(slot) {
            r.is_active = active;
        }
    }

    pub fn process(&mut self, ctx: &Ctx, l: &mut [f32], r: &mut [f32]) {
        self.chain.process(ctx, l, r);
    }

    pub fn tail(&self, ctx: &Ctx) -> usize {
        self.chain.tail(ctx)
    }
}
