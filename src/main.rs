use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{
    HeapRb,
    traits::{Consumer, Producer, Split},
};

fn spawn_source(path: &'static str, mut producer: impl Producer<Item = f32> + Send + 'static) {
    thread::spawn(move || {
        let mut reader = hound::WavReader::open(path).unwrap();
        let samples: Vec<f32> = reader.samples::<f32>().map(|s| s.unwrap()).collect();

        let mut i = 0;
        while i < samples.len() {
            match producer.try_push(samples[i]) {
                Ok(()) => i += 1,
                Err(_) => thread::sleep(std::time::Duration::from_millis(1)),
            }
        }
        println!("Source '{}': done feeding {} samples", path, samples.len());
    });
}

fn main() {
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device");
    let config = device.default_output_config().unwrap();
    println!("Output config: {:?}", config);

    let capacity = 48_000;

    // Two ring buffers, one per source.
    let (producer_a, mut consumer_a) = HeapRb::<f32>::new(capacity).split();
    let (producer_b, mut consumer_b) = HeapRb::<f32>::new(capacity).split();

    // Per-source gain (faders).
    let gain_a: f32 = 0.5;
    let gain_b: f32 = 0.5;

    // Launch both source threads.
    spawn_source("a.wav", producer_a);
    spawn_source("b.wav", producer_b);

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
    println!("Mixing a.wav + b.wav through ring buffers. Press Enter to stop.");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
}
