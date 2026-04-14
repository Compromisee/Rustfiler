use crate::classifier::ClassifiedFile;
use crate::ollama::ProjectGroup;
use crate::renamer::RenameAction;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

// ─────────────────────────────────────────────────────────────────────────────
// Phase
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AppPhase {
    #[default]
    Idle,
    Scanning,
    Classifying,
    DetectingGroups,
    AnalyzingRenames,
    Planning,
    Ready,
    Executing,
    Completed,
    Error,
}

// ─────────────────────────────────────────────────────────────────────────────
// Tab
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Overview,
    Files,
    Moves,
    Renames,
    Projects,
    Logs,
}

// ─────────────────────────────────────────────────────────────────────────────
// Log
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
    Success,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub level: LogLevel,
    pub message: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Actions
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MoveAction {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub is_directory: bool,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub struct RenameActionWithSelection {
    pub action: RenameAction,
    pub selected: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Stats
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ScanStats {
    pub total_files: usize,
    pub total_dirs: usize,
    pub game_dirs_skipped: usize,
    pub coding_projects_found: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// AppState
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct AppState {
    // ── settings ─────────────────────────────────────────────────────────────
    pub path: Option<PathBuf>,
    pub model: String,
    pub ollama_url: String,
    pub threads: usize,
    pub max_read_size: usize,
    pub dry_run: bool,
    pub skip_rename: bool,

    // ── runtime status ───────────────────────────────────────────────────────
    pub phase: AppPhase,
    pub progress: f32,
    pub progress_message: String,
    pub error_message: Option<String>,
    pub ollama_connected: Option<bool>,

    // ── results ──────────────────────────────────────────────────────────────
    pub scan_stats: ScanStats,
    pub classified_files: Vec<ClassifiedFile>,
    pub project_groups: Vec<ProjectGroup>,
    pub move_actions: Vec<MoveAction>,
    pub project_moves: Vec<MoveAction>,
    pub rename_actions: Vec<RenameActionWithSelection>,
    pub skipped_game_dirs: Vec<PathBuf>,
    pub existing_project_dirs: Vec<PathBuf>,

    // ── log buffer ───────────────────────────────────────────────────────────
    pub logs: Vec<LogEntry>,

    // ── ui state ─────────────────────────────────────────────────────────────
    pub show_settings: bool,
    pub show_logs: bool,
    pub selected_tab: Tab,
    pub filter_text: String,
}

impl AppState {
    pub fn new(args: &crate::Args) -> Self {
        Self {
            path: args.path.clone(),
            model: args.model.clone(),
            ollama_url: args.url.clone(),
            threads: args.threads,
            max_read_size: args.max_read_size,
            dry_run: args.dry_run,
            skip_rename: args.skip_rename,
            ..Default::default()
        }
    }

    pub fn log(&mut self, level: LogLevel, message: impl Into<String>) {
        if self.logs.len() >= 1_000 {
            self.logs.remove(0);
        }
        self.logs.push(LogEntry {
            timestamp: chrono::Local::now(),
            level,
            message: message.into(),
        });
    }

    pub fn reset_results(&mut self) {
        self.phase = AppPhase::Idle;
        self.progress = 0.0;
        self.progress_message.clear();
        self.error_message = None;
        self.scan_stats = ScanStats::default();
        self.classified_files.clear();
        self.project_groups.clear();
        self.move_actions.clear();
        self.project_moves.clear();
        self.rename_actions.clear();
        self.skipped_game_dirs.clear();
        self.existing_project_dirs.clear();
    }

    pub fn total_selected_actions(&self) -> usize {
        self.move_actions.iter().filter(|m| m.selected).count()
            + self.project_moves.iter().filter(|m| m.selected).count()
            + self.rename_actions.iter().filter(|r| r.selected).count()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared state alias
// ─────────────────────────────────────────────────────────────────────────────

pub type SharedState = Arc<RwLock<AppState>>;

pub fn create_shared_state(args: &crate::Args) -> SharedState {
    Arc::new(RwLock::new(AppState::new(args)))
}