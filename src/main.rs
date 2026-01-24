use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "aer")]
#[command(about = "A command-line toolkit for creatives", long_about = None)]
struct Cli {
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

fn main() -> std::io::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => aer::tool::procs::init(),
        Commands::Palette => aer::tool::palette::run(),
        Commands::Procs {
            procs_file,
            profile,
        } => aer::tool::procs::run(procs_file.as_deref(), profile.as_deref()),
    }
}
