# micpipe

`micpipe` is a lightweight macOS audio router for sending your microphone into
`BlackHole 2ch` or another CoreAudio output. This lets a call, meeting, or
recording app use BlackHole as its microphone input while receiving both your
voice and other audio routed to the virtual device.

Run it in the foreground for testing, or install it as a per-user `launchd`
service.

## What it does

- Routes one input device to one output device.
- Runs as a per-user `launchd` service.
- Follows the system default input, or pins a specific input by name.
- Restarts automatically when the system default input changes.
- Restarts automatically when the default input disconnects.
- Waits for a pinned input to reconnect, then restarts automatically.
- On macOS, starts audio streams only while another app is actively using the
  selected output as an input.
- Stops the audio streams when no app is actively reading the selected output.

## What it is not

`micpipe` is not a full multi-source mixer. It does not itself combine multiple
inputs, expose gain controls, or provide mute/solo controls. Instead, it routes
one microphone input to one output; other apps can send additional audio to
BlackHole separately.

## Requirements

- macOS
- Rust 1.88 or newer, with Cargo
- BlackHole, or another CoreAudio output device to receive the microphone audio
- Microphone permission for the terminal or installed binary that runs `micpipe`

## Install

Install from crates.io:

```bash
cargo install micpipe
```

Install from a source checkout:

```bash
cargo install --path .
```

## Quick start

Run in the foreground with the default route:

```bash
micpipe run
```

By default, `micpipe run` follows the system default input device and routes it
to the first output device whose description contains `BlackHole 2ch`.

On macOS, `micpipe` waits for an app to actively select that output as an input
before it starts the audio streams. For example, select `BlackHole 2ch` as the
microphone in a calling app, then join or start the call. Configure the other
audio you want to share to use `BlackHole 2ch` as its output as well.

Install and start the background service:

```bash
micpipe install --output "BlackHole 2ch"
```

Pin explicit devices when you want stable matching by device-name substring:

```bash
micpipe install \
  --input "MacBook Pro Microphone" \
  --output "BlackHole 2ch"
```

Use `run` instead of `install` for foreground testing:

```bash
micpipe run \
  --input "MacBook Pro Microphone" \
  --output "BlackHole 2ch"
```

Add `--debug` to log buffer occupancy once per second:

```bash
micpipe run --debug
```

## Runnable example: share audio and hear it locally

This test uses built-in macOS tools to record your microphone and a generated
voice through BlackHole while playing the same generated voice through your
current system output. It does not require a Multi-Output Device or pinning an
input in `micpipe`.

1. Open **QuickTime Player**, choose **File > New Audio Recording**, and select
   `BlackHole 2ch` as the recording's microphone. Keep the monitoring volume at
   zero during the test to prevent feedback.
2. In one Terminal window, start `micpipe` and leave it running:

   ```bash
   micpipe run
   ```

   With no `--input`, `micpipe` follows your current default microphone.
3. Start recording in QuickTime, then wait until the first Terminal reports
   `Mic -> BlackHole 2ch running while output is being used as input`.
4. Speak into your microphone and run this in a second Terminal window:

   ```bash
   say -a "BlackHole 2ch" "This audio is playing through BlackHole." &
   say "This audio is playing through BlackHole."
   wait
   ```

The first `say` process sends the phrase to BlackHole while the second sends it
to your current system output, so you should hear it immediately. Stop and play
the QuickTime recording; it should contain both your microphone from `micpipe`
and the generated voice sent to BlackHole. Press Control-C in the first Terminal
window when you are finished.

For a call, select `BlackHole 2ch` as the call app's microphone and send the
audio you want to share to BlackHole. Keep the call app's speaker output on your
headphones or speakers so its incoming audio is not fed back into BlackHole.

## Why it exists

`micpipe` was built for Tuple's
[Dad Joke Greeter](https://tuple.app/triggers/directory/dad-joke-greeter), which
speaks a joke when someone joins a room. The greeter sends generated audio to
BlackHole while `micpipe` sends the user's microphone to the same virtual
device. Tuple selects BlackHole as its microphone and receives both.

The same routing can be used with another call, meeting, or recording app that
accepts BlackHole as a microphone input.

## macOS setup notes

Install BlackHole separately before using the default output route.

Run `micpipe run` from the terminal, then select its output as an input in an
app if macOS needs to prompt for microphone permission. The permission belongs
to the app or binary that starts `micpipe`, so the foreground test is the
easiest way to confirm capture works before installing the service.

## CLI reference

```text
micpipe run [--input <name>] [--output <name>] [--debug]
micpipe install [--input <name>] [--output <name>] [--debug]
micpipe status
micpipe restart
micpipe stop
micpipe start
micpipe uninstall
```

`--output` is a case-insensitive substring match and defaults to
`BlackHole 2ch`.

`--input` is optional:

- Without `--input`, `micpipe` follows the system default input device.
  If the system default input changes, or if the current default input
  disconnects, `micpipe` logs the change and immediately asks the installed
  service to restart.
- With `--input`, `micpipe` pins that input by case-insensitive substring.
  If the pinned input disconnects, `micpipe` logs the disconnected device, stops
  the active audio streams, polls every 5 seconds until that input appears
  again, then asks the installed service to restart.

Output stream errors are logged, but do not currently trigger a restart.

## Service

`micpipe install` records the current executable path in the plist. When you
install from crates.io, that is usually the `micpipe` binary under Cargo's bin
directory.

Useful service commands:

```bash
micpipe status
micpipe restart
micpipe stop
micpipe start
micpipe uninstall
```

The service label is `com.markarranz.micpipe`, and the generated plist lives at:

```text
~/Library/LaunchAgents/com.markarranz.micpipe.plist
```

Re-run `micpipe uninstall` followed by `micpipe install ...` when you want to
change the installed route arguments.

## Logs

The service writes logs under:

```text
~/.local/share/micpipe/out.log
~/.local/share/micpipe/err.log
```

Watch both logs with:

```bash
tail -f ~/.local/share/micpipe/out.log ~/.local/share/micpipe/err.log
```

Log lines are timestamped in the user's local timezone with a numeric offset,
for example:

```text
[2026-06-29T13:04:05-07:00] Mic -> BlackHole 2ch running while output is being used as input
```

Human-facing lifecycle and recovery messages go to `out.log`. Raw stream errors
and restart-command failures go to `err.log`.

When the default input disconnects, `out.log` includes a recovery message like:

```text
[2026-06-29T13:04:05-07:00] input device disconnected: MacBook Pro Microphone; attempting micpipe restart
```

When the system default input changes without a disconnect, `out.log` includes:

```text
[2026-06-29T13:04:05-07:00] default input changed: MacBook Pro Microphone -> USB Microphone; attempting micpipe restart
```

When a pinned input disconnects, `out.log` records that `micpipe` is waiting for
that device before restarting. The running process remains alive as a reconnect
monitor, but the active audio streams are stopped:

```text
[2026-06-29T13:04:05-07:00] input device disconnected: USB Microphone; waiting for pinned input device 'USB Microphone' to reconnect before restarting
[2026-06-29T13:04:05-07:00] audio streams stopped while waiting for pinned input reconnect
[2026-06-29T13:04:10-07:00] pinned input device reconnected: USB Microphone; attempting micpipe restart
```

## Troubleshooting

### No output device matching BlackHole

Install BlackHole, confirm it appears in macOS audio devices, or pass a
different output substring:

```bash
micpipe run --output "Your Output Device"
```

### No default input device

Select a default microphone in macOS Sound settings, or pass an explicit input:

```bash
micpipe run --input "Your Microphone"
```

### Audio streams are idle

On macOS, this is expected until another app actively uses the selected output
as an input. Select the exact output device in the app's microphone settings and
start the app's input, such as by joining a call. Merely showing the device in
an audio menu may not start an input stream.

### Service installed but not loaded

Start it again and then inspect status:

```bash
micpipe start
micpipe status
```

### Logs show input changed or disconnected

If you are following the default input, `micpipe` attempts an immediate service
restart when the default changes or disconnects. If you pinned an input with
`--input`, reconnect that same device; `micpipe` polls every 5 seconds and
restarts after it appears again.

### Route arguments changed

Uninstall and reinstall the service so the plist is regenerated with the new
arguments:

```bash
micpipe uninstall
micpipe install --input "Your Microphone" --output "BlackHole 2ch"
```

## Development

Run the checks before committing changes:

```bash
cargo fmt
cargo test
cargo clippy --all-targets --all-features
git diff --check
```

After changing the binary for an installed service:

```bash
cargo build --release --bin micpipe
cp target/release/micpipe ~/.local/bin/micpipe
micpipe restart
tail -f ~/.local/share/micpipe/out.log ~/.local/share/micpipe/err.log
```

See [DESIGN.md](DESIGN.md) for implementation details.
