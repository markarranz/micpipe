# micpipe

`micpipe` routes a macOS input device into a macOS output device. It is built
for the common workflow of sending a microphone into `BlackHole 2ch`, then
using BlackHole as the microphone source in another app.

It can run in the foreground while you are testing, or as a per-user `launchd`
agent for day-to-day use.

## Requirements

- macOS
- A Rust toolchain
- BlackHole, or another CoreAudio output device to receive the microphone audio
- Microphone permission for the terminal or installed binary that runs `micpipe`

## Quick start

Build and run with the default route:

```bash
cargo build --release --bin micpipe
./target/release/micpipe run
```

By default, `micpipe run` follows the system default input device and routes it
to the first output device whose description contains `BlackHole 2ch`.

Pin explicit devices when you want stable matching by device-name substring:

```bash
./target/release/micpipe run \
  --input "MacBook Pro Microphone" \
  --output "BlackHole 2ch"
```

Add `--debug` to log buffer occupancy once per second:

```bash
./target/release/micpipe run --debug
```

## Device selection

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

Install `micpipe` from the path where you want `launchd` to run it. The install
command records the current executable path in the plist.

```bash
cargo build --release --bin micpipe
mkdir -p ~/.local/bin
cp target/release/micpipe ~/.local/bin/micpipe
~/.local/bin/micpipe install --output "BlackHole 2ch"
```

To install with a pinned input:

```bash
~/.local/bin/micpipe install \
  --input "MacBook Pro Microphone" \
  --output "BlackHole 2ch"
```

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
