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
    /// Launch the interactive color palette tool
    Palette,
}

fn main() -> std::io::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Palette => aer::tool::palette::run(),
    }
}
