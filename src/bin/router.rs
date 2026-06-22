use cpal::traits::{DeviceTrait, StreamTrait};
use ringbuf::{
    HeapRb,
    traits::{Consumer, Producer, Split},
};

use minimix::{
    audio::{convert_frame, find_input_device, find_output_device},
    resampler::Resampler,
};

fn main() {
    // --- Source: current default input device ---
    let input_device = find_input_device(None); // None = default
    let in_config = input_device.default_input_config().unwrap();
    let in_channels = in_config.channels() as usize;
    let in_rate = in_config.sample_rate();

    // --- Sink: BlackHole 2ch ---
    let output_device = find_output_device(Some("BlackHole 2ch"));
    let out_config = output_device.default_output_config().unwrap();
    let out_channels = out_config.channels() as usize;
    let out_rate = out_config.sample_rate();

    println!(
        "Routing: {} ({}ch@{}Hz) -> {} ({}ch@{}Hz)",
        input_device.description().unwrap(),
        in_channels,
        in_rate,
        output_device.description().unwrap(),
        out_channels,
        out_rate,
    );

    // --- Ring buffer between the two callbacks ---
    let capacity = 48_000; // ~0.5s of stereo @ 48k
    let (mut producer, mut consumer) = HeapRb::<f32>::new(capacity).split();

    // --- Input callback: capture -> convert -> resample -> push ---
    // Pre-allocate everything the callback resuses (no allocation on the audio thread).
    let mut resampler = Resampler::new(in_rate, out_rate, out_channels);
    let mut resampled: Vec<f32> = Vec::with_capacity(8192); // scratch, reused each call

    let input_stream = input_device
        .build_input_stream(
            &in_config.into(),
            move |input: &[f32], _: &cpal::InputCallbackInfo| {
                // `input` is interleaed at in_channels. Walk it frame by frame.
                for frame in input.chunks(in_channels) {
                    if frame.len() < in_channels {
                        break; // ignore a trailing partial frame
                    }
                    let converted = convert_frame(frame, in_channels, out_channels);

                    resampled.clear();
                    resampler.process(&converted, &mut resampled);

                    for &s in resampled.iter() {
                        // Drop samples if the buffer is full; never block the audio thread.
                        let _ = producer.try_push(s);
                    }
                }
            },
            |err| eprintln!("input stream error: {}", err),
            None,
        )
        .unwrap();

    // --- Output callback: drain ring buffer into BlackHole ---
    let output_stream = output_device
        .build_output_stream(
            &out_config.into(),
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for out_sample in output.iter_mut() {
                    *out_sample = consumer.try_pop().unwrap_or(0.0);
                }
            },
            |err| eprintln!("output stream error: {}", err),
            None,
        )
        .unwrap();

    input_stream.play().unwrap();
    output_stream.play().unwrap();

    println!("Mic -> BlackHole running. Press Enter to stop.");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
}
