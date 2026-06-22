use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn main() {
    // --- Load the WAV file fully into memory ---
    let mut reader = hound::WavReader::open("test.wav").unwrap();
    let spec = reader.spec();
    println!("WAV spec: {:?}", spec);

    // Read all samples as f32, interleaved [L, R, L, R, ...]
    let samples: Vec<f32> = reader.samples::<f32>().map(|s| s.unwrap()).collect();
    println!("Loaded {} samples", samples.len());

    // --- Set up the output device ---
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let config = device.default_output_config().unwrap();
    println!("Output config: {:?}", config);

    // Playback position: index into the samples vec
    let mut position = 0;

    let stream = device
        .build_output_stream(
            &config.into(),
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for out_sample in output.iter_mut() {
                    if position < samples.len() {
                        *out_sample = samples[position];
                        position += 1;
                    } else {
                        *out_sample = 0.0; // silence after file ends
                    }
                }
            },
            |err| eprintln!("stream error: {}", err),
            None,
        )
        .unwrap();

    stream.play().unwrap();
    println!("Playing test.wav. Press Enter to stop.");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}
