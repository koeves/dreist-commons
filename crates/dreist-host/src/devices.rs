//! What the standalone's audio / MIDI settings panels list ([`crate::audio`]
//! and [`crate::midi`] fill them).

/// Devices the standalone can open.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioSetup {
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
    pub default_output: String,
    /// Rates the chosen output supports, and its own rate.
    pub rates: Vec<u32>,
    pub default_rate: u32,
    pub out_channels: u16,
    pub in_channels: u16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MidiSetup {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}
