use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "micpipe",
    version,
    about = "Route your microphone into BlackHole"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the audio driver (this is what the launchd service invokes.)
    Run(RunArgs),
    /// Install and start the launchd service.
    Install(RunArgs),
    /// Remove the launchd service.
    Uninstall,
    /// Start the installed service.
    Start,
    /// stop the running service.
    Stop,
    /// Restart the service.
    Restart,
    /// Show whether the service is installed and running.
    Status,
}

#[derive(clap::Args, Clone)]
pub struct RunArgs {
    /// Output device name to route into (substring match).
    #[arg(short, long, default_value = "BlackHole 2ch")]
    pub output: String,

    /// Input device name (substring match). Omit to follow the system default.
    #[arg(short, long)]
    pub input: Option<String>,

    /// Enable per-second buffer occupancy logging.
    #[arg(short, long)]
    pub debug: bool,
}
