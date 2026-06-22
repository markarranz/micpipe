mod resampler;
use resampler::Resampler;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{
    HeapRb,
    traits::{Consumer, Producer, Split},
};
use std::thread;

/// Convert one input frame (file's channels) into one output frame (device's channels).
fn convert_frame(input: &[f32], in_ch: usize, out_ch: usize) -> Vec<f32> {
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

fn spawn_source(
    path: &'static str,
    out_channels: usize,
    out_rate: u32,
    mut producer: impl Producer<Item = f32> + Send + 'static,
) {
    thread::spawn(move || {
        let reader = hound::WavReader::open(path).unwrap();
        let in_channels = reader.spec().channels as usize;
        let in_rate = reader.spec().sample_rate;
        println!(
            "'{}': {}ch@{}Hz -> {}ch@{}Hz",
            path, in_channels, in_rate, out_channels, out_rate
        );

        let mut samples = reader.into_samples::<f32>();
        let mut resampler = Resampler::new(in_rate, out_rate, out_channels);

        // Output samples produced by the resampler, waiting to be pushed.
        let mut pending: Vec<f32> = Vec::new();
        let mut push_idx = 0;

        loop {
            // If nothing pending, read one input frame, convert channels, resample.
            if push_idx >= pending.len() {
                pending.clear();
                push_idx = 0;

                // Read one INPUT frame.
                let mut input_frame = Vec::with_capacity(in_channels);
                let mut ended = false;
                for _ in 0..in_channels {
                    match samples.next() {
                        Some(Ok(s)) => input_frame.push(s),
                        Some(Err(e)) => {
                            eprintln!("'{}': {}", path, e);
                            ended = true;
                            break;
                        }
                        None => {
                            ended = true;
                            break;
                        }
                    }
                }
                if ended || input_frame.len() < in_channels {
                    break; // clean end of file (or partial frame at EOF)
                }

                // Convert channels, then resample. Resampler appends to `pending`.
                let converted = convert_frame(&input_frame, in_channels, out_channels);
                resampler.process(&converted, &mut pending);

                // process() may emit zero frames (e.g. the very first call), so loop again.
                if pending.is_empty() {
                    continue;
                }
            }

            // Push the pending output samples into the ring buffer.
            match producer.try_push(pending[push_idx]) {
                Ok(()) => push_idx += 1,
                Err(_) => thread::sleep(std::time::Duration::from_millis(1)),
            }
        }
        println!("Source '{}': stream finished", path);
    });
}

fn main() {
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let config = device.default_output_config().unwrap();
    println!("Output config: {:?}", config);

    let out_channels = config.channels() as usize;
    let out_rate = config.sample_rate();

    let capacity = 48_000;
    let (producer_a, mut consumer_a) = HeapRb::<f32>::new(capacity).split();
    let (producer_b, mut consumer_b) = HeapRb::<f32>::new(capacity).split();

    // Per-source gain (faders).
    let gain_a: f32 = 0.5;
    let gain_b: f32 = 0.5;

    // Launch both source threads.
    spawn_source("a.wav", out_channels, out_rate, producer_a);
    spawn_source("b.mono.441k.wav", out_channels, out_rate, producer_b);

    let stream = device
        .build_output_stream(
            &config.into(),
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for out_sample in output.iter_mut() {
                    let a = consumer_a.try_pop().unwrap_or(0.0);
                    let b = consumer_b.try_pop().unwrap_or(0.0);
                    // THE MIX.
                    *out_sample = a * gain_a + b * gain_b;
                }
            },
            |err| eprintln!("stream error: {}", err),
            None,
        )
        .unwrap();

    stream.play().unwrap();
    println!("Mixing a.wav + b.mono.441k.wav through ring buffers. Press Enter to stop.");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}
