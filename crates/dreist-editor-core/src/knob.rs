//! Transmod knobs bound to a layer, pad or effect parameter: the value or a
//! source's depth shown on the knob, and the depths sent back.

use dreist_formats::Node;

/// Show a transmod knob for `source`: the base value, or the source's depth
/// (−1..1) as an editable zone around the base (see dreist-gui render).
pub fn bind_knob(c: &mut dreist_gui::Ctrl, source: usize, base: f32, depth: f32) {
    if source == 0 {
        c.value = base;
        c.remove_attr("editingDepth");
        c.remove_attr("modDepth");
    } else {
        c.value = (depth + 1.0) / 2.0;
        c.set_attr("editingDepth", "true".into());
        c.set_attr("baseValue", base.to_string());
        c.set_attr("modDepth", depth.to_string());
    }
}

/// The transmod sources (bit per source 1..=28) with a non-zero depth on
/// parameter `key` of `node` (the objects' `Get…ModulationBitField`).
pub fn modulation_mask(node: Option<&Node>, key: &str) -> u32 {
    let Some(n) = node else { return 0 };
    (1..=28).filter(|&s| dreist_engine::engine::doc::attr_name(key, s).and_then(|k| n.f32(&k)).is_some_and(|d| d != 0.0)).fold(0, |m, s| m | 1 << s)
}

/// The value to send for a knob moved to `v` while editing `source`.
pub fn knob_value(source: usize, v: f32) -> f32 {
    if source == 0 { v } else { v * 2.0 - 1.0 }
}
