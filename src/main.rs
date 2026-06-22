use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn main() {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("no default output device");

    println!("Output device: {}", device.description().unwrap());

    let config = device.default_output_config().unwrap();
    println!("Default config: {:?}", config);

    let sample_rate = config.sample_rate() as f32;
    let channels = config.channels() as usize;

    // Sine wave state: phase accumulator
    let mut phase: f32 = 0.0;
    let freq = 440.0; // A4
    let phase_increment = freq / sample_rate;

    let stream = device
        .build_output_stream(
            &config.into(),
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                // output is interleaved: [L, R, L, R, ...]
                for frame in output.chunks_mut(channels) {
                    // Genere one sample of the sine wave
                    let value = (phase * 2.0 * std::f32::consts::PI).sin() * 0.2;
                    phase = (phase + phase_increment) % 1.0;

                    // Write the same value to every channel in this frame
                    for sample in frame.iter_mut() {
                        *sample = value;
                    }
                }
            },
            |err| eprintln!("stream error: {}", err),
            None,
        )
        .unwrap();

    stream.play().unwrap();
    println!("Playing 440 Hz tone. Press Enter to stop.");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}
