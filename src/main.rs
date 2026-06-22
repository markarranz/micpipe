use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{
    HeapRb,
    traits::{Consumer, Producer, Split},
};

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
    mut producer: impl Producer<Item = f32> + Send + 'static,
) {
    thread::spawn(move || {
        let reader = hound::WavReader::open(path).unwrap();
        let in_channels = reader.spec().channels as usize;
        println!(
            "'{}': {} channel(s) -> {} channel(s)",
            path, in_channels, out_channels
        );

        let mut samples = reader.into_samples::<f32>();

        // Pending OUTPUT frame: converted, waiting to be pushed
        let mut pending: Option<Vec<f32>> = None;
        let mut pushed_in_frame = 0; // how many samples of the pending frame are already in the buffer

        loop {
            // Build a new pending output frame if we don't have one.
            if pending.is_none() {
                // Read one INPUT frame: in_channels sample.
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
                pending = Some(convert_frame(&input_frame, in_channels, out_channels));
                pushed_in_frame = 0;
            }

            // Push the pending frame sample-by-sample; only clear when fully pushed.
            if let Some(frame) = &pending {
                let mut blocked = false;
                while pushed_in_frame < frame.len() {
                    match producer.try_push(frame[pushed_in_frame]) {
                        Ok(()) => pushed_in_frame += 1,
                        Err(_) => {
                            blocked = true;
                            break;
                        }
                    }
                }
                if pushed_in_frame == frame.len() {
                    pending = None; // whole frame delivered
                } else if blocked {
                    thread::sleep(std::time::Duration::from_millis(1));
                }
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

    let capacity = 48_000;
    let (producer_a, mut consumer_a) = HeapRb::<f32>::new(capacity).split();
    let (producer_b, mut consumer_b) = HeapRb::<f32>::new(capacity).split();

    // Per-source gain (faders).
    let gain_a: f32 = 0.5;
    let gain_b: f32 = 0.5;

    // Launch both source threads.
    spawn_source("a.wav", out_channels, producer_a);
    spawn_source("b.mono.wav", out_channels, producer_b);

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
    println!("Mixing a.wav + b.mono.wav through ring buffers. Press Enter to stop.");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}
