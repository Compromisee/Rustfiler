use crate::app_state::SharedState;
use crate::ollama::{FileClassification, OllamaClient, ProjectGroup};
use crate::scanner::ScannedFile;
use anyhow::Result;
use colored::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

// ─────────────────────────────────────────────────────────────────────────────
// Public type  — Clone required so AppState: Clone
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ClassifiedFile {
    pub original: ScannedFile,
    pub classification: FileClassification,
}

// ─────────────────────────────────────────────────────────────────────────────
// Classification
// ─────────────────────────────────────────────────────────────────────────────

/// Convenience wrapper with no GUI state reporting.
pub async fn classify_files(
    files: &[ScannedFile],
    ollama: &OllamaClient,
) -> Result<Vec<ClassifiedFile>> {
    classify_files_with_progress(files, ollama, None).await
}

/// Full version that pushes progress updates into `SharedState` when provided.
pub async fn classify_files_with_progress(
    files: &[ScannedFile],
    ollama: &OllamaClient,
    state: Option<SharedState>,
) -> Result<Vec<ClassifiedFile>> {
    let results: Arc<Mutex<Vec<ClassifiedFile>>> = Arc::new(Mutex::new(Vec::new()));
    let total = files.len();
    let processed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let batch_size = 8usize;

    for chunk in files.chunks(batch_size) {
        let mut handles = Vec::new();

        for file in chunk {
            let ollama    = ollama.clone();
            let file      = file.clone();
            let results   = results.clone();
            let processed = processed.clone();
            let state     = state.clone();

            let handle = tokio::spawn(async move {
                let preview = if file.is_text { &file.content_preview } else { "" };

                let classification = match ollama
                    .classify_file(&file.filename, preview, &file.extension)
                    .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!("classify_file failed for {}: {}", file.filename, e);
                        fallback_classify(&file)
                    }
                };

                results.lock().await.push(ClassifiedFile {
                    original: file,
                    classification,
                });

                let done = processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;

                if let Some(ref s) = state {
                    let mut st = s.write().await;
                    // Classification occupies 10 %–50 % of the overall bar.
                    st.progress = (0.1 + (done as f32 / total as f32) * 0.4).min(0.5);
                    st.progress_message = format!("Classifying… {done}/{total}");
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            if let Err(e) = handle.await {
                tracing::error!("Classification task panicked: {:?}", e);
            }
        }
    }

    let out = Arc::try_unwrap(results)
        .expect("Arc still has multiple owners")
        .into_inner();

    println!(
        "{}",
        format!("  Classified {} files", out.len()).green()
    );

    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Group detection
// ─────────────────────────────────────────────────────────────────────────────

pub async fn detect_groups(
    classified_files: &[ClassifiedFile],
    ollama: &OllamaClient,
) -> Result<Vec<ProjectGroup>> {
    println!("{}", "  Detecting project groups…".cyan());

    let mut description = String::new();
    for cf in classified_files {
        description.push_str(&format!(
            "- {} (type: {}/{}, hint: {})\n",
            cf.original.filename,
            cf.classification.category,
            cf.classification.subcategory,
            if cf.classification.project_hint.is_empty() {
                "none"
            } else {
                &cf.classification.project_hint
            }
        ));
        if description.len() > 3_000 {
            description.push_str("… (truncated)\n");
            break;
        }
    }

    match ollama.detect_project_groups(&description).await {
        Ok(groups) => {
            println!(
                "{}",
                format!("  Found {} project groups", groups.len()).green()
            );
            Ok(groups)
        }
        Err(e) => {
            tracing::warn!("detect_project_groups failed: {}", e);
            Ok(fallback_grouping(classified_files))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fallbacks (no Ollama needed)
// ─────────────────────────────────────────────────────────────────────────────

pub fn fallback_classify(file: &ScannedFile) -> FileClassification {
    let category = match file.extension.as_str() {
        "rs" | "py" | "js" | "ts" | "c" | "cpp" | "java" | "go" | "rb" | "php" | "swift"
        | "kt" | "cs" | "scala" | "zig" | "nim" | "dart" | "lua" | "ex" | "exs" | "hs"
        | "ml" | "clj" | "erl" => "coding_project",

        "html" | "htm" | "css" | "scss" | "vue" | "svelte" | "jsx" | "tsx" => "web",

        "txt" | "md" | "doc" | "docx" | "pdf" | "odt" | "rtf" | "tex" | "rst" => "documents",

        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" | "tiff" => "images",

        "mp3" | "wav" | "flac" | "ogg" | "aac" | "wma" | "m4a" => "music",

        "mp4" | "avi" | "mkv" | "mov" | "wmv" | "flv" | "webm" => "videos",

        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" => "archives",

        "json" | "csv" | "xml" | "yaml" | "yml" | "sql" | "db" | "sqlite" => "data",

        "toml" | "ini" | "cfg" | "conf" | "config" | "env" => "config",

        "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" => "scripts",

        _ => "misc",
    };

    FileClassification {
        category: category.to_string(),
        subcategory: file.extension.clone(),
        suggested_name: file.filename.clone(),
        project_hint: String::new(),
        confidence: 0.5,
    }
}

fn fallback_grouping(classified_files: &[ClassifiedFile]) -> Vec<ProjectGroup> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();

    for cf in classified_files {
        groups
            .entry(cf.classification.category.clone())
            .or_default()
            .push(cf.original.filename.clone());
    }

    groups
        .into_iter()
        .map(|(category, files)| ProjectGroup {
            group_name: category.clone(),
            category_folder: match category.as_str() {
                "coding_project" => "coding_projects",
                "web"            => "web_projects",
                "scripts"        => "scripts",
                "documents"      => "documents",
                "data"           => "data_files",
                "config"         => "config_files",
                _                => "misc",
            }
                .to_string(),
            files,
        })
        .collect()
}