# micpipe design

`micpipe` is a small CoreAudio routing service. It captures samples from an
input device, converts and resamples them to the selected output format, then
writes them into an output device such as BlackHole.

## Goals

- Keep the audio callbacks non-blocking.
- Avoid per-frame allocation in the hot path.
- Run cleanly as a per-user `launchd` service.
- Recover from input-device changes or disconnects in the mode the user
  configured.
- Keep the implementation explicit and testable without adding a large
  framework.

## High-level flow

```text
CLI -> service/router
router -> CPAL input stream -> InputPipe -> ring buffer -> OutputPipe -> CPAL output stream
```

`src/main.rs` parses the CLI and dispatches commands. `src/service.rs` owns the
`launchd` plist lifecycle. `src/router.rs` owns the runtime audio route.
`AudioRuntime` keeps the stream handles, optional default-input listener, and
control channel alive for the duration of `run`.

## Modules

- `src/cli.rs`: subcommands and shared run/install arguments.
- `src/default_input_watcher.rs`: macOS Core Audio listener for default input
  device changes.
- `src/service.rs`: install, uninstall, start, stop, restart, status, plist
  rendering, and `launchctl` calls.
- `src/audio.rs`: CPAL device lookup and frame channel conversion.
- `src/router.rs`: stream setup, buffer sizing, callbacks, restart policy, and
  runtime orchestration.
- `src/resampler.rs`: streaming linear frame resampler.
- `src/logging.rs`: timestamped stdout/stderr log helpers.
- `src/error.rs`: lightweight error/context helpers.

## Route setup

`AudioRoute::from_args` resolves the input and output devices before any stream
is created. A missing `--input` uses CPAL's default input device. A provided
`--input` or `--output` is matched as a case-insensitive substring against
device descriptions.

The input stream uses the device default input config. The output stream uses the
device default output config, but prefers a 512-frame output buffer. When CPAL
reports the selected output device's supported buffer range, `micpipe` clamps
that preference into the device range before opening the stream. If the range is
unknown, it falls back to requesting the preferred fixed size.

## Buffering

The input and output callbacks communicate through a `ringbuf::HeapRb<f32>`.
The buffer size is chosen by `BufferPlan` from the output callback size selected
during route setup:

- A steady input gets a two-output-callback cushion.
- Inputs at 24 kHz or below are treated as likely jittery and get an additional
  50 ms margin.
- Capacity is larger than the target fill so short bursts do not immediately
  overflow.

The output side waits until the ring buffer reaches the target fill before
playing non-silence. If the output side fully underruns, it writes silence and
re-arms that priming gate so playback resumes after the cushion is rebuilt.

## Input callback

`InputPipe` owns the producer side of the ring buffer, channel-conversion
scratch space, resampling scratch space, and `Resampler`.

For each complete input frame:

1. `convert_frame` maps the input channel count to the output channel count.
2. `Resampler::process` appends zero or more output frames.
3. Samples are pushed into the ring buffer with `try_push`.

Samples are dropped when the ring buffer is full. The callback never waits for
the output side.

## Output callback

`OutputPipe` owns the consumer side of the ring buffer and the priming state. It
stores observed occupancy in an `Arc<AtomicUsize>` so the optional debug logger
can report buffer fill without touching the audio callback state.

When there are not enough samples to start or continue cleanly, the callback
writes silence.

## Resampling

The resampler is a streaming linear interpolator over whole frames. It keeps the
previous and next frames and advances a fractional position by `in_rate /
out_rate` for each emitted output frame.

This is intentionally simple. It is good enough for a microphone-monitoring
utility, but it is not meant to be a studio-quality sample-rate converter.

## Input recovery

Input stream errors are always written to `err.log`. When CPAL reports
`ErrorKind::DeviceNotAvailable`, the input error callback also writes a
human-facing recovery message to `out.log` using the input device description
captured during route setup.

The restart policy depends on how input was configured:

- Default input mode: if `--input` was omitted, `micpipe` immediately requests
  `micpipe restart` through the installed `launchd` service when the current
  input disconnects. On macOS it also registers a Core Audio listener for
  `kAudioHardwarePropertyDefaultInputDevice`, so a manual system default-input
  change triggers the same restart path without waiting for a disconnect.
- Pinned input mode: if `--input` was provided, `micpipe` starts a watcher
  thread that checks every 5 seconds for a matching input device. It also sends
  a control message to the runtime so the active input and output streams are
  dropped while the process stays alive as a reconnect monitor. Once the pinned
  input reappears, the watcher requests the service restart.

Restart requests are made on helper threads, not inside the CPAL callback.
Default input change notifications are deduplicated with disconnect-triggered
restart requests so the service is asked to restart only once for the first
recovery event.

Output stream errors are logged only. There is no output-device reconnect
policy today.

## Service model

`micpipe install` writes:

```text
~/Library/LaunchAgents/com.markarranz.micpipe.plist
```

The plist has `RunAtLoad` and `KeepAlive` enabled, writes stdout/stderr to
`~/.local/share/micpipe`, and stores the exact executable path returned by
`std::env::current_exe()`.

`micpipe restart` and automatic restart recovery both use:

```text
launchctl kickstart -k gui/$UID/com.markarranz.micpipe
```

That means automatic restart recovery is useful when `micpipe` is running as an
installed, loaded service. In foreground mode, the restart request can fail and
the failure is logged.

## Logging

`log_out!` and `log_err!` prepend local-time timestamps before writing to stdout
and stderr. On Unix, local timestamps use `localtime_r`; if local conversion
fails, formatting falls back to UTC with a `+00:00` offset.

The log format is:

```text
[YYYY-MM-DDTHH:MM:SS-07:00] message
```

## Concurrency

The steady-state runtime has:

- CPAL's input callback.
- CPAL's output callback.
- An optional debug logger thread.
- A main runtime loop waiting for control messages.
- A restart thread for immediate default-input recovery, a Core Audio listener
  thread for default-input changes, or a reconnect watcher thread for
  pinned-input recovery.

The audio callbacks do not take mutexes, call `launchctl`, enumerate devices, or
sleep. Shared debug state is a single relaxed atomic occupancy value. In pinned
input mode, the input error callback sends a `StopAudioWork` control message;
the main runtime loop handles that message by dropping the stream handles and
stopping the debug logger.

## Tests

The current test coverage focuses on behavior that can be checked without
owning real audio devices:

- Channel conversion.
- Device-description matching.
- Resampler behavior.
- Buffer sizing.
- Input and output pipe behavior.
- Restart-policy selection.
- Plist rendering and XML escaping.
- Timestamp formatting.

## Known constraints

- Device pinning uses description substring matching, not stable CoreAudio
  device IDs.
- Pinned-device reconnect detection is polling based, with a 5 second interval.
- Default-input change detection is macOS-specific and uses Core Audio HAL
  property notifications.
- Output disconnects do not trigger restart or reconnect handling.
- The main runtime loop is still process-lifetime scoped; shutdown is currently
  handled by process termination or `launchd`.
- The resampler favors simplicity over high-fidelity conversion.
