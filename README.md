# micpipe

`micpipe` is a lightweight macOS audio router service for sending your
microphone into `BlackHole 2ch` or another CoreAudio output. It is built for the
common workflow of using BlackHole as the microphone source in another app while
keeping the route alive in the background.

It can also run in the foreground while you are testing.

## What it does

- Routes one input device to one output device.
- Runs as a per-user `launchd` service.
- Follows the system default input, or pins a specific input by name.
- Restarts automatically when the default input disconnects.
- Waits for a pinned input to reconnect, then restarts automatically.

## What it is not

`micpipe` is not a full multi-source mixer. It does not currently combine
multiple inputs, expose gain controls, or provide mute/solo controls. It is a
small background audio router for keeping a microphone-to-output route alive.

## Requirements

- macOS
- Rust and Cargo
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

## Common use cases

- Send your mic into BlackHole so another app can select BlackHole as its input.
- Keep that route alive as a background service.
- Follow whichever microphone is currently the macOS default.
- Pin a USB or external microphone and restart only after that same device
  reconnects.

## macOS setup notes

Install BlackHole separately before using the default output route.

Run `micpipe run` once from the terminal if macOS needs to prompt for microphone
permission. The permission belongs to the app or binary that starts `micpipe`, so
the foreground test is the easiest way to confirm capture works before installing
the service.

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
  If that input disconnects, `micpipe` logs that the input device was
  disconnected and immediately asks the installed service to restart.
- With `--input`, `micpipe` pins that input by case-insensitive substring.
  If the pinned input disconnects, `micpipe` logs the disconnected device, polls
  every 5 seconds until that input appears again, then asks the installed
  service to restart.

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
[2026-06-29T13:04:05-07:00] Mic -> BlackHole running...
```

Human-facing lifecycle and recovery messages go to `out.log`. Raw stream errors
and restart-command failures go to `err.log`.

When the default input disconnects, `out.log` includes a recovery message like:

```text
[2026-06-29T13:04:05-07:00] input device disconnected: MacBook Pro Microphone; attempting micpipe restart
```

When a pinned input disconnects, `out.log` records that `micpipe` is waiting for
that device before restarting:

```text
[2026-06-29T13:04:05-07:00] input device disconnected: USB Microphone; waiting for pinned input device 'USB Microphone' to reconnect before restarting
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

### Service installed but not loaded

Start it again and then inspect status:

```bash
micpipe start
micpipe status
```

### Logs show input device disconnected

If you are following the default input, `micpipe` attempts an immediate service
restart. If you pinned an input with `--input`, reconnect that same device;
`micpipe` polls every 5 seconds and restarts after it appears again.

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
