//! On-disk names of the legacy instrument that the apps read for
//! compatibility: where its content is installed, its marker files, its
//! document root element and the names its skins use. They must stay
//! byte-identical.

use std::path::PathBuf;

/// The legacy documents folder under the home folder (user content).
pub const DOCUMENTS_DIR: &str = "Documents/FXpansion/Geist2";

/// The installed factory content under the home folder.
pub const CONTENT_DIR: &str = "Documents/FXpansion/Geist2/Content";

/// The demo library inside the factory content.
pub const DEMO_FACTORY: &str = "Geist2 Demo Factory";

/// The lite edition's factory content under the home folder (REX loops).
pub const LITE_FACTORY: &str = "Documents/ableton/FXpansion/GeistLite/AudioData/GeistLite Factory";

/// A folder's "do not slice" marker file.
pub const NOSLICE_FILE: &str = "Geist.noslice";

/// The document root element of project and part files.
pub const ROOT_ELEMENT: &str = "Geist";

/// The stored type name of the Reverb effect.
pub const REVERB_TYPE: &str = "FXpansion Reverb";

/// Icon font families named by legacy skins.
pub const ICON_FAMILIES: [&str; 2] = ["FXpansion-Geist2-Icons", "FXpansion-Geist2-Browser"];

/// The installed factory content (`DREIST_CONTENT` overrides it).
pub fn content_dir() -> PathBuf {
    std::env::var_os("DREIST_CONTENT").map(PathBuf::from).unwrap_or_else(|| crate::paths::home().join(CONTENT_DIR))
}

/// A file of the demo library (`rel` inside [`DEMO_FACTORY`]).
pub fn demo(rel: &str) -> PathBuf {
    content_dir().join(DEMO_FACTORY).join(rel)
}

/// The original v1's per-step routing-depth lanes (its on-disk names), the
/// step lanes after the 93 shared ones ([`crate::params::V1_ROUTING_LANE`]
/// onwards): the depths of the layer's routings 0..3.
pub const ROUTING_DEPTH_LANES: [&str; 4] = ["Ampenvdepth1", "Ampenvdepth2", "Freeenvdepth1", "Freeenvdepth2"];

/// The original v1's layer routing element and its slot names.
pub const ROUTING_ELEMENT: &str = "LayerModulationRouting";
const ROUTING_DESTS: [&str; 4] = ["Pitch", "Cutoff", "Resonance", "Drive"];
const NO_DESTINATION: &str = "No destination";

/// A routing slot's destination (0 pitch, 1 cutoff, 2 resonance, 3 drive)
/// and depth (0..1, 0.5 = none).
pub type RoutingSlot = Option<(u8, f32)>;

/// The original v1's four envelope routings of a layer, by slot (0 and 1
/// the amp envelope's, 2 and 3 the free envelope's); `None` when the layer
/// carries none (a v2 document).
pub fn routing_slots(layer: &crate::Node) -> Option<[RoutingSlot; 4]> {
    let mut any = false;
    let slots = std::array::from_fn(|i| {
        let r = layer.child(ROUTING_ELEMENT, &format!("ModRouting{i}"))?;
        any = true;
        let dest = ROUTING_DESTS.iter().position(|d| r.attr("ModulationDestination") == Some(d))? as u8;
        Some((dest, r.f32("ModulationDepth").unwrap_or(0.5)))
    });
    any.then_some(slots)
}

/// Write routing slot `i` (`None`: no destination) into `layer`.
pub fn set_routing_slot(layer: &mut crate::Node, i: usize, slot: RoutingSlot) {
    let r = layer.child_mut_or_insert(ROUTING_ELEMENT, &format!("ModRouting{i}"));
    let (dest, depth) = match slot {
        Some((d, v)) => (ROUTING_DESTS.get(d as usize).copied().unwrap_or(NO_DESTINATION), v),
        None => (NO_DESTINATION, 0.5),
    };
    r.set("ModulationDestination", dest).set("ModulationDepth", crate::node::format_g16(depth as f64));
}

#[cfg(test)]
mod routing_tests {
    use super::*;

    #[test]
    fn routing_slots_round_trip() {
        let mut l = crate::Node::parse(r#"<Layer name="Layer0"><LayerModulationRouting name="ModRouting0" ModulationDepth="0.9" ModulationDestination="Cutoff" /><LayerModulationRouting name="ModRouting3" ModulationDepth="0.3" ModulationDestination="No destination" /></Layer>"#).unwrap();
        assert_eq!(routing_slots(&l), Some([Some((1, 0.9)), None, None, None]));
        set_routing_slot(&mut l, 2, Some((0, 0.25)));
        set_routing_slot(&mut l, 0, None);
        assert_eq!(routing_slots(&l), Some([None, None, Some((0, 0.25)), None]));
        assert_eq!(routing_slots(&crate::Node::new("Layer", Some("Layer0"))), None);
        assert_eq!(crate::params::step_lane_index("Freeenvdepth2"), Some(crate::params::V1_ROUTING_LANE + 3));
        assert_eq!(crate::params::step_lane_name(crate::params::V1_ROUTING_LANE), Some("Ampenvdepth1"));
        assert_eq!(crate::params::step_lane_default(crate::params::V1_ROUTING_LANE + 2), 0.5);
    }
}
