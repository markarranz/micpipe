mod audio;
mod cli;
#[cfg(target_os = "macos")]
mod default_input_watcher;
mod logging;
#[cfg(target_os = "macos")]
mod output_usage_watcher;
mod resampler;
mod router;
mod service;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => router::run(&args),
        Command::Install(args) => service::install(&args),
        Command::Uninstall => service::uninstall(),
        Command::Start => service::start(),
        Command::Stop => service::stop(),
        Command::Restart => service::restart(),
        Command::Status => service::status(),
    }
}
