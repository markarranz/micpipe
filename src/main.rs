use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{
    HeapRb,
    traits::{Consumer, Producer, Split},
};

fn main() {
    // --- Output device ---
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let config = device.default_output_config().unwrap();
    println!("Output config: {:?}", config);

    // --- Ring buffer: capacity in INTERLEAVED SAMPLES ---
    // 48000 Hz * 2 channels = 96000 samples/sec.
    // 48000 sample capacity = 0.5s of audio buffered.
    let capacity = 48_000;
    let rb = HeapRb::<f32>::new(capacity);
    let (mut producer, mut consumer) = rb.split();

    // --- Source thread: read WAV, push into ring buffer ---
    thread::spawn(move || {
        let mut reader = hound::WavReader::open("a.wav").unwrap();
        let samples: Vec<f32> = reader.samples::<f32>().map(|s| s.unwrap()).collect();

        let mut i = 0;
        loop {
            if i >= samples.len() {
                break; // file exhausted
            }
            // Try to push the next sample. If the buffer is full, the producer can't push - spin
            // briefly and retry rather than dropping data.
            match producer.try_push(samples[i]) {
                Ok(()) => i += 1,
                Err(_) => {
                    // Buffer full - wait a moment for the consumer to drain it.
                    thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
        println!("Source thread: done feeding {} samples", samples.len());
    });

    // --- Audio callback: pull from ring buffer, output ---
    let stream = device
        .build_output_stream(
            &config.into(),
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for out_sample in output.iter_mut() {
                    // Pull one sample. If none available (underrun), output silence.
                    *out_sample = consumer.try_pop().unwrap_or(0.0);
                }
            },
            |err| eprintln!("stream error: {}", err),
            None,
        )
        .unwrap();

    stream.play().unwrap();
    println!("Playing a.wav through ring buffer. Press Enter to stop.");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}
