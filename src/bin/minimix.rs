use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use cpal::traits::{DeviceTrait, StreamTrait};
use ringbuf::{
    HeapRb,
    traits::{Consumer, Observer, Producer, Split},
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
    // ~100ms buffer @ 48k stereo; target a ~30ms cushion before draining.
    let capacity = 9600;
    let target_fill = 2800; // ~30ms - the cushion the output waits before strarting
    let (mut producer, mut consumer) = HeapRb::<f32>::new(capacity).split();

    // --- Gate: stays false until the buffer first builds the cushion
    let primed = Arc::new(AtomicBool::new(false));
    let primed_cb = Arc::clone(&primed);

    // --- Clock-drift instrumentation: shared buffer-occupancy gauge ---
    let occupancy = Arc::new(AtomicUsize::new(0));
    let occupancy_cb = Arc::clone(&occupancy); // moves into the output callback
    let occupancy_log = Arc::clone(&occupancy); // moves into the logger thread

    // --- Input callback: capture -> convert -> resample -> push ---
    let mut resampler = Resampler::new(in_rate, out_rate, out_channels);
    let mut resampled: Vec<f32> = Vec::with_capacity(8192); // scratch, reused each call

    let input_stream = input_device
        .build_input_stream(
            &in_config.into(),
            move |input: &[f32], _: &cpal::InputCallbackInfo| {
                // `input` is interleaved at in_channels. Walk it frame by frame.
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
                let available = consumer.occupied_len();

                // Observe buffer fill BEFORE draining (real-time safe: just an atomic store).
                occupancy_cb.store(available, Ordering::Relaxed);

                // Not yet primed: wait for the cushion to build. Output silence.
                if !primed_cb.load(Ordering::Relaxed) {
                    if available >= target_fill {
                        primed_cb.store(true, Ordering::Relaxed);
                    } else {
                        for s in output.iter_mut() {
                            *s = 0.0;
                        }
                        return;
                    }
                }

                // Primed: drain normally. If we ever fully underrun, re-arm the gate so we rebuild
                // the cushion instead of stuttering sample-by-sample.
                for out_sample in output.iter_mut() {
                    match consumer.try_pop() {
                        Some(s) => *out_sample = s,
                        None => {
                            *out_sample = 0.0;
                            primed_cb.store(false, Ordering::Relaxed);
                        }
                    }
                }
            },
            |err| eprintln!("output stream error: {}", err),
            None,
        )
        .unwrap();

    input_stream.play().unwrap();
    output_stream.play().unwrap();

    println!("Mic -> BlackHole running...");

    // --- Logger thread: prints occupancy once per second (off the audio thread) ---
    if std::env::var("MINIMIX_DEBUG").is_ok() {
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let occ = occupancy_log.load(Ordering::Relaxed);
                let pct = (occ as f32 / capacity as f32) * 100.0;
                println!("buffer: {} / {} samples ({:.1}%)", occ, capacity, pct);
            }
        });
    }

    // Park main forever so the streams stay alive.
    loop {
        std::thread::park();
    }
}
