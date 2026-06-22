mod audio;
mod cli;
mod resampler;
mod router;
mod service;

use clap::Parser;
use cli::{Cli, Command};

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Run(args) => router::run(args),
        Command::Install(args) => service::install(args),
        Command::Uninstall => service::uninstall(),
        Command::Start => service::start(),
        Command::Stop => service::stop(),
        Command::Restart => service::restart(),
        Command::Status => service::status(),
    }
}
