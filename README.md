# micpipe

## Development

Copy plist to LaunchAgents directory:

```bash
cp ./com.markarranz.micpipe.plist ~/Library/LaunchAgents/
```

After any rebuild, the update cycle is:

```bash
cargo build --release --bin micpipe
cp target/release/micpipe ~/.local/bin/micpipe
launchctl kickstart -k gui/$(id -u)/com.markarranz.micpipe
tail -f ~/.local/share/micpipe/out.log
```

Useful service management commands:

```bash
# Register/load the service:
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.markarranz.micpipe.plist

# Check agent status:
launchctl print gui/$(id -u)/com.markarranz.micpipe

# Stop & unload:
launchctl bootout gui/$(id -u)/com.markarranz.micpipe

# Restart after rebuilding the binary
launchctl kickstart -k gui/$(id -u)/com.markarranz.micpipe

# Watch the logs
tail -f ~/.local/share/micpipe/out.log ~/.local/share/micpipe/err.log
```
