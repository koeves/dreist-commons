//! Audio devices through cpal's default host (the one nih-plug's
//! standalone opens: CoreAudio, WASAPI or ALSA).

use cpal::traits::{DeviceTrait, HostTrait};

use crate::devices::AudioSetup;

/// Rates offered when a device supports them.
pub const RATES: [u32; 6] = [44100, 48000, 88200, 96000, 176400, 192000];

fn named(devs: impl Iterator<Item = cpal::Device>, name: &str) -> Option<cpal::Device> {
    devs.into_iter().find(|d| d.name().is_ok_and(|n| n == name))
}

/// The devices, and the rates / channels of `output` and `input` ("" =
/// the default output, no input).
pub fn setup(output: &str, input: &str) -> AudioSetup {
    let host = cpal::default_host();
    let outputs: Vec<String> = host.output_devices().map(|d| d.filter_map(|d| d.name().ok()).collect()).unwrap_or_default();
    let inputs: Vec<String> = host.input_devices().map(|d| d.filter_map(|d| d.name().ok()).collect()).unwrap_or_default();
    let default = host.default_output_device();
    let default_output = default.as_ref().and_then(|d| d.name().ok()).unwrap_or_default();
    let out = if output.is_empty() { default } else { host.output_devices().ok().and_then(|d| named(d, output)) };
    let (mut rates, mut default_rate, mut out_channels) = (Vec::new(), 0, 0);
    if let Some(d) = &out {
        if let Ok(cfgs) = d.supported_output_configs() {
            let cfgs: Vec<_> = cfgs.collect();
            rates = RATES.iter().copied().filter(|r| cfgs.iter().any(|c| (c.min_sample_rate().0..=c.max_sample_rate().0).contains(r))).collect();
            out_channels = cfgs.iter().map(|c| c.channels()).max().unwrap_or(0);
        }
        default_rate = d.default_output_config().map(|c| c.sample_rate().0).unwrap_or(0);
    }
    let in_channels = if input.is_empty() { 0 } else { host.input_devices().ok().and_then(|d| named(d, input)).and_then(|d| d.default_input_config().ok()).map(|c| c.channels()).unwrap_or(0) };
    AudioSetup { outputs, inputs, default_output, rates, default_rate, out_channels, in_channels }
}

/// The nominal sample rate of the named (or default) output device.
pub fn output_sample_rate(name: &str) -> Option<u32> {
    let host = cpal::default_host();
    let dev = if name.is_empty() { None } else { host.output_devices().ok()?.find(|d| d.name().is_ok_and(|n| n == name)) };
    let dev = dev.or_else(|| host.default_output_device())?;
    Some(dev.default_output_config().ok()?.sample_rate().0)
}
