# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Restart the service when the macOS default input device changes.
- Add CI coverage for formatting, linting, tests, and builds.

### Changed

- Stop active audio streams while waiting for a pinned input device to reconnect.
- Clarify the CLI description.

### Fixed

- Keep the default input change listener's callback state alive for the full
  listener lifetime.
- Use immutable references when registering and unregistering the Core Audio
  property listener address.

## [0.1.0] - 2026-07-02

### Added

- Initial release of `micpipe`.
- Route microphone audio to BlackHole or another Core Audio output device.
- Run in the foreground or as a per-user launchd service.
- Follow the system default input when no input device is pinned.
- Pin input and output devices by case-insensitive name substring.
- Log buffer occupancy with `--debug`.
- Install, start, stop, restart, uninstall, and inspect the launchd service.

[unreleased]: https://github.com/markarranz/micpipe/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/markarranz/micpipe/releases/tag/v0.1.0
