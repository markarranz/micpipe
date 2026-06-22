use cpal::traits::{DeviceTrait, HostTrait};

/// Convert one input frame (in_ch samples) into one output frame (out_ch samples).
pub fn convert_frame(input: &[f32], in_ch: usize, out_ch: usize) -> Vec<f32> {
    match (in_ch, out_ch) {
        (1, 2) => vec![input[0], input[0]], // mono -> stereo: duplicate
        (2, 1) => vec![(input[0] + input[1]) * 0.5], // stereo -> mono: average
        (a, b) if a == b => input.to_vec(), // same: passthrough
        // Fallback: take what we can, pad with silence.
        (_, out) => {
            let mut f = vec![0.0; out];
            for i in 0..out.min(input.len()) {
                f[i] = input[i];
            }
            f
        }
    }
}

/// Find an output device whose description contains `name` (case-sensitive).
/// Pass None to get the default output device.
pub fn find_output_device(name: Option<&str>) -> cpal::Device {
    let host = cpal::default_host();
    match name {
        None => host
            .default_output_device()
            .expect("no default output device"),
        Some(needle) => {
            let needle = needle.to_lowercase();
            host.output_devices()
                .expect("could not enumerate output devices")
                .find(|d| {
                    d.description()
                        .map(|desc| format!("{}", desc).to_lowercase().contains(&needle))
                        .unwrap_or(false)
                })
                .unwrap_or_else(|| panic!("no output device matching '{}'", needle))
        }
    }
}

/// Find an input device whose description contains `name` (case-sensitive).
/// Pass None to get the default input device.
pub fn find_input_device(name: Option<&str>) -> cpal::Device {
    let host = cpal::default_host();
    match name {
        None => host
            .default_input_device()
            .expect("no default input device"),
        Some(needle) => {
            let needle = needle.to_lowercase();
            host.input_devices()
                .expect("could not enumerate input devices")
                .find(|d| {
                    d.description()
                        .map(|desc| format!("{}", desc).to_lowercase().contains(&needle))
                        .unwrap_or(false)
                })
                .unwrap_or_else(|| panic!("no input device matching '{}'", needle))
        }
    }
}
