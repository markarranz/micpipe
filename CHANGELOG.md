# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-07-10

### Added

- Start audio streams only while another app actively uses the selected output
  as an input.
- Restart the service when the macOS default input device changes.

### Changed

- Adapt output buffering to the selected device's supported buffer range.
- Stop active audio streams while waiting for a pinned input device to reconnect.
- Clarify `micpipe`'s virtual-microphone use case and add a runnable BlackHole
  routing example.

### Fixed

- Correct generated launchd plist argument indentation.

## [0.1.0] - 2026-07-02

### Added

- Initial release of `micpipe`.
- Route microphone audio to BlackHole or another Core Audio output device.
- Run in the foreground or as a per-user launchd service.
- Follow the system default input when no input device is pinned.
- Pin input and output devices by case-insensitive name substring.
- Log buffer occupancy with `--debug`.
- Install, start, stop, restart, uninstall, and inspect the launchd service.

[unreleased]: https://github.com/markarranz/micpipe/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/markarranz/micpipe/releases/tag/v0.2.0
[0.1.0]: https://github.com/markarranz/micpipe/releases/tag/v0.1.0
