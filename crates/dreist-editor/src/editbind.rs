//! Editor-only bindings (`Binding` `EditorOnly`): controls whose MIDI
//! / host binding posts a message back to the editor instead of the engine
//! ([re:aca9d3], [re:841010],
//! [re:702088], [re:f3c257], [re:45e9a3]
//! [re:4dc7b3], [re:c2fdf6], [re:029183], the editor
//! [re:59ad1d], [re:b2db08]; ev_48b124b4…).
//!
//! The panels' message handlers act on them (ev_7a135bd5…: [re:b1e892]
//! [re:0f6c7c], [re:728dc3], [re:13d3b0],
//! [re:edde0b]):
//! - set a toggle from the value (≥ 0.5) and notify: Show Splits 0x1c5, All
//!   Pads 0x1c6, All Layers 0x1c7, Show Amp / Free Envelope 0x1cd / 0x1ce,
//!   browser Auto / Slice / Preview mode 0x15c–0x15e;
//! - flip a toggle: pattern Gated 0x1bc, Restart 0x1bd, Remix 0x1be;
//! - press a button: browser next / previous file 0x155 / 0x156, next /
//!   previous location 0x157 / 0x158, refresh 0x159, enter 0x15a, parent
//!   0x15b; pattern steps − / + 0x1b4 / 0x1b5, Duplicate 0x1b6, nudges
//!   0x1b7–0x1ba, Quantize 0x1bb, Copy to next 0x1bf; undo / redo 0x18d /
//!   0x18e; scene snapshot to next 0x10e;
//! - values: pattern length 0x1b2 = ⌊v·1023 + 1⌋, step length 0x1b3 =
//!   the popup item at v;
//! - panels: editor tabs 0x146–0x14c, utility tabs 0x14d / 0x14e, mixer
//!   Mix / Sends / Effects 0x14f–0x151.

/// One editor binding: the control (`name` inside a container named
/// `scope`), its binding path / name / type and message.
pub struct EditorBinding {
    pub scope: &'static str,
    pub control: &'static str,
    pub group: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
    pub id: u32,
}

const fn b(scope: &'static str, control: &'static str, group: &'static str, name: &'static str, kind: &'static str, id: u32) -> EditorBinding {
    EditorBinding { scope, control, group, name, kind, id }
}

pub const TABLE: &[EditorBinding] = &[
    b("Header", "UndoButton", "Global", "Undo", "Trigger", 0x18d),
    b("Header", "RedoButton", "Global", "Redo", "Trigger", 0x18e),
    b("PadInspector", "AllPads", "Global", "All Pads", "Continuous", 0x1c6),
    b("PadInspector", "AllLayers", "Global", "All Layers", "Continuous", 0x1c7),
    b("PadInspector", "ShowSplits", "Global", "Show Splits", "Continuous", 0x1c5),
    b("PadInspector", "ShowAmpEnvButton", "Global", "Show Amp Envelope", "Continuous", 0x1cd),
    b("PadInspector", "ShowFreeEnvButton", "Global", "Show Free Envelope", "Continuous", 0x1ce),
    b("UtilityTabPanel", "AutoAllocateButton", "Browser", "Auto Mode", "Trigger", 0x15c),
    b("UtilityTabPanel", "SliceModeButton", "Browser", "Slice Mode", "Trigger", 0x15d),
    b("UtilityTabPanel", "PreviewButton", "Browser", "Preview", "Trigger", 0x15e),
    b("UtilityTabPanel", "NextFileButton", "Browser", "Next file", "Trigger", 0x155),
    b("UtilityTabPanel", "PrevFileButton", "Browser", "Previous file", "Trigger", 0x156),
    b("UtilityTabPanel", "NextLocnButton", "Browser", "Next location", "Trigger", 0x157),
    b("UtilityTabPanel", "PrevLocnButton", "Browser", "Previous location", "Trigger", 0x158),
    b("UtilityTabPanel", "RefreshButton", "Browser", "Refresh", "Trigger", 0x159),
    b("UtilityTabPanel", "EnterButton", "Browser", "Enter location", "Trigger", 0x15a),
    b("UtilityTabPanel", "ParentButton", "Browser", "Parent location", "Trigger", 0x15b),
    b("ToolBar", "LengthBox", "Engine", "Pattern Length", "Continuous", 0x1b2),
    b("ToolBar", "StepLengthCombo", "Engine", "Step Length", "Continuous", 0x1b3),
    b("ToolBar", "MinusButton", "Engine", "Decrease number of steps", "Gated", 0x1b4),
    b("ToolBar", "PlusButton", "Engine", "Increase number of steps", "Gated", 0x1b5),
    b("ToolBar", "RepeatButton", "Engine", "Duplicate", "Gated", 0x1b6),
    b("ToolBar", "QuantizeButton", "Engine", "Pattern quantize", "Gated", 0x1bb),
    b("ToolBar", "Gated", "Engine", "Pattern gated", "Gated", 0x1bc),
    b("ToolBar", "Restart", "Engine", "Pattern restart", "Gated", 0x1bd),
    b("ToolBar", "Remix", "Engine", "Remix", "Gated", 0x1be),
    b("ToolBar", "CopyToNext", "Engine", "Copy to next", "Trigger", 0x1bf),
    b("PatternEditor", "NudgePatternBackButton", "Engine", "Nudge pattern back", "Gated", 0x1b7),
    b("PatternEditor", "NudgePatternForwardButton", "Engine", "Nudge pattern forward", "Gated", 0x1b8),
    b("PatternEditor", "NudgeTrackBackButton", "Engine", "Nudge track back", "Gated", 0x1b9),
    b("PatternEditor", "NudgeTrackForwardButton", "Engine", "Nudge track forward", "Gated", 0x1ba),
    b("EditorTabPanel", "SnapshotToNextButton", "Scenes", "Snapshot To Next Scene", "Trigger", 0x10e),
];

/// Editor tab buttons ([re:23be19]: button id → panel).
pub const EDITOR_TABS: [(usize, &str, u32); 7] =
    [(0, "Pattern", 0x146), (1, "Scenes", 0x147), (2, "Song", 0x148), (3, "Layer Mixer", 0x14b), (4, "Pad Mixer", 0x14a), (5, "Global Mixer", 0x149), (6, "Mapping", 0x14c)];
pub const UTILITY_TABS: [(usize, &str, u32); 2] = [(0, "Browser", 0x14e), (1, "Transmod", 0x14d)];
/// Mixer ModeGroup children ([re:69fc1c]).
pub const MIXER_MODES: [(&str, u32); 3] = [("Mix", 0x14f), ("Sends", 0x150), ("Effects", 0x151)];

pub fn by_id(id: u32) -> Option<&'static EditorBinding> {
    TABLE.iter().find(|e| e.id == id)
}
