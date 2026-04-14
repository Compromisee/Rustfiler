use crate::app_state::SharedState;
use crate::classifier::ClassifiedFile;
use crate::ollama::OllamaClient;
use crate::scanner::is_text_file;
use anyhow::Result;
use colored::*;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

// ─────────────────────────────────────────────────────────────────────────────
// Public type  — Clone required so AppState: Clone
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RenameAction {
    pub original_name: String,
    pub new_name: String,
    pub path: PathBuf,
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

pub async fn generate_renames(
    classified_files: &[ClassifiedFile],
    ollama: &OllamaClient,
    skip_rename: bool,
) -> Result<Vec<RenameAction>> {
    generate_renames_with_progress(classified_files, ollama, skip_rename, None).await
}

pub async fn generate_renames_with_progress(
    classified_files: &[ClassifiedFile],
    ollama: &OllamaClient,
    skip_rename: bool,
    state: Option<SharedState>,
) -> Result<Vec<RenameAction>> {
    if skip_rename {
        println!("{}", "  Skipping AI rename step".yellow());
        return Ok(vec![]);
    }

    let renameable: Vec<&ClassifiedFile> = classified_files
        .iter()
        .filter(|cf| {
            is_text_file(&cf.original.extension)
                && should_consider_rename(&cf.original.filename)
        })
        .collect();

    if renameable.is_empty() {
        println!("{}", "  No files need renaming".dimmed());
        return Ok(vec![]);
    }

    println!(
        "  Analysing {} files for potential renaming…",
        renameable.len().to_string().cyan()
    );

    let renames: Arc<Mutex<Vec<RenameAction>>> = Arc::new(Mutex::new(Vec::new()));
    let total = renameable.len();
    let processed_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let batch_size = 4usize;

    for chunk in renameable.chunks(batch_size) {
        let mut handles = Vec::new();

        for cf in chunk.iter() {
            let ollama        = ollama.clone();
            let filename      = cf.original.filename.clone();
            let preview       = cf.original.content_preview.clone();
            let path          = cf.original.path.clone();
            let ai_suggestion = cf.classification.suggested_name.clone();
            let renames       = renames.clone();
            let processed     = processed_count.clone();
            let state         = state.clone();

            let handle = tokio::spawn(async move {
                // Prefer the AI's classification suggestion; fall back to a
                // dedicated rename prompt; fall back to keeping the original.
                let new_name = if !ai_suggestion.is_empty() && ai_suggestion != filename {
                    ai_suggestion
                } else {
                    match ollama.suggest_rename(&filename, &preview).await {
                        Ok(n) => n,
                        Err(e) => {
                            tracing::warn!("suggest_rename failed for {}: {}", filename, e);
                            filename.clone()
                        }
                    }
                };

                if new_name != filename && !new_name.is_empty() {
                    renames.lock().await.push(RenameAction {
                        original_name: filename.clone(),
                        new_name,
                        path,
                    });
                }

                let done =
                    processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;

                if let Some(ref s) = state {
                    let mut st = s.write().await;
                    // Renames occupy 70 %–90 % of the overall bar.
                    st.progress = (0.7 + (done as f32 / total as f32) * 0.2).min(0.9);
                    st.progress_message =
                        format!("Analysing renames… {done}/{total}");
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            if let Err(e) = handle.await {
                tracing::error!("Rename task panicked: {:?}", e);
            }
        }
    }

    let final_renames = Arc::try_unwrap(renames)
        .expect("Arc still has multiple owners after all tasks finished")
        .into_inner();

    println!(
        "  {} files will be renamed",
        final_renames.len().to_string().green()
    );

    Ok(final_renames)
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: should we even try to rename this file?
// ─────────────────────────────────────────────────────────────────────────────

fn should_consider_rename(filename: &str) -> bool {
    let name = filename.to_lowercase();

    // Well-known filenames that must never be renamed
    const SKIP: &[&str] = &[
        "cargo.toml",
        "cargo.lock",
        "package.json",
        "package-lock.json",
        "tsconfig.json",
        "webpack.config",
        ".gitignore",
        ".gitattributes",
        ".editorconfig",
        "makefile",
        "dockerfile",
        "readme",
        "license",
        "changelog",
        "contributing",
        "go.mod",
        "go.sum",
        "gemfile",
        "rakefile",
        "procfile",
        ".env",
        "pyproject.toml",
        "setup.py",
        "setup.cfg",
        "requirements.txt",
        "pipfile",
        "yarn.lock",
        "pnpm-lock.yaml",
        "bun.lockb",
        "flake.nix",
        "flake.lock",
        "shell.nix",
        "default.nix",
    ];

    if SKIP.iter().any(|p| name.contains(p)) {
        return false;
    }

    // Generic stems that Ollama might be able to improve
    const GENERIC: &[&str] = &[
        "untitled",
        "new_file",
        "new file",
        "temp",
        "tmp",
        "test",
        "document",
        "file",
        "data",
        "output",
        "result",
        "draft",
        "copy",
        "backup",
        "misc",
        "stuff",
        "notes",
        "note",
    ];

    let stem = std::path::Path::new(filename)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    GENERIC.iter().any(|g| {
        stem == *g
            || stem.starts_with(&format!("{g}_"))
            || stem.starts_with(&format!("{g} "))
            // e.g. "untitled1", "temp2"
            || stem.trim_end_matches(|c: char| c.is_ascii_digit()) == *g
    })
}