//! Multi-pad selection ([re:d611cc], `e_setPadSelections`
//! 0x44, `EngineState` `padSelAnchor` / `padSelItems`) and the original's
//! "apply to every selected pad" rule of [re:5723c8]
//! (`GetSelectionOptions` [re:f3180c] / `GetSelectedPads` [re:eac469]).
//!
//! A click selects the pad; cmd toggles it in the selection; shift selects
//! the contiguous range from the anchor. Pad messages from the editor whose
//! pad is part of the selection apply to every selected pad: parameter
//! (0x4e) and send level (0x55) changes move the others by the same delta,
//! clamped to 0..1; the rest set the same value. Automation messages are
//! not affected (they come from bindings, straight to the engine).
//!
//! Modifiers (`e_setModifiers` 0x26, the pad header's AllPads / AllLayers;
//! `GetSelectionOptions` [re:f3180c] / [re:2b5118], `GetSelectedLayers` [re:cba339],
//! ev_9aeeaaa0…, `HandleLayerMessage` [re:7c4772]): with all pads on, pad
//! messages go to all 64 pads. Layer messages from the editor whose layer
//! is in its pad's layer selection (the port: the current layer) or with
//! all layers on go to every existing layer of the selected layers (all 8
//! with all layers) of the addressed pad (every pad with all pads);
//! `e_setLayerParameter` 0x66 moves the others by the same delta. The layer
//! selection (`e_setLayerSelections` 0x45, [re:b98fa6] `layerSelAnchor` /
//! `layerSelItems`) comes from the pad inspector's layer table.

use dreist_engine::binding::{Message, id};
use dreist_formats::Node;

use crate::command::Command;
use crate::model::Model;

/// The engine's selection: (anchor, mask of selected pads).
pub fn selection(model: &Model, e: usize) -> (usize, u64) {
    let st = model.engine_node(e).and_then(|n| n.children.iter().find(|c| c.class == "EngineState"));
    let anchor = model.pad[e];
    let mask = st.and_then(|s| s.attr("padSelItems")).and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(0);
    // The current pad is always part of it.
    (anchor, mask | 1u64 << (anchor.min(63)))
}

/// [re:0d8a70] the selection in the document (`EngineState` attributes) and make
/// the anchor the current pad.
pub fn set_selection(model: &mut Model, e: usize, anchor: usize, mask: u64) {
    model.pad[e] = anchor;
    let st = model.engine_mut(e).child_mut_or_insert("EngineState", "EngineState");
    st.set("padSelAnchor", anchor);
    st.set("padSelItems", mask);
}

/// `SelectPad`: plain → just `p`; cmd → toggle `p`; shift → the range from
/// the anchor. Returns the message for the engine (0x44 [e] [anchor, mask
/// low 32 bits, high 32 bits as raw floats]).
pub fn select(model: &mut Model, e: usize, p: usize, cmd: bool, shift: bool) -> Command {
    let (anchor, mask) = selection(model, e);
    let bit = 1u64 << p.min(63);
    let (anchor, mask) = if cmd {
        let m = mask ^ bit;
        if m == 0 {
            (p, bit)
        } else if m & bit != 0 {
            (p, m)
        } else if anchor == p {
            (m.trailing_zeros() as usize, m)
        } else {
            (anchor, m)
        }
    } else if shift {
        let (a, b) = (anchor.min(p), anchor.max(p));
        (anchor, (a..=b).fold(0u64, |m, i| m | 1u64 << i))
    } else {
        (p, bit)
    };
    set_selection(model, e, anchor, mask);
    crate::command::msg(id::SET_PAD_SELECTIONS, &[e as i32], &[anchor as f32, f32::from_bits(mask as u32), f32::from_bits((mask >> 32) as u32)])
}

/// Copy the engines' selection attributes from `from` into `to` (pad
/// selection is not an undo step).
pub fn carry_selection(from: &Node, to: &mut Node) {
    for eng in from.children.iter().filter(|c| c.class == "Engine") {
        let Some(st) = eng.children.iter().find(|c| c.class == "EngineState") else { continue };
        let Some(teng) = to.children.iter_mut().find(|c| c.class == "Engine" && c.name == eng.name) else { continue };
        let i = match teng.children.iter().position(|c| c.class == "EngineState") {
            Some(i) => i,
            None => {
                teng.children.push(Node::new("EngineState", Some("EngineState")));
                teng.children.len() - 1
            }
        };
        let tst = &mut teng.children[i];
        for k in ["padSelAnchor", "padSelItems"] {
            match st.attr(k) {
                Some(v) => {
                    tst.set(k, v);
                }
                None => tst.attrs.retain(|(a, _)| a != k),
            }
        }
        // Layer selections ([re:01b7f4]) are not undo steps either.
        for ps in st.children.iter().filter(|c| c.class == "PadState") {
            let Some(name) = ps.name.as_deref() else { continue };
            let t = tst.child_mut_or_insert("PadState", name);
            for k in ["layerSelAnchor", "layerSelItems"] {
                if let Some(v) = ps.attr(k) {
                    t.set(k, v);
                }
            }
        }
    }
}

/// Pad messages that apply to the whole selection.
fn fans_out(kind: u32) -> bool {
    matches!(
        kind,
        id::MUTE_PAD
            | id::SOLO_PAD
            | id::SET_PAD_PARAMETER
            | id::SET_PAD_OUTPUT
            | id::SET_PAD_LAYER_MODE
            | id::SET_PAD_LAYER_MODE01
            | id::SET_PAD_LINKAGE
            | id::SET_PAD_SEND_LEVEL
            | id::AUTO_VELOCITY_SPLIT_PAD
            | id::AUTO_VELOCITY_CROSS
    )
}

/// The extra messages for the other selected pads (to be applied after the
/// original ones, against the document as it was).
pub fn fan_out(model: &Model, cmds: &[Command]) -> Vec<Command> {
    let mut extra = Vec::new();
    for c in cmds {
        let Command::Message(m) = c else { continue };
        if engine_fans_out(m.kind) && m.index[0] >= 0 {
            engine_fan_out(model, m, &mut extra);
            continue;
        }
        if layer_fans_out(m.kind) && m.index[0] >= 0 && m.index[1] >= 0 && m.index[2] >= 0 {
            layer_fan_out(model, m, &mut extra);
            continue;
        }
        if !fans_out(m.kind) || m.index[0] < 0 || m.index[1] < 0 {
            continue;
        }
        let (e, p) = (m.index[0] as usize, m.index[1] as usize);
        let (_, mask) = selection(model, e);
        let mask = if model.all_pads { u64::MAX } else { mask };
        if mask & (1u64 << p.min(63)) == 0 || mask.count_ones() < 2 {
            continue;
        }
        // Relative changes: the addressed pad's old value.
        let attr = |m: &Message| -> Option<(String, f32)> {
            match m.kind {
                id::SET_PAD_PARAMETER => {
                    let par = dreist_formats::PadParam::ALL.get(m.value[1].max(0.0) as usize)?;
                    let src = m.value[2].max(0.0) as usize;
                    let a = dreist_engine::engine::doc::attr_name(par.name(), src)?;
                    Some((a, if src == 0 { par.default_value() } else { 0.0 }))
                }
                id::SET_PAD_SEND_LEVEL => {
                    let a = dreist_engine::engine::doc::attr_name(&dreist_formats::params::aux_send_name(m.value[0].max(0.0) as usize), m.value[2].max(0.0) as usize)?;
                    Some((a, 0.0))
                }
                _ => None,
            }
        };
        let value_slot = if m.kind == id::SET_PAD_PARAMETER { 0 } else { 1 };
        let rel = attr(m);
        let old_of = |q: usize, a: &str, d: f32| model.pad_node(e, q).and_then(|n| n.f32(a)).unwrap_or(d);
        let delta = rel.as_ref().map(|(a, d)| m.value[value_slot] - old_of(p, a, *d));
        for q in (0..64).filter(|&q| q != p && mask & (1u64 << q) != 0) {
            let mut n = m.clone();
            n.index[1] = q as i32;
            if let (Some((a, d)), Some(dv)) = (&rel, delta) {
                n.value[value_slot] = (old_of(q, a, *d) + dv).clamp(0.0, 1.0);
            }
            extra.push(Command::Message(n));
        }
    }
    extra
}

/// A pad's layer selection ([re:01b7f4] `layerSelItems`, the current
/// layer always part of it for the current pad): (anchor, mask).
pub fn layer_selection(model: &Model, e: usize, p: usize) -> (usize, u8) {
    let ps = model.engine_node(e).and_then(|n| n.children.iter().find(|c| c.class == "EngineState")).and_then(|s| s.children.iter().find(|c| c.class == "PadState" && c.index() == Some(p as u32)));
    let stored = ps.and_then(|s| s.attr("layerSelItems")).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0) as u8;
    let anchor = if p == model.pad[e] { model.layer[e] } else { ps.and_then(|s| s.u32("layerSelAnchor")).unwrap_or(0) as usize };
    (anchor, stored | 1u8 << anchor.min(7))
}

/// [re:281f54] $_2 [re:c47c8f]: the layer table's row selection
/// (cmd toggles, shift extends) → `e_setLayerSelections` 0x45 [e, p]
/// [anchor, mask as raw float bits]; the anchor becomes the current layer.
pub fn select_layer(model: &mut Model, e: usize, l: usize, cmd: bool, shift: bool) -> Command {
    let p = model.pad[e];
    let (anchor, mask) = layer_selection(model, e, p);
    let bit = 1u8 << l.min(7);
    let (anchor, mask) = if cmd {
        let m = mask ^ bit;
        if m == 0 {
            (l, bit)
        } else if m & bit != 0 {
            (l, m)
        } else if anchor == l {
            (m.trailing_zeros() as usize, m)
        } else {
            (anchor, m)
        }
    } else if shift {
        let (a, b) = (anchor.min(l), anchor.max(l));
        (anchor, (a..=b).fold(0u8, |m, i| m | 1u8 << i))
    } else {
        (l, bit)
    };
    model.layer[e] = anchor;
    let st = model.engine_mut(e).child_mut_or_insert("EngineState", "EngineState");
    let ps = st.child_mut_or_insert("PadState", &format!("PadState{p}"));
    ps.set("layerSelAnchor", anchor);
    ps.set("layerSelItems", mask as u32);
    crate::command::msg(id::SET_LAYER_SELECTIONS, &[e as i32, p as i32], &[anchor as f32, f32::from_bits(mask as u32)])
}

/// The engine selection mask (the current engine always in it).
pub fn engine_selection(model: &Model) -> u8 {
    let bit = 1u8 << model.engine.min(7);
    if model.engine_sel & bit != 0 { model.engine_sel } else { bit }
}

/// [re:352eb9] on the engines (mixer strip presses): plain → just
/// `k`; cmd toggles; shift extends from the current engine. Returns the
/// new current engine.
pub fn select_engine(model: &mut Model, k: usize, cmd: bool, shift: bool) -> usize {
    let anchor = model.engine;
    let mask = engine_selection(model);
    let bit = 1u8 << k.min(7);
    let (anchor, mask) = if cmd {
        let m = mask ^ bit;
        if m == 0 {
            (k, bit)
        } else if m & bit != 0 {
            (k, m)
        } else if anchor == k {
            (m.trailing_zeros() as usize, m)
        } else {
            (anchor, m)
        }
    } else if shift {
        let (a, b) = (anchor.min(k), anchor.max(k));
        (anchor, (a..=b).fold(0u8, |m, i| m | 1u8 << i))
    } else {
        (k, bit)
    };
    model.engine_sel = mask;
    anchor
}

/// `HandleEngineMessage` [re:16aacd] (ev_5594bf7a…): editor messages for an
/// engine in the selection apply to the other selected engines; parameter
/// (0x90) and send (0x98) changes by the same delta, clamped 0..1.
fn engine_fans_out(kind: u32) -> bool {
    (0x8c..=0x96).contains(&kind) || kind == 0x98 || kind == 0x9b
}

fn engine_fan_out(model: &Model, m: &Message, extra: &mut Vec<Command>) {
    let e = m.index[0] as usize;
    let mask = engine_selection(model);
    if e > 7 || mask & (1u8 << e) == 0 || mask.count_ones() < 2 {
        return;
    }
    let rel = match m.kind {
        id::SET_ENGINE_PARAMETER => dreist_formats::EngineParam::ALL.get(m.value[1].max(0.0) as usize).and_then(|par| {
            let src = m.value[2].max(0.0) as usize;
            Some((dreist_engine::engine::doc::attr_name(par.name(), src)?, if src == 0 { par.default_value() } else { 0.0 }, 0))
        }),
        id::SET_ENGINE_SEND_LEVEL => dreist_engine::engine::doc::attr_name(&dreist_formats::params::aux_send_name(m.value[0].max(0.0) as usize), m.value[2].max(0.0) as usize).map(|a| (a, 0.0, 1)),
        _ => None,
    };
    let old_of = |q: usize, a: &str, d: f32| model.engine_node(q).and_then(|n| n.f32(a)).unwrap_or(d);
    for q in (0..8).filter(|&q| q != e && mask & (1u8 << q) != 0) {
        let mut n = m.clone();
        n.index[0] = q as i32;
        if let Some((a, d, slot)) = &rel {
            let delta = m.value[*slot] - old_of(e, a, *d);
            n.value[*slot] = (old_of(q, a, *d) + delta).clamp(0.0, 1.0);
        }
        extra.push(Command::Message(n));
    }
}

/// `HandleLayerMessage` types (0x65..0x89; 0x83 addresses no layer).
fn layer_fans_out(kind: u32) -> bool {
    (0x65..=0x89).contains(&kind) && kind != 0x83
}

fn layer_fan_out(model: &Model, m: &Message, extra: &mut Vec<Command>) {
    let (e, p, l) = (m.index[0] as usize, m.index[1] as usize, m.index[2] as usize);
    let (_, lmask) = layer_selection(model, e.min(7), p.min(63));
    let selected = lmask & (1u8 << l.min(7)) != 0;
    if !model.all_layers && !selected {
        return;
    }
    let pads: Vec<usize> = if model.all_pads { (0..64).collect() } else { vec![p] };
    let layers: Vec<usize> = if model.all_layers { (0..crate::model::LAYERS).collect() } else { (0..crate::model::LAYERS).filter(|k| lmask & (1u8 << k) != 0).collect() };
    // 0x66 [param, value, source]: relative to the addressed layer's old value.
    let rel = (m.kind == id::SET_LAYER_PARAMETER)
        .then(|| {
            let par = dreist_formats::LayerParam::ALL.get(m.value[0].max(0.0) as usize).copied()?;
            let src = m.value[2].max(0.0) as usize;
            let a = dreist_engine::engine::doc::attr_name(par.name(), src)?;
            Some((a, if src == 0 { par.default_value() } else { 0.0 }, src))
        })
        .flatten();
    let old_of = |q: usize, k: usize, a: &str, d: f32| model.layer_node(e, q, k).and_then(|n| n.f32(a)).unwrap_or(d);
    let delta = rel.as_ref().map(|(a, d, _)| m.value[1] - old_of(p, l, a, *d));
    for &q in &pads {
        for &k in &layers {
            if (q, k) == (p, l) || model.layer_node(e, q, k).is_none() {
                continue;
            }
            let mut n = m.clone();
            n.index[1] = q as i32;
            n.index[2] = k as i32;
            if let (Some((a, d, src)), Some(dv)) = (&rel, delta) {
                let lo = if *src == 0 { 0.0 } else { -1.0 };
                n.value[1] = (old_of(q, k, a, *d) + dv).clamp(lo, 1.0);
            }
            extra.push(Command::Message(n));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_rules() {
        let mut model = Model::new(crate::project::new_document());
        let e = model.engine;
        select(&mut model, e, 3, false, false);
        assert_eq!(selection(&model, e), (3, 1 << 3));
        select(&mut model, e, 6, false, true);
        assert_eq!(selection(&model, e), (3, 0b111_1000));
        select(&mut model, e, 5, true, false);
        assert_eq!(selection(&model, e).1, 0b101_1000);
        // Fan-out: mute on pad 3 mutes 4 and 6 too; a gain change moves
        // the others by the same delta.
        let mute = crate::command::msg(id::MUTE_PAD, &[e as i32, 3], &[1.0]);
        let extra = fan_out(&model, &[mute]);
        assert_eq!(extra.len(), 2);
        let gain = dreist_formats::PadParam::Gain;
        let base = gain.default_value();
        let m = crate::command::msg(id::SET_PAD_PARAMETER, &[e as i32, 3], &[base - 0.1, gain as u8 as f32, 0.0]);
        let extra = fan_out(&model, &[m]);
        let Command::Message(x) = &extra[0] else { panic!() };
        assert!((x.value[0] - (base - 0.1)).abs() < 1e-5, "{}", x.value[0]);
        // A pad outside the selection is not fanned out.
        let other = crate::command::msg(id::MUTE_PAD, &[e as i32, 9], &[1.0]);
        assert!(fan_out(&model, &[other]).is_empty());
        // All pads: every other pad.
        model.all_pads = true;
        assert_eq!(fan_out(&model, &[crate::command::msg(id::MUTE_PAD, &[e as i32, 9], &[1.0])]).len(), 63);
    }

    #[test]
    fn engine_selection_fans_engine_messages_out() {
        let mut model = Model::new(crate::project::new_document());
        model.engine = select_engine(&mut model, 1, false, false);
        model.engine = select_engine(&mut model, 3, false, true);
        assert_eq!(engine_selection(&model), 0b1110);
        let gain = dreist_formats::EngineParam::Gain;
        let base = gain.default_value();
        let m = crate::command::msg(id::SET_ENGINE_PARAMETER, &[1], &[base - 0.2, gain as u8 as f32, 0.0]);
        let extra = fan_out(&model, &[m]);
        let to: Vec<(i32, f32)> = extra.iter().filter_map(|c| if let Command::Message(m) = c { Some((m.index[0], m.value[0])) } else { None }).collect();
        assert_eq!(to.len(), 2);
        assert!(to.iter().all(|(_, v)| (v - (base - 0.2)).abs() < 1e-5));
        assert_eq!(to.iter().map(|t| t.0).collect::<Vec<_>>(), vec![2, 3]);
        // Engine 5 outside the selection: nothing.
        assert!(fan_out(&model, &[crate::command::msg(id::MUTE_ENGINE, &[5], &[1.0])]).is_empty());
    }

    #[test]
    fn modifiers_fan_layer_messages_out() {
        let mut model = Model::new(crate::project::new_document());
        let e = model.engine;
        for l in 0..3 {
            model.layer_mut(e, 0, l);
        }
        model.layer_mut(e, 5, 0);
        let mute = crate::command::msg(id::SET_LAYER_MUTE, &[e as i32, 0, 0], &[1.0]);
        assert!(fan_out(&model, std::slice::from_ref(&mute)).is_empty());
        model.all_layers = true;
        let extra = fan_out(&model, std::slice::from_ref(&mute));
        let to: Vec<(i32, i32)> = extra.iter().filter_map(|c| if let Command::Message(m) = c { Some((m.index[1], m.index[2])) } else { None }).collect();
        assert_eq!(to, vec![(0, 1), (0, 2)]);
        // All pads with the current layer: layer 0 of every pad.
        model.all_layers = false;
        model.all_pads = true;
        let extra = fan_out(&model, std::slice::from_ref(&mute));
        let to: Vec<(i32, i32)> = extra.iter().filter_map(|c| if let Command::Message(m) = c { Some((m.index[1], m.index[2])) } else { None }).collect();
        assert_eq!(to, vec![(5, 0)]);
        // A layer selection of 0 and 2: the edit reaches layer 2 too.
        model.all_pads = false;
        model.pad[e] = 0;
        select_layer(&mut model, e, 0, false, false);
        select_layer(&mut model, e, 2, true, false);
        assert_eq!(layer_selection(&model, e, 0), (2, 0b101));
        let mute = crate::command::msg(id::SET_LAYER_MUTE, &[e as i32, 0, 2], &[1.0]);
        let extra = fan_out(&model, std::slice::from_ref(&mute));
        let to: Vec<(i32, i32)> = extra.iter().filter_map(|c| if let Command::Message(m) = c { Some((m.index[1], m.index[2])) } else { None }).collect();
        assert_eq!(to, vec![(0, 0)]);
    }
}
