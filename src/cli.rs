use crate::organizer::Organizer;
use crate::Args;
use anyhow::Result;
use colored::*;

pub fn run_cli(args: Args) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        println!(
            "{}",
            "╔══════════════════════════════════════════╗"
                .cyan()
                .bold()
        );
        println!(
            "{}",
            "║   AI File Organizer (Ollama-powered)     ║"
                .cyan()
                .bold()
        );
        println!(
            "{}",
            "╚══════════════════════════════════════════╝"
                .cyan()
                .bold()
        );
        println!();

        let path = match &args.path {
            Some(p) => p.clone(),
            None => {
                anyhow::bail!("Path is required in CLI mode. Use --path <folder>");
            }
        };

        if args.dry_run {
            println!(
                "{}",
                "🔍 DRY RUN MODE - No changes will be made"
                    .yellow()
                    .bold()
            );
            println!();
        }

        println!("📁 Target: {}", path.display().to_string().green());
        println!("🤖 Model:  {}", args.model.green());
        println!("🧵 Threads: {}", args.threads.to_string().green());
        println!();

        if !path.exists() {
            anyhow::bail!("Path does not exist: {}", path.display());
        }

        let ollama = crate::ollama::OllamaClient::new(&args.url, &args.model);
        match ollama.health_check().await {
            Ok(_) => println!("{}", "✅ Ollama server is reachable".green()),
            Err(e) => {
                println!("{}: {}", "❌ Cannot reach Ollama server".red().bold(), e);
                println!("   Make sure Ollama is running: ollama serve");
                anyhow::bail!("Ollama not available");
            }
        }
        println!();

        let config = crate::organizer::OrganizerConfig {
            path,
            dry_run: args.dry_run,
            model: args.model,
            url: args.url,
            threads: args.threads,
            max_read_size: args.max_read_size,
            skip_rename: args.skip_rename,
        };

        let organizer = Organizer::new(config);
        organizer.run(None).await?;

        println!();
        println!("{}", "✨ Done!".green().bold());
        Ok(())
    })
}