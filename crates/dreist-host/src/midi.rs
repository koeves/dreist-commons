//! MIDI ports and input connections through midir.

use crate::devices::MidiSetup;

/// The MIDI input and output port names (`client` names the midir client).
pub fn setup(client: &str) -> MidiSetup {
    let inputs = midir::MidiInput::new(client).map(|m| m.ports().iter().filter_map(|p| m.port_name(p).ok()).collect()).unwrap_or_default();
    let outputs = midir::MidiOutput::new(client).map(|m| m.ports().iter().filter_map(|p| m.port_name(p).ok()).collect()).unwrap_or_default();
    MidiSetup { inputs, outputs }
}

/// An open MIDI input; dropping it closes it.
pub struct InputConnection(midir::MidiInputConnection<()>);

impl InputConnection {
    /// Close the connection.
    pub fn close(self) {
        let _ = self.0.close();
    }
}

/// Connect to MIDI input `port` (the first one when `port` is empty) with
/// a client named `client` and a connection named `name`, receiving every
/// message, system real-time and exclusive ones included. `f` gets each
/// message with its midir timestamp (microseconds) on midir's thread.
pub fn connect_input(client: &str, name: &str, port: &str, mut f: impl FnMut(u64, &[u8]) + Send + 'static) -> Option<InputConnection> {
    let mut input = midir::MidiInput::new(client).ok()?;
    input.ignore(midir::Ignore::None);
    let ports = input.ports();
    let p = ports.iter().find(|p| port.is_empty() || input.port_name(p).is_ok_and(|n| n == port))?.clone();
    input.connect(&p, name, move |stamp, m, _| f(stamp, m), ()).ok().map(InputConnection)
}
