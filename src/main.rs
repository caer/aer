use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "aer")]
#[command(about = "A command-line toolkit for creatives", long_about = None)]
struct Cli {
    /// Enable debug logging for troubleshooting
    #[arg(long, global = true)]
    troubles: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Aer project in the current directory
    Init,

    /// Launch the interactive color palette tool
    Palette,

    /// Run asset processors from a TOML configuration file
    Procs {
        /// Path to the TOML configuration file (default: Aer.toml)
        procs_file: Option<PathBuf>,

        /// Profile to use (merges with default)
        #[arg(short, long)]
        profile: Option<String>,
    },
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let cli = Cli::parse();

    // Install tracing subscriber for logging.
    let default_level = if cli.troubles { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level)),
        )
        .without_time()
        .init();

    match cli.command {
        Commands::Init => aer::tool::procs::init().await,
        Commands::Palette => aer::tool::palette::run(),
        Commands::Procs {
            procs_file,
            profile,
        } => aer::tool::procs::run(procs_file.as_deref(), profile.as_deref()).await,
    }
}
