use crate::app_state::*;
use crate::classifier::{self, ClassifiedFile};
use crate::ollama::{OllamaClient, ProjectGroup};
use crate::renamer::{self, RenameAction};
use crate::scanner;
use anyhow::Result;
use colored::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

// ─────────────────────────────────────────────────────────────────────────────
// Config
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OrganizerConfig {
    pub path: PathBuf,
    pub dry_run: bool,
    pub model: String,
    pub url: String,
    pub threads: usize,
    pub max_read_size: usize,
    pub skip_rename: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Organizer
// ─────────────────────────────────────────────────────────────────────────────

pub struct Organizer {
    config: OrganizerConfig,
    ollama: OllamaClient,
}

impl Organizer {
    pub fn new(config: OrganizerConfig) -> Self {
        let ollama = OllamaClient::new(&config.url, &config.model);
        Self { config, ollama }
    }

    // ── main entry-point ─────────────────────────────────────────────────────

    pub async fn run(&self, state: Option<SharedState>) -> Result<()> {
        // ── Step 1: scan ─────────────────────────────────────────────────────
        self.update(&state, AppPhase::Scanning, 0.0, "Scanning directory…").await;
        self.log_info(&state, "Scanning directory…").await;

        let scan_result =
            scanner::scan_directory(&self.config.path, self.config.max_read_size)?;

        if let Some(ref s) = state {
            let mut st = s.write().await;
            st.scan_stats.total_files = scan_result.files.len();
            st.scan_stats.game_dirs_skipped = scan_result.skipped_game_dirs.len();
            st.skipped_game_dirs = scan_result.skipped_game_dirs.clone();
        }

        self.log_info(
            &state,
            format!(
                "Found {} files, skipped {} game dirs",
                scan_result.files.len(),
                scan_result.skipped_game_dirs.len()
            ),
        )
            .await;

        if scan_result.files.is_empty() {
            self.log_warn(&state, "No files found to organise.").await;
            return Ok(());
        }

        // Detect existing coding-project directories
        let project_roots = scanner::detect_coding_project_roots(&self.config.path);
        if let Some(ref s) = state {
            let mut st = s.write().await;
            st.scan_stats.coding_projects_found = project_roots.len();
            st.existing_project_dirs = project_roots.clone();
        }

        // ── Step 2: classify ──────────────────────────────────────────────────
        self.update(&state, AppPhase::Classifying, 0.1, "Classifying files…").await;
        self.log_info(&state, "Classifying files with AI…").await;

        let classified = classifier::classify_files_with_progress(
            &scan_result.files,
            &self.ollama,
            state.clone(),
        )
            .await?;

        if let Some(ref s) = state {
            s.write().await.classified_files = classified.clone();
        }

        self.log_info(&state, format!("Classified {} files", classified.len()))
            .await;

        // ── Step 3: detect groups ────────────────────────────────────────────
        self.update(&state, AppPhase::DetectingGroups, 0.5, "Detecting groups…").await;
        self.log_info(&state, "Detecting project groups…").await;

        let loose: Vec<ClassifiedFile> = classified
            .iter()
            .filter(|cf| cf.original.relative_path.components().count() == 1)
            .cloned()
            .collect();

        let groups = if loose.is_empty() {
            vec![]
        } else {
            classifier::detect_groups(&loose, &self.ollama).await?
        };

        if let Some(ref s) = state {
            s.write().await.project_groups = groups.clone();
        }

        self.log_info(&state, format!("Found {} project groups", groups.len()))
            .await;

        // ── Step 4: renames ───────────────────────────────────────────────────
        self.update(&state, AppPhase::AnalyzingRenames, 0.7, "Analysing renames…")
            .await;
        self.log_info(&state, "Analysing renames…").await;

        let renames = renamer::generate_renames_with_progress(
            &classified,
            &self.ollama,
            self.config.skip_rename,
            state.clone(),
        )
            .await?;

        if let Some(ref s) = state {
            s.write().await.rename_actions = renames
                .iter()
                .map(|r| RenameActionWithSelection {
                    action: r.clone(),
                    selected: true,
                })
                .collect();
        }

        self.log_info(&state, format!("Found {} potential renames", renames.len()))
            .await;

        // ── Step 5: plan moves ────────────────────────────────────────────────
        self.update(&state, AppPhase::Planning, 0.9, "Planning moves…").await;
        self.log_info(&state, "Planning file organisation…").await;

        let (file_moves, proj_moves) =
            self.plan_moves(&classified, &groups, &project_roots)?;

        if let Some(ref s) = state {
            let mut st = s.write().await;
            st.move_actions = file_moves
                .iter()
                .map(|(src, dst, is_dir)| MoveAction {
                    source: src.clone(),
                    destination: dst.clone(),
                    is_directory: *is_dir,
                    selected: true,
                })
                .collect();
            st.project_moves = proj_moves
                .iter()
                .map(|(src, dst, is_dir)| MoveAction {
                    source: src.clone(),
                    destination: dst.clone(),
                    is_directory: *is_dir,
                    selected: true,
                })
                .collect();
        }

        self.log_info(
            &state,
            format!(
                "Planned {} file moves and {} project moves",
                file_moves.len(),
                proj_moves.len()
            ),
        )
            .await;

        self.update(&state, AppPhase::Ready, 1.0, "Ready!").await;
        self.log_ok(&state, "Analysis complete — review the plan and click Execute.")
            .await;

        // ── Step 6: execute (CLI-only path) ───────────────────────────────────
        // In GUI mode `state` is Some and execution is triggered separately.
        // In CLI mode `state` is None and we execute here when not a dry-run.
        if state.is_none() && !self.config.dry_run {
            self.execute_cli(&renames, &proj_moves, &file_moves)?;
        }

        Ok(())
    }

    // ── planning ─────────────────────────────────────────────────────────────

    fn plan_moves(
        &self,
        classified: &[ClassifiedFile],
        groups: &[ProjectGroup],
        existing_projects: &[PathBuf],
    ) -> Result<(Vec<(PathBuf, PathBuf, bool)>, Vec<(PathBuf, PathBuf, bool)>)> {
        let root = &self.config.path;
        let mut file_moves: Vec<(PathBuf, PathBuf, bool)> = Vec::new();
        let mut proj_moves: Vec<(PathBuf, PathBuf, bool)> = Vec::new();
        let mut handled: HashMap<String, bool> = HashMap::new();

        // AI-detected groups with more than one file get their own sub-folder
        for group in groups {
            if group.files.len() <= 1 {
                continue;
            }
            let group_dir = root.join(&group.category_folder).join(&group.group_name);
            for filename in &group.files {
                if let Some(cf) = classified.iter().find(|cf| &cf.original.filename == filename) {
                    let dst = group_dir.join(filename);
                    if cf.original.path != dst {
                        file_moves.push((cf.original.path.clone(), dst, false));
                        handled.insert(filename.clone(), true);
                    }
                }
            }
        }

        // Existing coding-project directories → coding_projects/<name>
        for project_dir in existing_projects {
            let name = project_dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let dst = root.join("coding_projects").join(&name);
            if *project_dir != dst
                && !project_dir.starts_with(&root.join("coding_projects"))
            {
                proj_moves.push((project_dir.clone(), dst, true));
            }
        }

        // Everything else: sort by category into a flat folder
        for cf in classified {
            if handled.contains_key(&cf.original.filename) {
                continue;
            }
            if cf.original.relative_path.components().count() > 1 {
                continue; // already in a sub-folder, leave it
            }
            let folder = category_to_folder(&cf.classification.category);
            let dst = root.join(folder).join(&cf.original.filename);
            if cf.original.path != dst {
                file_moves.push((cf.original.path.clone(), dst, false));
            }
        }

        Ok((file_moves, proj_moves))
    }

    // ── CLI execution ────────────────────────────────────────────────────────

    fn execute_cli(
        &self,
        renames: &[RenameAction],
        proj_moves: &[(PathBuf, PathBuf, bool)],
        file_moves: &[(PathBuf, PathBuf, bool)],
    ) -> Result<()> {
        println!("{}", "Executing renames…".cyan());
        for r in renames {
            let new_path = r.path.parent().unwrap_or(&r.path).join(&r.new_name);
            if new_path == r.path || new_path.exists() {
                continue;
            }
            println!("  ✏  {} → {}", r.original_name.dimmed(), r.new_name.green());
            fs::rename(&r.path, &new_path)?;
        }

        println!("{}", "Executing project moves…".cyan());
        for (src, dst, is_dir) in proj_moves {
            ensure_parent(dst)?;
            if dst.exists() {
                continue;
            }
            println!("  📦 {} → {}", src.display().to_string().dimmed(), dst.display().to_string().green());
            if *is_dir {
                move_directory(src, dst)?;
            } else {
                fs::rename(src, dst)?;
            }
        }

        println!("{}", "Executing file moves…".cyan());
        for (src, dst, _) in file_moves {
            ensure_parent(dst)?;
            if dst.exists() {
                continue;
            }
            println!("  📄 {} → {}", src.display().to_string().dimmed(), dst.display().to_string().green());
            if fs::rename(src, dst).is_err() {
                fs::copy(src, dst)?;
                fs::remove_file(src)?;
            }
        }

        println!("{}", "Done!".green().bold());
        Ok(())
    }

    // ── state helpers ────────────────────────────────────────────────────────

    async fn update(
        &self,
        state: &Option<SharedState>,
        phase: AppPhase,
        progress: f32,
        message: &str,
    ) {
        if let Some(ref s) = state {
            let mut st = s.write().await;
            st.phase = phase;
            st.progress = progress;
            st.progress_message = message.to_string();
        }
    }

    async fn log_info(&self, state: &Option<SharedState>, msg: impl Into<String>) {
        let msg = msg.into();
        println!("{}", msg);
        if let Some(ref s) = state {
            s.write().await.log(LogLevel::Info, msg);
        }
    }

    async fn log_warn(&self, state: &Option<SharedState>, msg: impl Into<String>) {
        let msg = msg.into();
        println!("{}", msg.yellow());
        if let Some(ref s) = state {
            s.write().await.log(LogLevel::Warning, msg);
        }
    }

    async fn log_ok(&self, state: &Option<SharedState>, msg: impl Into<String>) {
        let msg = msg.into();
        println!("{}", msg.green());
        if let Some(ref s) = state {
            s.write().await.log(LogLevel::Success, msg);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Free helpers
// ─────────────────────────────────────────────────────────────────────────────

fn category_to_folder(category: &str) -> &str {
    match category {
        "coding_project" => "coding_projects",
        "documents"      => "documents",
        "images"         => "images",
        "music"          => "music",
        "videos"         => "videos",
        "archives"       => "archives",
        "data"           => "data_files",
        "config"         => "config_files",
        "scripts"        => "scripts",
        "web"            => "web_projects",
        _                => "misc",
    }
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(p) = path.parent() {
        if !p.exists() {
            fs::create_dir_all(p)?;
        }
    }
    Ok(())
}

fn move_directory(src: &Path, dst: &Path) -> Result<()> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_dir_recursive(src, dst)?;
    fs::remove_dir_all(src)?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_dir() {
            copy_dir_recursive(&s, &d)?;
        } else {
            fs::copy(&s, &d)?;
        }
    }
    Ok(())
}