# Minimix

## Development

Copy plist to LaunchAgents directory:

```bash
cp ./com.markarranz.minimix.plist ~/Library/LaunchAgents/
```

After any rebuild, the update cycle is:

```bash
cargo build --release --bin minimix
cp target/release/minimix ~/.local/bin/minimix
launchctl kickstart -k gui/$(id -u)/com.markarranz.minimix
tail -f ~/.local/share/minimix/out.log
```

Useful service management commands:

```bash
# Register/load the service:
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.markarranz.minimix.plist

# Check agent status:
launchctl print gui/$(id -u)/com.markarranz.minimix

# Stop & unload:
launchctl bootout gui/$(id -u)/com.markarranz.minimix

# Restart after rebuilding the binary
launchctl kickstart -k gui/$(id -u)/com.markarranz.minimix

# Watch the logs
tail -f ~/.local/share/minimix/out.log ~/.local/share/minimix/err.log
```
