use cpal::traits::{DeviceTrait, HostTrait};

use crate::error::{OptionExt, Result, ResultExt};

/// Convert one input frame into one output frame.
pub fn convert_frame(input: &[f32], in_ch: usize, output: &mut [f32]) {
    output.fill(0.0);

    match (in_ch, output.len()) {
        (1, 2) if !input.is_empty() => {
            output[0] = input[0];
            output[1] = input[0];
        }
        (2, 1) if input.len() >= 2 => {
            output[0] = (input[0] + input[1]) * 0.5;
        }
        (a, b) if a == b && input.len() >= b => {
            output.copy_from_slice(&input[..b]);
        }
        (_, out_ch) => {
            let len = out_ch.min(input.len());
            output[..len].copy_from_slice(&input[..len]);
        }
    }
}

/// Find an output device whose description contains `name` (case-insensitive).
/// Pass None to get the default output device.
pub fn find_output_device(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    match name {
        None => Ok(host
            .default_output_device()
            .context("no default output device")?),
        Some(needle) => {
            let needle = needle.to_lowercase();
            Ok(host
                .output_devices()
                .context("could not enumerate output devices")?
                .find(|d| {
                    d.description()
                        .map(|desc| device_description_matches(&desc.to_string(), &needle))
                        .unwrap_or(false)
                })
                .context(format!("no output device matching '{needle}'"))?)
        }
    }
}

/// Find an input device whose description contains `name` (case-insensitive).
/// Pass None to get the default input device.
pub fn find_input_device(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    match name {
        None => Ok(host
            .default_input_device()
            .context("no default input device")?),
        Some(needle) => {
            let needle = needle.to_lowercase();
            Ok(host
                .input_devices()
                .context("could not enumerate input devices")?
                .find(|d| {
                    d.description()
                        .map(|desc| device_description_matches(&desc.to_string(), &needle))
                        .unwrap_or(false)
                })
                .context(format!("no input device matching '{needle}'"))?)
        }
    }
}

fn device_description_matches(description: &str, lowercase_needle: &str) -> bool {
    description.to_lowercase().contains(lowercase_needle)
}

#[cfg(test)]
mod tests {
    use super::{convert_frame, device_description_matches};

    #[test]
    fn duplicates_mono_to_stereo() {
        let mut output = [0.0, 0.0];

        convert_frame(&[0.75], 1, &mut output);

        assert_eq!(output, [0.75, 0.75]);
    }

    #[test]
    fn averages_stereo_to_mono() {
        let mut output = [0.0];

        convert_frame(&[0.25, 0.75], 2, &mut output);

        assert_eq!(output, [0.5]);
    }

    #[test]
    fn copies_matching_channels() {
        let mut output = [0.0, 0.0];

        convert_frame(&[0.25, 0.75], 2, &mut output);

        assert_eq!(output, [0.25, 0.75]);
    }

    #[test]
    fn pads_missing_fallback_channels() {
        let mut output = [1.0, 1.0, 1.0];

        convert_frame(&[0.25], 4, &mut output);

        assert_eq!(output, [0.25, 0.0, 0.0]);
    }

    #[test]
    fn matches_device_description_case_insensitively() {
        assert!(device_description_matches("BlackHole 2ch", "blackhole"));
        assert!(device_description_matches(
            "MacBook Pro Microphone [Microphone] via Built-in",
            "microphone"
        ));
        assert!(!device_description_matches(
            "External Headphones",
            "blackhole"
        ));
    }
}
