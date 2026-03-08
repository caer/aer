use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

/// Entrypoint.
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
        Commands::Init => aer::tool::init().await,
        Commands::Palette => aer::tool::palette::run(),
        Commands::Procs {
            procs_file,
            profile,
        } => aer::tool::procs::run(procs_file.as_deref(), profile.as_deref()).await,
        Commands::Serve { port, profile } => aer::tool::serve::run(port, profile.as_deref()).await,
        Commands::Kits { command } => match command {
            KitsCommand::Refresh { kit, update } => {
                let config_path = Path::new(aer::tool::DEFAULT_CONFIG_FILE);
                let loaded = aer::tool::load_config(config_path, None).await?;
                aer::tool::kits::refresh_kits(
                    &loaded.kits,
                    &loaded.config_dir,
                    kit.as_deref(),
                    update,
                )
                .await
            }
        },
    }
}

/// Top-level CLI arguments.
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

/// CLI sub-commands.
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

    /// Start a development server with file watching that
    /// runs all asset processors configured in Aer.toml.
    Serve {
        /// Port to listen on
        #[arg(long, default_value = "1337")]
        port: u16,

        /// Profile to use (merges with default)
        #[arg(short, long)]
        profile: Option<String>,
    },

    /// Manage Kits.
    Kits {
        #[command(subcommand)]
        command: KitsCommand,
    },
}

/// Kit management sub-commands.
#[derive(Subcommand)]
enum KitsCommand {
    /// Refresh kits from their git sources.
    Refresh {
        /// Refresh only this kit.
        #[arg(long)]
        kit: Option<String>,
        /// Force refresh, even if the local copy appears up to date.
        #[arg(long)]
        update: bool,
    },
}
