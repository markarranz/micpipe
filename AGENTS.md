# Repository Guidelines

## Project Structure & Module Organization

`micpipe` is a single Rust 2024 binary crate. `src/main.rs` dispatches CLI
commands defined in `src/cli.rs`. Service installation and `launchd` integration
live in `src/service.rs` and `src/plist.template`. Audio device discovery,
routing, buffering, and resampling are split across `audio.rs`, `router.rs`, and
`resampler.rs`. The two macOS CoreAudio watchers are
`default_input_watcher.rs` and `output_usage_watcher.rs`. Tests are colocated
with their modules under `#[cfg(test)]`; there is no separate `tests/` tree.
Read `DESIGN.md` before changing runtime behavior and update `README.md` or
`CHANGELOG.md` when user-visible behavior changes.

## Build, Test, and Development Commands

Use Rust 1.88 or newer. CI runs these commands on macOS:

- `cargo build --locked` builds the binary without changing dependency versions.
- `cargo run -- run` starts the router in the foreground for manual testing.
- `cargo fmt --all --check` verifies rustfmt output.
- `cargo test --all-targets --locked` runs all unit tests.
- `cargo clippy --all-targets --all-features --locked -- -D warnings` enforces
  the repository lint policy.
- `cargo check --target x86_64-apple-darwin --locked` checks Intel macOS support.
- `cargo package --locked` validates the crates.io package contents.

Run `git diff --check` before committing.

## Coding Style & Naming Conventions

Write idiomatic Rust following the
[Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) and
[Microsoft's Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/).
Let rustfmt control indentation and line wrapping. Use `snake_case` for modules,
functions, and tests; `UpperCamelCase` for types; and `SCREAMING_SNAKE_CASE` for
constants. Keep Clippy warnings clean rather than broadly allowing lints. Every
unsafe block needs a nearby `SAFETY` explanation. Keep audio callbacks
non-blocking: avoid allocation, locks, device enumeration, process launches,
logging, and sleeps in the callback path.

## Testing Guidelines

Add focused unit tests beside the behavior being changed. Name tests after the
observable outcome, such as `output_pipe_waits_for_cushion_before_draining`.
Cover recovery policies, buffer boundaries, channel conversion, and error cases
with deterministic fixtures. Note any manual CoreAudio, microphone-permission,
or `launchd` validation in the pull request because CI cannot emulate hardware.

## Commit & Pull Request Guidelines

Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).
Name branches with a matching type prefix and a short kebab-case topic, such as
`feat/output-demand`, `fix/plist-indentation`, `docs/runnable-example`, or
`chore/dependencies`. Format commit subjects as
`<type>[optional scope]: <description>`, for example
`feat(router): gate audio work on output demand`. Keep each commit to one logical
change. Pull requests should explain the user-visible result, important design
choices, and validation performed. Keep the diff focused, update relevant
documentation, and call out macOS-only assumptions or untested hardware
behavior explicitly.
