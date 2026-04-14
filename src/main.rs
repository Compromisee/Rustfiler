mod app_state;
mod classifier;
mod cli;
mod ollama;
mod organizer;
mod renamer;
mod scanner;

#[cfg(feature = "gui")]
mod gui;

use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "file-organizer", about = "AI-powered file organizer using Ollama")]
pub struct Args {
    /// Target folder to organize (optional in GUI mode)
    #[arg(short, long)]
    path: Option<std::path::PathBuf>,

    /// Dry run mode - show what would happen without making changes
    #[arg(short, long, default_value_t = false)]
    dry_run: bool,

    /// Ollama model to use
    #[arg(short, long, default_value = "llama3.2")]
    model: String,

    /// Ollama server URL
    #[arg(short, long, default_value = "http://localhost:11434")]
    url: String,

    /// Number of worker threads for file processing
    #[arg(short, long, default_value_t = 4)]
    threads: usize,

    /// Maximum file size to read for content analysis (in bytes)
    #[arg(long, default_value_t = 8192)]
    max_read_size: usize,

    /// Skip AI renaming
    #[arg(long, default_value_t = false)]
    skip_rename: bool,

    /// Run in CLI mode instead of GUI
    #[arg(long, default_value_t = false)]
    cli: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("file_organizer=info".parse()?),
        )
        .init();

    #[cfg(feature = "gui")]
    {
        if args.cli || args.path.is_some() {
            cli::run_cli(args)?;
        } else {
            gui::run_gui(args)?;
        }
    }

    #[cfg(not(feature = "gui"))]
    {
        cli::run_cli(args)?;
    }

    Ok(())
}