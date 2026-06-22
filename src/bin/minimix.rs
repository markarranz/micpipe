use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use cpal::{
    BufferSize, StreamConfig,
    traits::{DeviceTrait, StreamTrait},
};

use clap::{Parser, Subcommand};

use ringbuf::{
    HeapRb,
    traits::{Consumer, Observer, Producer, Split},
};

use minimix::{
    audio::{convert_frame, find_input_device, find_output_device},
    resampler::Resampler,
};

// CoreAudio's fixed buffer size.
const OUTPUT_BUFFER_FRAMES: u32 = 512;
const STEADY_CUSHION_CALLBACKS: usize = 2;
const JITTERY_EXTRA_MARGIN_MS: u32 = 50;

#[derive(Parser)]
#[command(
    name = "minimix",
    version,
    about = "Route your microphone into BlackHole"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the audio driver (this is what the launchd service invokes.)
    Run(RunArgs),
    /// Install and start the launchd service.
    Install(RunArgs),
    /// Remove the launchd service.
    Uninstall,
    /// Start the installed service.
    Start,
    /// stop the running service.
    Stop,
    /// Restart the service.
    Restart,
    /// Show whether the service is installed and running.
    Status,
}

#[derive(clap::Args, Clone)]
struct RunArgs {
    /// Output device name to route into (substring match).
    #[arg(short, long, default_value = "BlackHole 2ch")]
    output: String,

    /// Input device name (substring match). Omit to follow the system default.
    #[arg(short, long)]
    input: Option<String>,

    /// Enable per-second buffer occupancy logging.
    #[arg(short, long)]
    debug: bool,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => run(args),
        Command::Install(args) => install(args),
        Command::Uninstall => uninstall(),
        Command::Start => start(),
        Command::Stop => stop(),
        Command::Restart => restart(),
        Command::Status => status(),
    }
}

fn run(args: RunArgs) {
    // --- Source: current default input device ---
    let input_device = find_input_device(args.input.as_deref()); // None = default
    let in_config = input_device.default_input_config().unwrap();
    let in_channels = in_config.channels() as usize;
    let in_rate = in_config.sample_rate();

    // --- Sink: BlackHole 2ch ---
    let output_device = find_output_device(Some(&args.output));
    let out_config = output_device.default_output_config().unwrap();
    let out_channels = out_config.channels() as usize;
    let out_rate = out_config.sample_rate();

    let mut out_stream_config: StreamConfig = out_config.into();
    out_stream_config.buffer_size = BufferSize::Fixed(OUTPUT_BUFFER_FRAMES);

    println!(
        "Routing: {} ({}ch@{}Hz) -> {} ({}ch@{}Hz)",
        input_device.description().unwrap(),
        in_channels,
        in_rate,
        output_device.description().unwrap(),
        out_channels,
        out_rate,
    );

    // --- Detect jitter risk ---
    let likely_jittery = in_rate <= 24_000;
    let samples_per_ms = (out_rate as usize * out_channels) / 1_000;
    let callback_samples = OUTPUT_BUFFER_FRAMES as usize * out_channels;

    let jitter_extra = if likely_jittery {
        JITTERY_EXTRA_MARGIN_MS as usize * samples_per_ms
    } else {
        0
    };

    // Cushion = baseline phase-offset coverage + (jittery margin if applicable).
    let target_fill = STEADY_CUSHION_CALLBACKS * callback_samples + jitter_extra;
    let capacity = (target_fill * 4).max(callback_samples * 8);
    let (mut producer, mut consumer) = HeapRb::<f32>::new(capacity).split();

    println!(
        "Input {} | callback {} frames | cushion {} samples (~{}ms) | buffer {} samples",
        if likely_jittery { "jittery" } else { "steady" },
        OUTPUT_BUFFER_FRAMES,
        target_fill,
        target_fill / samples_per_ms,
        capacity
    );

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

    let input_callback = move |input: &[f32], _: &cpal::InputCallbackInfo| {
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
    };

    // --- Output callback: drain ring buffer into BlackHole ---
    let output_callback = move |output: &mut [f32], _: &cpal::OutputCallbackInfo| {
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
    };

    let input_stream = input_device
        .build_input_stream(
            in_config.into(),
            input_callback,
            |err| eprintln!("input stream error: {}", err),
            None,
        )
        .unwrap();

    let output_stream = output_device
        .build_output_stream(
            out_stream_config,
            output_callback,
            |err| eprintln!("output stream error: {}", err),
            None,
        )
        .unwrap();

    input_stream.play().unwrap();
    output_stream.play().unwrap();

    println!("Mic -> BlackHole running...");

    // --- Logger thread: prints occupancy once per second (off the audio thread) ---
    if args.debug {
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

fn install(_args: RunArgs) {
    todo!("write plist + bootstrap")
}
fn uninstall() {
    todo!("bootout + remove plist")
}
fn start() {
    todo!("launchctl bootstrap")
}
fn stop() {
    todo!("launchctl bootout")
}
fn restart() {
    todo!("launchctl kickstart")
}
fn status() {
    todo!("launchctl print + parse")
}
