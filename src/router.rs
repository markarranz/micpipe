use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use cpal::{
    BufferSize, ErrorKind, StreamConfig,
    traits::{DeviceTrait, StreamTrait},
};

use ringbuf::{
    HeapRb,
    traits::{Consumer, Producer, Split},
};

use anyhow::{Context, Result};

use crate::{
    audio::{convert_frame, find_input_device, find_output_device},
    resampler::Resampler,
    service,
};

use crate::cli::RunArgs;
#[cfg(target_os = "macos")]
use crate::default_input_watcher::DefaultInputChangeListener;

// CoreAudio's fixed buffer size.
const OUTPUT_BUFFER_FRAMES: u32 = 512;
const STEADY_CUSHION_CALLBACKS: usize = 2;
const JITTERY_EXTRA_MARGIN_MS: u32 = 50;
const RESAMPLED_SCRATCH_CAPACITY: usize = 8192;
const PINNED_INPUT_RECONNECT_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub fn run(args: &RunArgs) -> Result<()> {
    let runtime = AudioRuntime::start(args)?;
    park_runtime(runtime);
}

struct AudioRuntime {
    _input_stream: cpal::Stream,
    _output_stream: cpal::Stream,
    #[cfg(target_os = "macos")]
    _default_input_change_listener: Option<DefaultInputChangeListener>,
}

impl AudioRuntime {
    fn start(args: &RunArgs) -> Result<Self> {
        let restart_policy = RestartPolicy::from_args(args);
        let route = AudioRoute::from_args(args)?;
        log_route(&route);

        let buffer_plan = BufferPlan::new(route.in_rate, route.out_rate, route.out_channels);
        let (producer, consumer) = HeapRb::<f32>::new(buffer_plan.capacity).split();
        log_buffer_plan(buffer_plan);

        // Clock-drift instrumentation: shared buffer-occupancy gauge.
        let occupancy = Arc::new(AtomicUsize::new(0));
        let restart_requested = Arc::new(AtomicBool::new(false));

        let input_stream = build_input_stream(
            &route,
            producer,
            restart_policy.clone(),
            Arc::clone(&restart_requested),
        )?;
        let output_stream =
            build_output_stream(&route, consumer, buffer_plan, Arc::clone(&occupancy))?;

        input_stream
            .play()
            .context("failed to start input stream")?;
        output_stream
            .play()
            .context("failed to start output stream")?;

        crate::log_out!("Mic -> BlackHole running...");

        #[cfg(target_os = "macos")]
        let default_input_change_listener =
            watch_default_input_changes_when_needed(&route, &restart_policy, restart_requested)?;

        spawn_buffer_logger(args.debug, occupancy, buffer_plan);

        Ok(Self {
            _input_stream: input_stream,
            _output_stream: output_stream,
            #[cfg(target_os = "macos")]
            _default_input_change_listener: default_input_change_listener,
        })
    }
}

fn park_runtime(_runtime: AudioRuntime) -> ! {
    loop {
        std::thread::park();
    }
}

fn log_route(route: &AudioRoute) {
    crate::log_out!(
        "Routing: {} ({}ch@{}Hz) -> {} ({}ch@{}Hz)",
        route.input_description,
        route.in_channels,
        route.in_rate,
        route.output_description,
        route.out_channels,
        route.out_rate,
    );
}

fn log_buffer_plan(buffer_plan: BufferPlan) {
    crate::log_out!(
        "Input {} | callback {} frames | cushion {} samples (~{}ms) | buffer {} samples",
        if buffer_plan.likely_jittery {
            "jittery"
        } else {
            "steady"
        },
        OUTPUT_BUFFER_FRAMES,
        buffer_plan.target_fill,
        buffer_plan.target_fill / buffer_plan.samples_per_ms,
        buffer_plan.capacity
    );
}

fn build_input_stream<P>(
    route: &AudioRoute,
    producer: P,
    restart_policy: RestartPolicy,
    restart_requested: Arc<AtomicBool>,
) -> Result<cpal::Stream>
where
    P: Producer<Item = f32> + Send + 'static,
{
    let mut input_pipe = InputPipe::new(
        producer,
        route.in_channels,
        route.in_rate,
        route.out_rate,
        route.out_channels,
    );

    route
        .input_device
        .build_input_stream(
            route.input_config,
            move |input: &[f32], _: &cpal::InputCallbackInfo| input_pipe.capture(input),
            input_error_callback(
                restart_policy,
                route.input_description.clone(),
                restart_requested,
            ),
            None,
        )
        .context("failed to build input stream")
}

fn input_error_callback(
    restart_policy: RestartPolicy,
    input_device_description: String,
    restart_requested: Arc<AtomicBool>,
) -> impl FnMut(cpal::Error) + Send + 'static {
    move |err| {
        crate::log_err!("input stream error: {}", err);
        if err.kind() == ErrorKind::DeviceNotAvailable
            && !restart_requested.swap(true, Ordering::Relaxed)
        {
            match &restart_policy {
                RestartPolicy::FollowDefaultInput => {
                    crate::log_out!(
                        "input device disconnected: {}; attempting micpipe restart",
                        input_device_description
                    );
                    request_service_restart();
                }
                RestartPolicy::PinnedInput { name } => {
                    crate::log_out!(
                        "input device disconnected: {}; waiting for pinned input device '{}' to reconnect before restarting",
                        input_device_description,
                        name
                    );
                    request_restart_when_pinned_input_reconnects(
                        name.clone(),
                        Arc::clone(&restart_requested),
                    );
                }
            }
        }
    }
}

fn build_output_stream<C>(
    route: &AudioRoute,
    consumer: C,
    buffer_plan: BufferPlan,
    occupancy: Arc<AtomicUsize>,
) -> Result<cpal::Stream>
where
    C: Consumer<Item = f32> + Send + 'static,
{
    let mut output_pipe = OutputPipe::new(consumer, buffer_plan.target_fill, occupancy);

    route
        .output_device
        .build_output_stream(
            route.output_config,
            move |output: &mut [f32], _: &cpal::OutputCallbackInfo| output_pipe.fill(output),
            |err| crate::log_err!("output stream error: {}", err),
            None,
        )
        .context("failed to build output stream")
}

#[cfg(target_os = "macos")]
fn watch_default_input_changes_when_needed(
    route: &AudioRoute,
    restart_policy: &RestartPolicy,
    restart_requested: Arc<AtomicBool>,
) -> Result<Option<DefaultInputChangeListener>> {
    if matches!(restart_policy, RestartPolicy::FollowDefaultInput) {
        return Ok(Some(watch_default_input_changes(
            route.input_description.clone(),
            restart_requested,
        )?));
    }

    Ok(None)
}

fn spawn_buffer_logger(debug: bool, occupancy: Arc<AtomicUsize>, buffer_plan: BufferPlan) {
    if !debug {
        return;
    }

    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let occ = occupancy.load(Ordering::Relaxed);
            let pct = (occ as f32 / buffer_plan.capacity as f32) * 100.0;
            crate::log_out!(
                "buffer: {} / {} samples ({:.1}%)",
                occ,
                buffer_plan.capacity,
                pct
            );
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RestartPolicy {
    FollowDefaultInput,
    PinnedInput { name: String },
}

impl RestartPolicy {
    fn from_args(args: &RunArgs) -> Self {
        match &args.input {
            Some(input) => Self::PinnedInput {
                name: input.clone(),
            },
            None => Self::FollowDefaultInput,
        }
    }
}

struct AudioRoute {
    input_device: cpal::Device,
    input_config: StreamConfig,
    input_description: String,
    in_channels: usize,
    in_rate: u32,
    output_device: cpal::Device,
    output_config: StreamConfig,
    output_description: String,
    out_channels: usize,
    out_rate: u32,
}

impl AudioRoute {
    fn from_args(args: &RunArgs) -> Result<Self> {
        let input_device = find_input_device(args.input.as_deref())?;
        let input_config = input_device
            .default_input_config()
            .context("failed to get input device default config")?;
        let in_channels = input_config.channels() as usize;
        let in_rate = input_config.sample_rate();
        let input_description = input_device
            .description()
            .context("failed to describe input device")?
            .to_string();

        let output_device = find_output_device(Some(&args.output))?;
        let output_config = output_device
            .default_output_config()
            .context("failed to get output device default config")?;
        let out_channels = output_config.channels() as usize;
        let out_rate = output_config.sample_rate();
        let output_description = output_device
            .description()
            .context("failed to describe output device")?
            .to_string();

        let input_config = input_config.into();
        let mut output_config: StreamConfig = output_config.into();
        output_config.buffer_size = BufferSize::Fixed(OUTPUT_BUFFER_FRAMES);

        Ok(Self {
            input_device,
            input_config,
            input_description,
            in_channels,
            in_rate,
            output_device,
            output_config,
            output_description,
            out_channels,
            out_rate,
        })
    }
}

struct InputPipe<P>
where
    P: Producer<Item = f32>,
{
    producer: P,
    in_channels: usize,
    converted: Vec<f32>,
    resampled: Vec<f32>,
    resampler: Resampler,
}

impl<P> InputPipe<P>
where
    P: Producer<Item = f32>,
{
    fn new(
        producer: P,
        in_channels: usize,
        in_rate: u32,
        out_rate: u32,
        out_channels: usize,
    ) -> Self {
        Self {
            producer,
            in_channels,
            converted: vec![0.0; out_channels],
            resampled: Vec::with_capacity(RESAMPLED_SCRATCH_CAPACITY),
            resampler: Resampler::new(in_rate, out_rate, out_channels),
        }
    }

    fn capture(&mut self, input: &[f32]) {
        for frame in input.chunks_exact(self.in_channels) {
            convert_frame(frame, self.in_channels, &mut self.converted);

            self.resampled.clear();
            self.resampler.process(&self.converted, &mut self.resampled);

            for &sample in &self.resampled {
                // Drop samples if the buffer is full; never block the audio thread.
                let _ = self.producer.try_push(sample);
            }
        }
    }
}

struct OutputPipe<C>
where
    C: Consumer<Item = f32>,
{
    consumer: C,
    primed: bool,
    occupancy: Arc<AtomicUsize>,
    target_fill: usize,
}

impl<C> OutputPipe<C>
where
    C: Consumer<Item = f32>,
{
    fn new(consumer: C, target_fill: usize, occupancy: Arc<AtomicUsize>) -> Self {
        Self {
            consumer,
            primed: false,
            occupancy,
            target_fill,
        }
    }

    fn fill(&mut self, output: &mut [f32]) {
        let available = self.consumer.occupied_len();

        // Observe buffer fill before draining. This is real-time safe: just an atomic store.
        self.occupancy.store(available, Ordering::Relaxed);

        // Wait for the cushion to build before the first non-silent output.
        if !self.primed {
            if available >= self.target_fill {
                self.primed = true;
            } else {
                output.fill(0.0);
                return;
            }
        }

        // If we fully underrun, re-arm the gate so we rebuild the cushion instead of stuttering.
        for out_sample in output.iter_mut() {
            if let Some(sample) = self.consumer.try_pop() {
                *out_sample = sample;
            } else {
                *out_sample = 0.0;
                self.primed = false;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BufferPlan {
    likely_jittery: bool,
    samples_per_ms: usize,
    target_fill: usize,
    capacity: usize,
}

impl BufferPlan {
    fn new(in_rate: u32, out_rate: u32, out_channels: usize) -> Self {
        let likely_jittery = in_rate <= 24_000;
        let samples_per_ms = ((out_rate as usize * out_channels) / 1_000).max(1);
        let callback_samples = OUTPUT_BUFFER_FRAMES as usize * out_channels;
        let jitter_extra = if likely_jittery {
            JITTERY_EXTRA_MARGIN_MS as usize * samples_per_ms
        } else {
            0
        };

        // Cushion = baseline phase-offset coverage + (jittery margin if applicable).
        let target_fill = STEADY_CUSHION_CALLBACKS * callback_samples + jitter_extra;
        let capacity = (target_fill * 4).max(callback_samples * 8);

        Self {
            likely_jittery,
            samples_per_ms,
            target_fill,
            capacity,
        }
    }
}

fn request_service_restart() {
    crate::log_err!("requesting micpipe restart");
    std::thread::spawn(|| match service::restart_service() {
        Ok(status) if status.success() => {
            crate::log_err!("micpipe restart requested");
        }
        Ok(status) => {
            crate::log_err!("micpipe restart failed: {}", status);
        }
        Err(err) => {
            crate::log_err!("failed to request micpipe restart: {}", err);
        }
    });
}

#[cfg(target_os = "macos")]
fn watch_default_input_changes(
    current_input_description: String,
    restart_requested: Arc<AtomicBool>,
) -> Result<DefaultInputChangeListener> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let listener = DefaultInputChangeListener::new(sender)?;

    std::thread::spawn(move || {
        for () in receiver {
            let default_input_description = match default_input_description() {
                Ok(description) => description,
                Err(err) => {
                    crate::log_err!("failed to inspect default input device change: {}", err);
                    continue;
                }
            };

            if default_input_description == current_input_description {
                continue;
            }

            if restart_requested.swap(true, Ordering::Relaxed) {
                break;
            }

            crate::log_out!(
                "default input changed: {} -> {}; attempting micpipe restart",
                current_input_description,
                default_input_description
            );
            request_service_restart();
            break;
        }
    });

    Ok(listener)
}

#[cfg(target_os = "macos")]
fn default_input_description() -> Result<String> {
    Ok(find_input_device(None)?
        .description()
        .context("failed to describe default input device")?
        .to_string())
}

fn request_restart_when_pinned_input_reconnects(input: String, restart_requested: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(PINNED_INPUT_RECONNECT_POLL_INTERVAL);
            let Ok(device) = find_input_device(Some(&input)) else {
                continue;
            };

            if restart_requested.swap(true, Ordering::Relaxed) {
                break;
            }

            let device_description = device
                .description()
                .map_or_else(|_| input.clone(), |description| description.to_string());
            crate::log_out!(
                "pinned input device reconnected: {}; attempting micpipe restart",
                device_description
            );
            request_service_restart();
            break;
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use ringbuf::{
        HeapRb,
        traits::{Consumer, Producer, Split},
    };

    use super::{
        BufferPlan, InputPipe, OUTPUT_BUFFER_FRAMES, OutputPipe, RestartPolicy,
        STEADY_CUSHION_CALLBACKS,
    };
    use crate::cli::RunArgs;

    #[test]
    fn steady_buffer_plan_uses_two_callback_cushion() {
        let plan = BufferPlan::new(48_000, 48_000, 2);
        let callback_samples = OUTPUT_BUFFER_FRAMES as usize * 2;

        assert!(!plan.likely_jittery);
        assert_eq!(
            plan.target_fill,
            STEADY_CUSHION_CALLBACKS * callback_samples
        );
        assert_eq!(plan.capacity, callback_samples * 8);
    }

    #[test]
    fn jittery_buffer_plan_adds_extra_margin() {
        let plan = BufferPlan::new(16_000, 48_000, 2);

        assert!(plan.likely_jittery);
        assert_eq!(plan.samples_per_ms, 96);
        assert_eq!(plan.target_fill, 2_048 + 4_800);
        assert_eq!(plan.capacity, plan.target_fill * 4);
    }

    #[test]
    fn input_pipe_ignores_partial_input_frames() {
        let (producer, mut consumer) = HeapRb::<f32>::new(4).split();
        let mut pipe = InputPipe::new(producer, 2, 48_000, 48_000, 2);

        pipe.capture(&[0.0, 1.0, 0.5]);
        pipe.capture(&[0.25, 0.75]);

        assert_eq!(consumer.try_pop(), Some(0.0));
        assert_eq!(consumer.try_pop(), Some(1.0));
        assert_eq!(consumer.try_pop(), None);
    }

    #[test]
    fn output_pipe_waits_for_cushion_before_draining() {
        let (mut producer, consumer) = HeapRb::<f32>::new(4).split();
        producer.try_push(0.25).unwrap();
        producer.try_push(0.75).unwrap();
        let occupancy = Arc::new(AtomicUsize::new(0));
        let mut pipe = OutputPipe::new(consumer, 3, Arc::clone(&occupancy));
        let mut output = [1.0, 1.0];

        pipe.fill(&mut output);

        assert_eq!(output, [0.0, 0.0]);
        assert_eq!(occupancy.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn output_pipe_drains_after_cushion_is_ready() {
        let (mut producer, consumer) = HeapRb::<f32>::new(4).split();
        producer.try_push(0.25).unwrap();
        producer.try_push(0.75).unwrap();
        let occupancy = Arc::new(AtomicUsize::new(0));
        let mut pipe = OutputPipe::new(consumer, 2, Arc::clone(&occupancy));
        let mut output = [0.0, 0.0];

        pipe.fill(&mut output);

        assert_eq!(output, [0.25, 0.75]);
        assert_eq!(occupancy.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn restart_policy_follows_default_input() {
        let args = RunArgs {
            output: "BlackHole 2ch".to_string(),
            input: None,
            debug: false,
        };

        assert_eq!(
            RestartPolicy::from_args(&args),
            RestartPolicy::FollowDefaultInput
        );
    }

    #[test]
    fn restart_policy_pins_input_by_name() {
        let args = RunArgs {
            output: "BlackHole 2ch".to_string(),
            input: Some("MacBook Pro Microphone".to_string()),
            debug: false,
        };

        assert_eq!(
            RestartPolicy::from_args(&args),
            RestartPolicy::PinnedInput {
                name: "MacBook Pro Microphone".to_string()
            }
        );
    }
}
