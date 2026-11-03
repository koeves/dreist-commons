//! Meter feed: every visible [re:d45123] gets one update
//! per editor tick from the engine's peak-hold levels, which the caller
//! then clears ([re:11bc96]).
//!
//! The meter's source comes from its ancestors: mixer strips (`channel`
//! attr: the mixer channel's output levels), inspector layer rows
//! (`layer` attr), the pad header (`PadSelButton`) and the sampler panel
//! ([re:7fc93e]).

use dreist_engine::Meters;
use dreist_gui::{Ctrl, meter};

use crate::model::Model;

#[derive(Clone, Copy)]
enum Src {
    Engine(usize),
    Aux(usize),
    Master,
    Pad(usize),
    Layer(usize),
    Sampler,
}

fn parse_channel(s: &str) -> Option<Src> {
    let num = |p: &str| s.strip_prefix(p)?.strip_suffix(')')?.parse().ok();
    if s == "Master" {
        return Some(Src::Master);
    }
    num("Engine(").map(Src::Engine).or_else(|| num("Aux(").map(Src::Aux)).or_else(|| num("Pad(").map(Src::Pad)).or_else(|| num("Layer(").map(Src::Layer))
}

/// Update all meters; the root-coordinate rectangles of those that changed.
pub fn update(root: &mut Ctrl, model: &Model, levels: &Meters) -> Vec<dreist_gui::Rect> {
    fn rec(c: &mut Ctrl, ox: i32, oy: i32, src: Option<Src>, model: &Model, levels: &Meters, out: &mut Vec<dreist_gui::Rect>) {
        if !c.visible {
            return;
        }
        let (x, y) = (ox + c.rect.x, oy + c.rect.y);
        let mut changed = false;
        let src = c
            .attr("channel")
            .and_then(parse_channel)
            .or_else(|| c.attr("layer").and_then(|v| v.parse().ok()).map(Src::Layer))
            .or(match c.name.as_str() {
                "PadSelButton" => Some(Src::Pad(model.pad[model.engine])),
                "SamplerPanel" => Some(Src::Sampler),
                _ => None,
            })
            .or(src);
        // [re:7ebd1a]: the selector overlay
        // follows |(L + R)/2| of the engine output, jumping up and falling
        // by (old − new)·0.8149 per tick; drawn as colAltFill at
        // min(level·0.75, 1) alpha.
        // [re:941836]: the logo (`MasterVU`,
        // [re:4f0f8e]) follows (L + R)/2 of the master
        // output with the same smoothing.
        let level_src = if c.overload == dreist_gui::render::LOGO_OVERLOAD {
            Some(levels.master)
        } else {
            c.attr("volEngine").and_then(|v| v.parse::<usize>().ok()).map(|e| levels.engine[e.min(levels.engine.len() - 1)])
        };
        if let Some(lv) = level_src {
            let target = ((lv[0] + lv[1]) * 0.5).abs();
            let cur = c.attr_f32("volLevel", 0.0);
            let next = if target > cur { target } else { (cur - target) * 0.814_929_4 + target };
            if (cur - next).abs() >= 1e-6 {
                c.set_attr("volLevel", next.to_string());
                c.set_attr("overlayAlpha", (next * 0.75).min(1.0).to_string());
                changed = true;
            }
        }
        if c.class == "CtrlMultiChannelDBMeter" {
            if let Some(s) = src {
                let e = model.engine;
                let lv = match s {
                    Src::Engine(k) => levels.engine[k.min(levels.engine.len() - 1)],
                    Src::Aux(k) => levels.aux[k.min(3)],
                    Src::Master => levels.master,
                    Src::Pad(k) => levels.pad_level(e, k),
                    Src::Layer(k) => levels.layer_level(e, model.pad[e], k),
                    Src::Sampler => [model.sampler.meters.0, model.sampler.meters.1],
                };
                let linear = c.attr_bool("optIsLinear", false);
                changed |= meter::update(&mut c.meters, &lv, linear);
            }
        }
        if changed {
            out.push(dreist_gui::Rect::new(x, y, c.rect.w, c.rect.h));
        }
        for k in &mut c.children {
            rec(k, x, y, src, model, levels, out);
        }
    }
    let mut out = Vec::new();
    rec(root, -root.rect.x, -root.rect.y, None, model, levels, &mut out);
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn channel_names() {
        assert!(matches!(super::parse_channel("Engine(3)"), Some(super::Src::Engine(3))));
        assert!(matches!(super::parse_channel("Master"), Some(super::Src::Master)));
        assert!(super::parse_channel("Other").is_none());
    }
}
