use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use cpal::{
    BufferSize, ErrorKind, StreamConfig, SupportedBufferSize,
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
#[cfg(target_os = "macos")]
use crate::output_usage_watcher::OutputUsageWatcher;

// Preferred output callback size. The selected output device may clamp this.
const REQUESTED_OUTPUT_BUFFER_FRAMES: u32 = 512;
const STEADY_CUSHION_CALLBACKS: usize = 2;
const JITTERY_EXTRA_MARGIN_MS: u32 = 50;
const RESAMPLED_SCRATCH_CAPACITY: usize = 8192;
const PINNED_INPUT_RECONNECT_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub fn run(args: &RunArgs) -> Result<()> {
    let runtime = AudioRuntime::start(args)?;
    runtime.park();
}

struct AudioRuntime {
    route: AudioRoute,
    buffer_plan: BufferPlan,
    restart_policy: RestartPolicy,
    restart_requested: Arc<AtomicBool>,
    audio_control_sender: std::sync::mpsc::Sender<AudioControl>,
    debug: bool,
    input_stream: Option<cpal::Stream>,
    output_stream: Option<cpal::Stream>,
    audio_work_active: Option<Arc<AtomicBool>>,
    output_in_use: bool,
    audio_control_receiver: std::sync::mpsc::Receiver<AudioControl>,
    #[cfg(target_os = "macos")]
    _output_usage_watcher: OutputUsageWatcher,
    #[cfg(target_os = "macos")]
    _default_input_change_listener: Option<DefaultInputChangeListener>,
}

impl AudioRuntime {
    fn start(args: &RunArgs) -> Result<Self> {
        let restart_policy = RestartPolicy::from_args(args);
        let route = AudioRoute::from_args(args)?;
        log_route(&route);

        let buffer_plan = BufferPlan::new(
            route.in_rate,
            route.out_rate,
            route.out_channels,
            route.output_buffer_frames,
        );
        log_buffer_plan(buffer_plan);

        let restart_requested = Arc::new(AtomicBool::new(false));
        let (audio_control_sender, audio_control_receiver) = std::sync::mpsc::channel();

        #[cfg(target_os = "macos")]
        let output_usage_watcher =
            OutputUsageWatcher::start(&route.output_device, audio_control_sender.clone())?;

        #[cfg(target_os = "macos")]
        let default_input_change_listener = watch_default_input_changes_when_needed(
            &route,
            &restart_policy,
            Arc::clone(&restart_requested),
        )?;

        let runtime = Self {
            route,
            buffer_plan,
            restart_policy,
            restart_requested,
            audio_control_sender,
            debug: args.debug,
            input_stream: None,
            output_stream: None,
            audio_work_active: None,
            output_in_use: false,
            audio_control_receiver,
            #[cfg(target_os = "macos")]
            _output_usage_watcher: output_usage_watcher,
            #[cfg(target_os = "macos")]
            _default_input_change_listener: default_input_change_listener,
        };

        // CoreAudio can report process input usage on macOS. Other hosts retain the previous
        // always-on behavior because they do not expose an equivalent demand signal here.
        #[cfg(not(target_os = "macos"))]
        {
            let mut runtime = runtime;
            runtime.output_in_use = true;
            runtime.start_audio_work()?;
            return Ok(runtime);
        }

        #[cfg(target_os = "macos")]
        {
            Ok(runtime)
        }
    }

    fn park(mut self) -> ! {
        loop {
            match self
                .audio_control_receiver
                .recv_timeout(Duration::from_millis(500))
            {
                Ok(AudioControl::StopAudioWork) => {
                    if self.stop_audio_work() {
                        crate::log_out!(
                            "audio streams stopped while waiting for pinned input reconnect"
                        );
                    }
                }
                Ok(AudioControl::OutputUsageChanged(in_use)) => {
                    self.output_in_use = in_use;
                    if in_use {
                        self.try_start_audio_work();
                    } else if self.stop_audio_work() {
                        crate::log_out!(
                            "audio streams stopped because output is no longer being used as input"
                        );
                    } else {
                        crate::log_out!(
                            "waiting for {} to be used as input before starting audio work",
                            self.route.output_description
                        );
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // A transient stream-setup failure should not require the consuming app to
                    // close and reopen its input before micpipe gets another chance.
                    if self.output_in_use && self.input_stream.is_none() {
                        self.try_start_audio_work();
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => std::thread::park(),
            }
        }
    }

    fn try_start_audio_work(&mut self) {
        if self.input_stream.is_some() {
            return;
        }

        if let Err(err) = self.start_audio_work() {
            crate::log_err!("failed to start audio work: {}", err);
        }
    }

    fn start_audio_work(&mut self) -> Result<()> {
        let (producer, consumer) = HeapRb::<f32>::new(self.buffer_plan.capacity).split();

        // Clock-drift instrumentation: shared buffer-occupancy gauge for this active session.
        let occupancy = Arc::new(AtomicUsize::new(0));
        let audio_work_active = Arc::new(AtomicBool::new(true));

        let input_stream = build_input_stream(
            &self.route,
            producer,
            self.restart_policy.clone(),
            Arc::clone(&self.restart_requested),
            self.audio_control_sender.clone(),
        )?;
        let output_stream = build_output_stream(
            &self.route,
            consumer,
            self.buffer_plan,
            Arc::clone(&occupancy),
        )?;

        input_stream
            .play()
            .context("failed to start input stream")?;
        output_stream
            .play()
            .context("failed to start output stream")?;

        crate::log_out!(
            "Mic -> {} running while output is being used as input",
            self.route.output_description
        );

        spawn_buffer_logger(
            self.debug,
            occupancy,
            self.buffer_plan,
            Arc::clone(&audio_work_active),
        );

        self.input_stream = Some(input_stream);
        self.output_stream = Some(output_stream);
        self.audio_work_active = Some(audio_work_active);
        Ok(())
    }
    fn stop_audio_work(&mut self) -> bool {
        let stopped_input = self.input_stream.take().is_some();
        let stopped_output = self.output_stream.take().is_some();
        if let Some(audio_work_active) = self.audio_work_active.take() {
            audio_work_active.store(false, Ordering::Relaxed);
        }
        stopped_input || stopped_output
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

    match route.output_buffer_support {
        OutputBufferSupport::Range { min, max }
            if route.output_buffer_frames == REQUESTED_OUTPUT_BUFFER_FRAMES =>
        {
            crate::log_out!(
                "Output buffer: requested/chosen {} frames (device range {}-{} frames)",
                route.output_buffer_frames,
                min,
                max
            );
        }
        OutputBufferSupport::Range { min, max } => {
            crate::log_out!(
                "Output buffer: requested {} frames, chosen {} frames (device range {}-{} frames)",
                REQUESTED_OUTPUT_BUFFER_FRAMES,
                route.output_buffer_frames,
                min,
                max
            );
        }
        OutputBufferSupport::Unknown => {
            crate::log_out!(
                "Output buffer: requested/chosen {} frames (device range unknown; using fixed-size fallback)",
                route.output_buffer_frames
            );
        }
    }
}

fn log_buffer_plan(buffer_plan: BufferPlan) {
    crate::log_out!(
        "Input {} | output callback {} frames | cushion {} samples (~{}ms) | buffer {} samples",
        if buffer_plan.likely_jittery {
            "jittery"
        } else {
            "steady"
        },
        buffer_plan.output_buffer_frames,
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
    audio_control_sender: std::sync::mpsc::Sender<AudioControl>,
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
                audio_control_sender,
            ),
            None,
        )
        .context("failed to build input stream")
}

fn input_error_callback(
    restart_policy: RestartPolicy,
    input_device_description: String,
    restart_requested: Arc<AtomicBool>,
    audio_control_sender: std::sync::mpsc::Sender<AudioControl>,
) -> impl FnMut(cpal::Error) + Send + 'static {
    move |err| {
        crate::log_err!("input stream error: {}", err);
        if err.kind() == ErrorKind::DeviceNotAvailable
            && !restart_requested.swap(true, Ordering::Relaxed)
        {
            match AudioDisconnectAction::from_policy(&restart_policy) {
                AudioDisconnectAction::RestartServiceNow => {
                    crate::log_out!(
                        "input device disconnected: {}; attempting micpipe restart",
                        input_device_description
                    );
                    request_service_restart();
                }
                AudioDisconnectAction::WaitForPinnedInputReconnectAndStopAudio { input } => {
                    crate::log_out!(
                        "input device disconnected: {}; waiting for pinned input device '{}' to reconnect before restarting",
                        input_device_description,
                        input
                    );
                    request_restart_when_pinned_input_reconnects(input);
                    let _ = audio_control_sender.send(AudioControl::StopAudioWork);
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

fn spawn_buffer_logger(
    debug: bool,
    occupancy: Arc<AtomicUsize>,
    buffer_plan: BufferPlan,
    audio_work_active: Arc<AtomicBool>,
) {
    if !debug {
        return;
    }

    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if !audio_work_active.load(Ordering::Relaxed) {
                break;
            }
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum AudioDisconnectAction {
    RestartServiceNow,
    WaitForPinnedInputReconnectAndStopAudio { input: String },
}

impl AudioDisconnectAction {
    fn from_policy(policy: &RestartPolicy) -> Self {
        match policy {
            RestartPolicy::FollowDefaultInput => Self::RestartServiceNow,
            RestartPolicy::PinnedInput { name } => Self::WaitForPinnedInputReconnectAndStopAudio {
                input: name.clone(),
            },
        }
    }
}

pub(crate) enum AudioControl {
    StopAudioWork,
    OutputUsageChanged(bool),
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
    output_buffer_frames: u32,
    output_buffer_support: OutputBufferSupport,
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
        let output_buffer_selection = choose_output_buffer_frames(
            output_config.buffer_size(),
            REQUESTED_OUTPUT_BUFFER_FRAMES,
        );
        let output_description = output_device
            .description()
            .context("failed to describe output device")?
            .to_string();

        let input_config = input_config.into();
        let mut output_config: StreamConfig = output_config.into();
        output_config.buffer_size = BufferSize::Fixed(output_buffer_selection.frames);

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
            output_buffer_frames: output_buffer_selection.frames,
            output_buffer_support: output_buffer_selection.support,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OutputBufferSelection {
    frames: u32,
    support: OutputBufferSupport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputBufferSupport {
    Range { min: u32, max: u32 },
    Unknown,
}

fn choose_output_buffer_frames(
    supported: &SupportedBufferSize,
    requested: u32,
) -> OutputBufferSelection {
    match *supported {
        SupportedBufferSize::Range { min, max } => OutputBufferSelection {
            frames: requested.clamp(min, max),
            support: OutputBufferSupport::Range { min, max },
        },
        SupportedBufferSize::Unknown => OutputBufferSelection {
            frames: requested,
            support: OutputBufferSupport::Unknown,
        },
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
    output_buffer_frames: u32,
    target_fill: usize,
    capacity: usize,
}

impl BufferPlan {
    fn new(in_rate: u32, out_rate: u32, out_channels: usize, output_buffer_frames: u32) -> Self {
        let likely_jittery = in_rate <= 24_000;
        let samples_per_ms = ((out_rate as usize * out_channels) / 1_000).max(1);
        let callback_samples = output_buffer_frames as usize * out_channels;
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
            output_buffer_frames,
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

fn request_restart_when_pinned_input_reconnects(input: String) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(PINNED_INPUT_RECONNECT_POLL_INTERVAL);
            let Ok(device) = find_input_device(Some(&input)) else {
                continue;
            };

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

    use cpal::SupportedBufferSize;
    use ringbuf::{
        HeapRb,
        traits::{Consumer, Producer, Split},
    };

    use super::{
        AudioDisconnectAction, BufferPlan, InputPipe, OutputBufferSupport, OutputPipe,
        RestartPolicy, STEADY_CUSHION_CALLBACKS, choose_output_buffer_frames,
    };
    use crate::cli::RunArgs;

    #[test]
    fn steady_buffer_plan_uses_two_callback_cushion() {
        let output_buffer_frames = 256;
        let plan = BufferPlan::new(48_000, 48_000, 2, output_buffer_frames);
        let callback_samples = output_buffer_frames as usize * 2;

        assert!(!plan.likely_jittery);
        assert_eq!(plan.output_buffer_frames, output_buffer_frames);
        assert_eq!(
            plan.target_fill,
            STEADY_CUSHION_CALLBACKS * callback_samples
        );
        assert_eq!(plan.capacity, callback_samples * 8);
    }

    #[test]
    fn jittery_buffer_plan_adds_extra_margin() {
        let output_buffer_frames = 256;
        let plan = BufferPlan::new(16_000, 48_000, 2, output_buffer_frames);
        let callback_samples = output_buffer_frames as usize * 2;

        assert!(plan.likely_jittery);
        assert_eq!(plan.samples_per_ms, 96);
        assert_eq!(
            plan.target_fill,
            STEADY_CUSHION_CALLBACKS * callback_samples + 4_800
        );
        assert_eq!(plan.capacity, plan.target_fill * 4);
    }

    #[test]
    fn output_buffer_selection_uses_requested_frames_inside_device_range() {
        let selection = choose_output_buffer_frames(
            &SupportedBufferSize::Range {
                min: 128,
                max: 1024,
            },
            512,
        );

        assert_eq!(selection.frames, 512);
        assert_eq!(
            selection.support,
            OutputBufferSupport::Range {
                min: 128,
                max: 1024
            }
        );
    }

    #[test]
    fn output_buffer_selection_clamps_to_device_range() {
        let too_small = choose_output_buffer_frames(
            &SupportedBufferSize::Range {
                min: 1024,
                max: 2048,
            },
            512,
        );
        let too_large =
            choose_output_buffer_frames(&SupportedBufferSize::Range { min: 128, max: 256 }, 512);

        assert_eq!(too_small.frames, 1024);
        assert_eq!(too_large.frames, 256);
    }

    #[test]
    fn output_buffer_selection_falls_back_to_requested_frames_for_unknown_range() {
        let selection = choose_output_buffer_frames(&SupportedBufferSize::Unknown, 512);

        assert_eq!(selection.frames, 512);
        assert_eq!(selection.support, OutputBufferSupport::Unknown);
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

    #[test]
    fn pinned_input_disconnect_waits_for_reconnect_and_stops_audio_work() {
        let action = AudioDisconnectAction::from_policy(&RestartPolicy::PinnedInput {
            name: "MacBook Pro Microphone".to_string(),
        });

        assert_eq!(
            action,
            AudioDisconnectAction::WaitForPinnedInputReconnectAndStopAudio {
                input: "MacBook Pro Microphone".to_string()
            }
        );
    }
}
