use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn load_wav(path: &str) -> Vec<f32> {
    let mut reader = hound::WavReader::open(path).unwrap();
    reader.samples::<f32>().map(|s| s.unwrap()).collect()
}

fn main() {
    // --- Load both sources into memory ---
    let source_a = load_wav("a.wav");
    let source_b = load_wav("b.wav");
    println!("Source A: {} samples", source_a.len());
    println!("Source B: {} samples", source_b.len());

    // --- Per-source gain (faders) ---
    let gain_a: f32 = 0.5;
    let gain_b: f32 = 0.5;

    // --- Output device ---
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let config = device.default_output_config().unwrap();
    println!("Output config: {:?}", config);

    let mut pos_a = 0;
    let mut pos_b = 0;

    let stream = device
        .build_output_stream(
            &config.into(),
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for out_sample in output.iter_mut() {
                    let a = if pos_a < source_a.len() {
                        let v = source_a[pos_a];
                        pos_a += 1;
                        v
                    } else {
                        0.0
                    };

                    let b = if pos_b < source_b.len() {
                        let v = source_b[pos_b];
                        pos_b += 1;
                        v
                    } else {
                        0.0
                    };

                    // THE MIX: gain-controlled addition
                    *out_sample = a * gain_a + b * gain_b;
                }
            },
            |err| eprintln!("stream error: {}", err),
            None,
        )
        .unwrap();

    stream.play().unwrap();
    println!("Mixing a.wav + b.wav. Press Enter to stop.");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}
