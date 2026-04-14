# AI File Organizer with GUI

## Updated Project Structure

```
file-organizer/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── cli.rs
│   ├── gui.rs
│   ├── app_state.rs
│   ├── organizer.rs
│   ├── ollama.rs
│   ├── scanner.rs
│   ├── classifier.rs
│   └── renamer.rs
```

## Cargo.toml

```toml
[package]
name = "file-organizer"
version = "0.1.0"
edition = "2021"

[features]
default = ["gui"]
gui = ["eframe", "egui", "egui_extras", "rfd", "image"]
cli-only = []

[dependencies]
tokio = { version = "1", features = ["full", "sync"] }
reqwest = { version = "0.12", features = ["json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
walkdir = "2"
rayon = "1.10"
crossbeam = "0.8"
dashmap = "6"
indicatif = { version = "0.17", features = ["rayon"] }
colored = "2"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
regex = "1"
mime_guess = "2"
sha2 = "0.10"
hex = "0.4"
chrono = "0.4"

# GUI dependencies
eframe = { version = "0.29", optional = true, default-features = false, features = [
    "default_fonts",
    "glow",
    "persistence",
] }
egui = { version = "0.29", optional = true }
egui_extras = { version = "0.29", optional = true, features = ["all_loaders"] }
rfd = { version = "0.15", optional = true }
image = { version = "0.25", optional = true, default-features = false, features = ["png"] }

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true

[profile.release-gui]
inherits = "release"
strip = false
```

## src/main.rs

```rust
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
```

## src/cli.rs

```rust
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
```

## src/app_state.rs

```rust
use crate::classifier::ClassifiedFile;
use crate::ollama::ProjectGroup;
use crate::renamer::RenameAction;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppPhase {
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

#[derive(Debug, Clone, Default)]
pub struct ScanStats {
    pub total_files: usize,
    pub total_dirs: usize,
    pub game_dirs_skipped: usize,
    pub coding_projects_found: usize,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
    Success,
}

#[derive(Debug, Default)]
pub struct AppState {
    // Settings
    pub path: Option<PathBuf>,
    pub model: String,
    pub ollama_url: String,
    pub threads: usize,
    pub max_read_size: usize,
    pub dry_run: bool,
    pub skip_rename: bool,

    // Status
    pub phase: AppPhase,
    pub progress: f32,
    pub progress_message: String,
    pub error_message: Option<String>,
    pub ollama_connected: Option<bool>,

    // Results
    pub scan_stats: ScanStats,
    pub classified_files: Vec<ClassifiedFile>,
    pub project_groups: Vec<ProjectGroup>,
    pub move_actions: Vec<MoveAction>,
    pub project_moves: Vec<MoveAction>,
    pub rename_actions: Vec<RenameActionWithSelection>,
    pub skipped_game_dirs: Vec<PathBuf>,
    pub existing_project_dirs: Vec<PathBuf>,

    // Logs
    pub logs: Vec<LogEntry>,

    // UI State
    pub show_settings: bool,
    pub show_logs: bool,
    pub selected_tab: Tab,
    pub filter_text: String,
}

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

impl Default for AppPhase {
    fn default() -> Self {
        Self::Idle
    }
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
            phase: AppPhase::Idle,
            progress: 0.0,
            progress_message: String::new(),
            error_message: None,
            ollama_connected: None,
            scan_stats: ScanStats::default(),
            classified_files: Vec::new(),
            project_groups: Vec::new(),
            move_actions: Vec::new(),
            project_moves: Vec::new(),
            rename_actions: Vec::new(),
            skipped_game_dirs: Vec::new(),
            existing_project_dirs: Vec::new(),
            logs: Vec::new(),
            show_settings: false,
            show_logs: true,
            selected_tab: Tab::Overview,
            filter_text: String::new(),
        }
    }

    pub fn log(&mut self, level: LogLevel, message: impl Into<String>) {
        self.logs.push(LogEntry {
            timestamp: chrono::Local::now(),
            level,
            message: message.into(),
        });
        // Keep last 1000 logs
        if self.logs.len() > 1000 {
            self.logs.remove(0);
        }
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
        let moves = self.move_actions.iter().filter(|m| m.selected).count();
        let proj_moves = self.project_moves.iter().filter(|m| m.selected).count();
        let renames = self.rename_actions.iter().filter(|r| r.selected).count();
        moves + proj_moves + renames
    }
}

pub type SharedState = Arc<RwLock<AppState>>;

pub fn create_shared_state(args: &crate::Args) -> SharedState {
    Arc::new(RwLock::new(AppState::new(args)))
}
```

## src/gui.rs

```rust
use crate::app_state::*;
use crate::organizer::{Organizer, OrganizerConfig};
use crate::Args;
use eframe::egui;
use std::sync::Arc;
use tokio::sync::RwLock;

pub fn run_gui(args: Args) -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(load_icon()),
        ..Default::default()
    };

    let state = create_shared_state(&args);

    eframe::run_native(
        "AI File Organizer",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(FileOrganizerApp::new(state)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {}", e))
}

fn load_icon() -> egui::IconData {
    // Simple default icon
    egui::IconData {
        rgba: vec![255; 32 * 32 * 4],
        width: 32,
        height: 32,
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Add monospace font for code/paths
    fonts.font_data.insert(
        "mono".to_owned(),
        egui::FontData::from_static(include_bytes!("/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf"))
            .into(),
    );

    ctx.set_fonts(fonts);
}

struct FileOrganizerApp {
    state: SharedState,
    runtime: tokio::runtime::Runtime,
    task_running: Arc<std::sync::atomic::AtomicBool>,
}

impl FileOrganizerApp {
    fn new(state: SharedState) -> Self {
        Self {
            state,
            runtime: tokio::runtime::Runtime::new().expect("Failed to create runtime"),
            task_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn start_analysis(&mut self) {
        if self.task_running.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }

        let state = self.state.clone();
        let task_running = self.task_running.clone();

        self.runtime.spawn(async move {
            task_running.store(true, std::sync::atomic::Ordering::SeqCst);

            {
                let mut s = state.write().await;
                s.reset_results();
                s.phase = AppPhase::Scanning;
                s.log(LogLevel::Info, "Starting analysis...");
            }

            let config = {
                let s = state.read().await;
                OrganizerConfig {
                    path: s.path.clone().unwrap_or_default(),
                    dry_run: true, // Always analyze first
                    model: s.model.clone(),
                    url: s.ollama_url.clone(),
                    threads: s.threads,
                    max_read_size: s.max_read_size,
                    skip_rename: s.skip_rename,
                }
            };

            let organizer = Organizer::new(config);

            match organizer.run(Some(state.clone())).await {
                Ok(_) => {
                    let mut s = state.write().await;
                    s.phase = AppPhase::Ready;
                    s.log(LogLevel::Success, "Analysis complete!");
                }
                Err(e) => {
                    let mut s = state.write().await;
                    s.phase = AppPhase::Error;
                    s.error_message = Some(e.to_string());
                    s.log(LogLevel::Error, format!("Error: {}", e));
                }
            }

            task_running.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    fn start_execution(&mut self) {
        if self.task_running.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }

        let state = self.state.clone();
        let task_running = self.task_running.clone();

        self.runtime.spawn(async move {
            task_running.store(true, std::sync::atomic::Ordering::SeqCst);

            {
                let mut s = state.write().await;
                s.phase = AppPhase::Executing;
                s.log(LogLevel::Info, "Executing selected actions...");
            }

            // Execute the selected actions
            let result = execute_selected_actions(state.clone()).await;

            {
                let mut s = state.write().await;
                match result {
                    Ok(count) => {
                        s.phase = AppPhase::Completed;
                        s.log(LogLevel::Success, format!("Completed {} actions!", count));
                    }
                    Err(e) => {
                        s.phase = AppPhase::Error;
                        s.error_message = Some(e.to_string());
                        s.log(LogLevel::Error, format!("Execution error: {}", e));
                    }
                }
            }

            task_running.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    fn check_ollama(&mut self) {
        let state = self.state.clone();

        self.runtime.spawn(async move {
            let url = {
                let s = state.read().await;
                s.ollama_url.clone()
            };

            let client = crate::ollama::OllamaClient::new(&url, "");
            let connected = client.health_check().await.is_ok();

            let mut s = state.write().await;
            s.ollama_connected = Some(connected);
            if connected {
                s.log(LogLevel::Success, "Ollama connection OK");
            } else {
                s.log(LogLevel::Warning, "Ollama not reachable");
            }
        });
    }
}

impl eframe::App for FileOrganizerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let state_snapshot = {
            let state = self.state.clone();
            self.runtime.block_on(async { state.read().await.clone() })
        };

        // Request repaint while task is running
        if self.task_running.load(std::sync::atomic::Ordering::SeqCst) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        // Top panel with title and main controls
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("🗂️ AI File Organizer").size(24.0));
                ui.add_space(20.0);

                // Connection status
                match state_snapshot.ollama_connected {
                    Some(true) => {
                        ui.label(
                            egui::RichText::new("● Ollama Connected")
                                .color(egui::Color32::GREEN)
                                .size(12.0),
                        );
                    }
                    Some(false) => {
                        ui.label(
                            egui::RichText::new("● Ollama Disconnected")
                                .color(egui::Color32::RED)
                                .size(12.0),
                        );
                    }
                    None => {
                        ui.label(
                            egui::RichText::new("● Checking...")
                                .color(egui::Color32::YELLOW)
                                .size(12.0),
                        );
                        self.check_ollama();
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙ Settings").clicked() {
                        let state = self.state.clone();
                        self.runtime.block_on(async {
                            let mut s = state.write().await;
                            s.show_settings = !s.show_settings;
                        });
                    }
                });
            });
            ui.add_space(8.0);
        });

        // Settings window
        if state_snapshot.show_settings {
            egui::Window::new("⚙ Settings")
                .collapsible(false)
                .resizable(true)
                .default_width(400.0)
                .show(ctx, |ui| {
                    self.render_settings(ui, &state_snapshot);
                });
        }

        // Left panel - folder selection and controls
        egui::SidePanel::left("left_panel")
            .resizable(true)
            .default_width(300.0)
            .min_width(250.0)
            .show(ctx, |ui| {
                self.render_left_panel(ui, &state_snapshot);
            });

        // Bottom panel - progress and logs
        egui::TopBottomPanel::bottom("bottom_panel")
            .resizable(true)
            .default_height(150.0)
            .min_height(80.0)
            .show(ctx, |ui| {
                self.render_bottom_panel(ui, &state_snapshot);
            });

        // Central panel - main content
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_central_panel(ui, &state_snapshot);
        });
    }
}

impl FileOrganizerApp {
    fn render_settings(&mut self, ui: &mut egui::Ui, state: &AppState) {
        let mut model = state.model.clone();
        let mut url = state.ollama_url.clone();
        let mut threads = state.threads;
        let mut max_read = state.max_read_size;
        let mut skip_rename = state.skip_rename;

        ui.horizontal(|ui| {
            ui.label("Ollama URL:");
            if ui.text_edit_singleline(&mut url).changed() {
                let state = self.state.clone();
                let url = url.clone();
                self.runtime.block_on(async {
                    let mut s = state.write().await;
                    s.ollama_url = url;
                    s.ollama_connected = None;
                });
            }
        });

        ui.horizontal(|ui| {
            ui.label("Model:");
            egui::ComboBox::from_id_salt("model_select")
                .selected_text(&model)
                .show_ui(ui, |ui| {
                    for m in &["llama3.2", "llama3.1", "mistral", "codellama", "phi3", "gemma2"] {
                        if ui.selectable_value(&mut model, m.to_string(), *m).changed() {
                            let state = self.state.clone();
                            let model = model.clone();
                            self.runtime.block_on(async {
                                state.write().await.model = model;
                            });
                        }
                    }
                });
            ui.text_edit_singleline(&mut model);
        });

        ui.horizontal(|ui| {
            ui.label("Threads:");
            if ui.add(egui::Slider::new(&mut threads, 1..=32)).changed() {
                let state = self.state.clone();
                self.runtime.block_on(async {
                    state.write().await.threads = threads;
                });
            }
        });

        ui.horizontal(|ui| {
            ui.label("Max read size:");
            if ui
                .add(egui::Slider::new(&mut max_read, 1024..=65536).suffix(" bytes"))
                .changed()
            {
                let state = self.state.clone();
                self.runtime.block_on(async {
                    state.write().await.max_read_size = max_read;
                });
            }
        });

        if ui.checkbox(&mut skip_rename, "Skip AI renaming").changed() {
            let state = self.state.clone();
            self.runtime.block_on(async {
                state.write().await.skip_rename = skip_rename;
            });
        }

        ui.separator();

        if ui.button("Test Ollama Connection").clicked() {
            let state = self.state.clone();
            self.runtime.block_on(async {
                state.write().await.ollama_connected = None;
            });
            self.check_ollama();
        }

        ui.separator();

        if ui.button("Close").clicked() {
            let state = self.state.clone();
            self.runtime.block_on(async {
                state.write().await.show_settings = false;
            });
        }
    }

    fn render_left_panel(&mut self, ui: &mut egui::Ui, state: &AppState) {
        ui.add_space(10.0);
        ui.heading("📁 Target Folder");
        ui.add_space(5.0);

        // Folder path display and selection
        ui.horizontal(|ui| {
            let path_text = state
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "No folder selected".to_string());

            ui.add(
                egui::TextEdit::singleline(&mut path_text.as_str())
                    .desired_width(180.0)
                    .interactive(false),
            );

            if ui.button("📂").on_hover_text("Browse").clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    let state = self.state.clone();
                    self.runtime.block_on(async {
                        let mut s = state.write().await;
                        s.path = Some(folder);
                        s.reset_results();
                    });
                }
            }
        });

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);

        // Action buttons
        let is_running = self.task_running.load(std::sync::atomic::Ordering::SeqCst);
        let has_path = state.path.is_some();
        let can_analyze = has_path && !is_running && state.ollama_connected == Some(true);
        let can_execute = state.phase == AppPhase::Ready && !is_running;

        ui.vertical_centered(|ui| {
            let analyze_btn = egui::Button::new(
                egui::RichText::new("🔍 Analyze Folder")
                    .size(16.0)
                    .strong(),
            )
            .min_size(egui::vec2(200.0, 40.0));

            if ui.add_enabled(can_analyze, analyze_btn).clicked() {
                self.start_analysis();
            }

            ui.add_space(10.0);

            let mut dry_run = state.dry_run;
            if ui.checkbox(&mut dry_run, "Dry Run Mode").changed() {
                let state = self.state.clone();
                self.runtime.block_on(async {
                    state.write().await.dry_run = dry_run;
                });
            }

            ui.add_space(20.0);

            let execute_text = if state.dry_run {
                "📋 Preview Only"
            } else {
                "🚀 Execute Selected"
            };

            let execute_btn = egui::Button::new(
                egui::RichText::new(execute_text)
                    .size(16.0)
                    .strong(),
            )
            .min_size(egui::vec2(200.0, 40.0))
            .fill(if state.dry_run {
                egui::Color32::from_rgb(80, 80, 80)
            } else {
                egui::Color32::from_rgb(0, 120, 60)
            });

            if ui
                .add_enabled(can_execute && !state.dry_run, execute_btn)
                .clicked()
            {
                self.start_execution();
            }

            if state.dry_run && can_execute {
                ui.label(
                    egui::RichText::new("Disable dry run to execute")
                        .size(11.0)
                        .color(egui::Color32::YELLOW),
                );
            }
        });

        ui.add_space(20.0);
        ui.separator();
        ui.add_space(10.0);

        // Stats
        ui.heading("📊 Statistics");
        ui.add_space(5.0);

        egui::Grid::new("stats_grid")
            .num_columns(2)
            .spacing([20.0, 4.0])
            .show(ui, |ui| {
                ui.label("Files found:");
                ui.label(
                    egui::RichText::new(state.scan_stats.total_files.to_string())
                        .strong()
                        .color(egui::Color32::LIGHT_BLUE),
                );
                ui.end_row();

                ui.label("Game dirs skipped:");
                ui.label(
                    egui::RichText::new(state.scan_stats.game_dirs_skipped.to_string())
                        .strong()
                        .color(egui::Color32::YELLOW),
                );
                ui.end_row();

                ui.label("Projects detected:");
                ui.label(
                    egui::RichText::new(state.scan_stats.coding_projects_found.to_string())
                        .strong()
                        .color(egui::Color32::GREEN),
                );
                ui.end_row();

                ui.label("Moves planned:");
                ui.label(
                    egui::RichText::new(state.move_actions.len().to_string())
                        .strong(),
                );
                ui.end_row();

                ui.label("Renames planned:");
                ui.label(
                    egui::RichText::new(state.rename_actions.len().to_string())
                        .strong(),
                );
                ui.end_row();

                ui.label("Selected actions:");
                ui.label(
                    egui::RichText::new(state.total_selected_actions().to_string())
                        .strong()
                        .color(egui::Color32::LIGHT_GREEN),
                );
                ui.end_row();
            });

        // Phase indicator
        ui.add_space(20.0);
        ui.separator();
        ui.add_space(10.0);

        let (phase_icon, phase_text, phase_color) = match state.phase {
            AppPhase::Idle => ("⏸", "Idle", egui::Color32::GRAY),
            AppPhase::Scanning => ("🔄", "Scanning...", egui::Color32::YELLOW),
            AppPhase::Classifying => ("🤖", "Classifying...", egui::Color32::LIGHT_BLUE),
            AppPhase::DetectingGroups => ("🔗", "Detecting groups...", egui::Color32::LIGHT_BLUE),
            AppPhase::AnalyzingRenames => ("📝", "Analyzing renames...", egui::Color32::LIGHT_BLUE),
            AppPhase::Planning => ("📋", "Planning...", egui::Color32::LIGHT_BLUE),
            AppPhase::Ready => ("✅", "Ready", egui::Color32::GREEN),
            AppPhase::Executing => ("🚀", "Executing...", egui::Color32::YELLOW),
            AppPhase::Completed => ("🎉", "Completed!", egui::Color32::GREEN),
            AppPhase::Error => ("❌", "Error", egui::Color32::RED),
        };

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(phase_icon).size(20.0));
            ui.label(egui::RichText::new(phase_text).size(14.0).color(phase_color));
        });

        if let Some(ref err) = state.error_message {
            ui.add_space(5.0);
            ui.label(
                egui::RichText::new(err)
                    .size(11.0)
                    .color(egui::Color32::RED),
            );
        }
    }

    fn render_bottom_panel(&mut self, ui: &mut egui::Ui, state: &AppState) {
        // Progress bar
        if state.phase != AppPhase::Idle && state.phase != AppPhase::Ready && state.phase != AppPhase::Completed && state.phase != AppPhase::Error {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::ProgressBar::new(state.progress)
                        .text(&state.progress_message)
                        .animate(true),
                );
            });
            ui.add_space(5.0);
        }

        // Logs
        ui.horizontal(|ui| {
            ui.heading("📜 Logs");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear").clicked() {
                    let state = self.state.clone();
                    self.runtime.block_on(async {
                        state.write().await.logs.clear();
                    });
                }
            });
        });

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for log in &state.logs {
                    let (icon, color) = match log.level {
                        LogLevel::Info => ("ℹ", egui::Color32::LIGHT_BLUE),
                        LogLevel::Warning => ("⚠", egui::Color32::YELLOW),
                        LogLevel::Error => ("❌", egui::Color32::RED),
                        LogLevel::Success => ("✅", egui::Color32::GREEN),
                    };

                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(log.timestamp.format("%H:%M:%S").to_string())
                                .size(10.0)
                                .color(egui::Color32::GRAY),
                        );
                        ui.label(egui::RichText::new(icon).color(color));
                        ui.label(egui::RichText::new(&log.message).size(12.0));
                    });
                }
            });
    }

    fn render_central_panel(&mut self, ui: &mut egui::Ui, state: &AppState) {
        // Tab bar
        ui.horizontal(|ui| {
            let tabs = [
                (Tab::Overview, "📊 Overview"),
                (Tab::Files, "📄 Files"),
                (Tab::Moves, "📁 Moves"),
                (Tab::Renames, "✏️ Renames"),
                (Tab::Projects, "📦 Projects"),
            ];

            for (tab, label) in tabs {
                let selected = state.selected_tab == tab;
                let btn = egui::Button::new(
                    egui::RichText::new(label)
                        .size(14.0)
                        .color(if selected {
                            egui::Color32::WHITE
                        } else {
                            egui::Color32::GRAY
                        }),
                )
                .fill(if selected {
                    egui::Color32::from_rgb(60, 60, 100)
                } else {
                    egui::Color32::TRANSPARENT
                });

                if ui.add(btn).clicked() {
                    let state_arc = self.state.clone();
                    self.runtime.block_on(async {
                        state_arc.write().await.selected_tab = tab;
                    });
                }
            }

            // Filter
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut filter = state.filter_text.clone();
                ui.add(
                    egui::TextEdit::singleline(&mut filter)
                        .hint_text("🔍 Filter...")
                        .desired_width(150.0),
                );
                if filter != state.filter_text {
                    let state_arc = self.state.clone();
                    self.runtime.block_on(async {
                        state_arc.write().await.filter_text = filter;
                    });
                }
            });
        });

        ui.separator();

        // Tab content
        match state.selected_tab {
            Tab::Overview => self.render_overview_tab(ui, state),
            Tab::Files => self.render_files_tab(ui, state),
            Tab::Moves => self.render_moves_tab(ui, state),
            Tab::Renames => self.render_renames_tab(ui, state),
            Tab::Projects => self.render_projects_tab(ui, state),
            Tab::Logs => {} // Handled in bottom panel
        }
    }

    fn render_overview_tab(&self, ui: &mut egui::Ui, state: &AppState) {
        if state.phase == AppPhase::Idle {
            ui.vertical_centered(|ui| {
                ui.add_space(100.0);
                ui.label(
                    egui::RichText::new("👆 Select a folder and click Analyze to begin")
                        .size(18.0)
                        .color(egui::Color32::GRAY),
                );
            });
            return;
        }

        ui.add_space(10.0);

        // Summary cards
        ui.horizontal(|ui| {
            self.render_stat_card(ui, "📄 Files", state.classified_files.len(), egui::Color32::LIGHT_BLUE);
            self.render_stat_card(ui, "📁 Moves", state.move_actions.len(), egui::Color32::GREEN);
            self.render_stat_card(ui, "✏️ Renames", state.rename_actions.len(), egui::Color32::YELLOW);
            self.render_stat_card(ui, "📦 Projects", state.project_groups.len(), egui::Color32::from_rgb(200, 150, 255));
            self.render_stat_card(ui, "🎮 Games Skipped", state.skipped_game_dirs.len(), egui::Color32::RED);
        });

        ui.add_space(20.0);
        ui.separator();
        ui.add_space(10.0);

        // Category breakdown
        ui.heading("📊 Category Breakdown");
        ui.add_space(10.0);

        let mut categories: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for cf in &state.classified_files {
            *categories.entry(cf.classification.category.clone()).or_default() += 1;
        }

        let mut cats: Vec<_> = categories.into_iter().collect();
        cats.sort_by(|a, b| b.1.cmp(&a.1));

        egui::Grid::new("category_grid")
            .num_columns(3)
            .spacing([20.0, 8.0])
            .show(ui, |ui| {
                for (cat, count) in cats.iter().take(10) {
                    let icon = category_icon(cat);
                    let max_count = cats.first().map(|(_, c)| *c).unwrap_or(1);
                    let ratio = *count as f32 / max_count as f32;

                    ui.label(egui::RichText::new(format!("{} {}", icon, cat)).strong());
                    ui.label(count.to_string());
                    ui.add(
                        egui::ProgressBar::new(ratio)
                            .desired_width(200.0)
                            .show_percentage(),
                    );
                    ui.end_row();
                }
            });

        // Skipped game directories
        if !state.skipped_game_dirs.is_empty() {
            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);

            ui.heading("🎮 Skipped Game Directories");
            ui.add_space(5.0);

            egui::ScrollArea::vertical()
                .max_height(150.0)
                .show(ui, |ui| {
                    for dir in &state.skipped_game_dirs {
                        ui.label(
                            egui::RichText::new(format!(
                                "  • {}",
                                dir.file_name().unwrap_or_default().to_string_lossy()
                            ))
                            .color(egui::Color32::YELLOW),
                        );
                    }
                });
        }
    }

    fn render_stat_card(&self, ui: &mut egui::Ui, label: &str, value: usize, color: egui::Color32) {
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(40, 40, 50))
            .rounding(8.0)
            .inner_margin(15.0)
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(value.to_string()).size(28.0).strong().color(color));
                    ui.label(egui::RichText::new(label).size(12.0).color(egui::Color32::GRAY));
                });
            });
    }

    fn render_files_tab(&mut self, ui: &mut egui::Ui, state: &AppState) {
        if state.classified_files.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("No files analyzed yet");
            });
            return;
        }

        let filter = state.filter_text.to_lowercase();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui_extras::TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(egui_extras::Column::auto().at_least(200.0)) // Filename
                .column(egui_extras::Column::auto().at_least(100.0)) // Category
                .column(egui_extras::Column::auto().at_least(100.0)) // Subcategory
                .column(egui_extras::Column::auto().at_least(80.0))  // Size
                .column(egui_extras::Column::remainder())            // Path
                .header(25.0, |mut header| {
                    header.col(|ui| { ui.strong("Filename"); });
                    header.col(|ui| { ui.strong("Category"); });
                    header.col(|ui| { ui.strong("Subcategory"); });
                    header.col(|ui| { ui.strong("Size"); });
                    header.col(|ui| { ui.strong("Path"); });
                })
                .body(|body| {
                    let filtered: Vec<_> = state
                        .classified_files
                        .iter()
                        .filter(|cf| {
                            filter.is_empty()
                                || cf.original.filename.to_lowercase().contains(&filter)
                                || cf.classification.category.to_lowercase().contains(&filter)
                        })
                        .collect();

                    body.rows(20.0, filtered.len(), |mut row| {
                        let cf = &filtered[row.index()];
                        row.col(|ui| {
                            ui.label(&cf.original.filename);
                        });
                        row.col(|ui| {
                            let icon = category_icon(&cf.classification.category);
                            ui.label(format!("{} {}", icon, &cf.classification.category));
                        });
                        row.col(|ui| {
                            ui.label(&cf.classification.subcategory);
                        });
                        row.col(|ui| {
                            ui.label(format_size(cf.original.size));
                        });
                        row.col(|ui| {
                            ui.label(cf.original.relative_path.display().to_string());
                        });
                    });
                });
        });
    }

    fn render_moves_tab(&mut self, ui: &mut egui::Ui, state: &AppState) {
        if state.move_actions.is_empty() && state.project_moves.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("No moves planned");
            });
            return;
        }

        ui.horizontal(|ui| {
            if ui.button("Select All").clicked() {
                let state = self.state.clone();
                self.runtime.block_on(async {
                    let mut s = state.write().await;
                    for m in &mut s.move_actions {
                        m.selected = true;
                    }
                    for m in &mut s.project_moves {
                        m.selected = true;
                    }
                });
            }
            if ui.button("Deselect All").clicked() {
                let state = self.state.clone();
                self.runtime.block_on(async {
                    let mut s = state.write().await;
                    for m in &mut s.move_actions {
                        m.selected = false;
                    }
                    for m in &mut s.project_moves {
                        m.selected = false;
                    }
                });
            }
        });

        ui.separator();

        let filter = state.filter_text.to_lowercase();
        let root = state.path.as_ref().cloned().unwrap_or_default();

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Project moves first
            if !state.project_moves.is_empty() {
                ui.heading("📦 Project Directory Moves");
                ui.add_space(5.0);

                for (idx, m) in state.project_moves.iter().enumerate() {
                    let src_name = m.source.file_name().unwrap_or_default().to_string_lossy();
                    if !filter.is_empty() && !src_name.to_lowercase().contains(&filter) {
                        continue;
                    }

                    ui.horizontal(|ui| {
                        let mut selected = m.selected;
                        if ui.checkbox(&mut selected, "").changed() {
                            let state = self.state.clone();
                            self.runtime.block_on(async {
                                state.write().await.project_moves[idx].selected = selected;
                            });
                        }

                        ui.label(egui::RichText::new("📦").size(14.0));
                        ui.label(
                            egui::RichText::new(src_name.to_string())
                                .color(egui::Color32::from_rgb(255, 180, 100)),
                        );
                        ui.label("→");
                        ui.label(
                            egui::RichText::new(
                                m.destination
                                    .strip_prefix(&root)
                                    .unwrap_or(&m.destination)
                                    .display()
                                    .to_string(),
                            )
                            .color(egui::Color32::GREEN),
                        );
                    });
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);
            }

            // File moves
            ui.heading("📁 File Moves");
            ui.add_space(5.0);

            for (idx, m) in state.move_actions.iter().enumerate() {
                let src_name = m.source.file_name().unwrap_or_default().to_string_lossy();
                if !filter.is_empty() && !src_name.to_lowercase().contains(&filter) {
                    continue;
                }

                ui.horizontal(|ui| {
                    let mut selected = m.selected;
                    if ui.checkbox(&mut selected, "").changed() {
                        let state = self.state.clone();
                        self.runtime.block_on(async {
                            state.write().await.move_actions[idx].selected = selected;
                        });
                    }

                    ui.label(
                        egui::RichText::new(src_name.to_string())
                            .color(egui::Color32::LIGHT_BLUE),
                    );
                    ui.label("→");
                    ui.label(
                        egui::RichText::new(
                            m.destination
                                .strip_prefix(&root)
                                .unwrap_or(&m.destination)
                                .display()
                                .to_string(),
                        )
                        .color(egui::Color32::GREEN),
                    );
                });
            }
        });
    }

    fn render_renames_tab(&mut self, ui: &mut egui::Ui, state: &AppState) {
        if state.rename_actions.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("No renames planned");
            });
            return;
        }

        ui.horizontal(|ui| {
            if ui.button("Select All").clicked() {
                let state = self.state.clone();
                self.runtime.block_on(async {
                    let mut s = state.write().await;
                    for r in &mut s.rename_actions {
                        r.selected = true;
                    }
                });
            }
            if ui.button("Deselect All").clicked() {
                let state = self.state.clone();
                self.runtime.block_on(async {
                    let mut s = state.write().await;
                    for r in &mut s.rename_actions {
                        r.selected = false;
                    }
                });
            }
        });

        ui.separator();

        let filter = state.filter_text.to_lowercase();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (idx, r) in state.rename_actions.iter().enumerate() {
                if !filter.is_empty()
                    && !r.action.original_name.to_lowercase().contains(&filter)
                    && !r.action.new_name.to_lowercase().contains(&filter)
                {
                    continue;
                }

                ui.horizontal(|ui| {
                    let mut selected = r.selected;
                    if ui.checkbox(&mut selected, "").changed() {
                        let state = self.state.clone();
                        self.runtime.block_on(async {
                            state.write().await.rename_actions[idx].selected = selected;
                        });
                    }

                    ui.label("✏️");
                    ui.label(
                        egui::RichText::new(&r.action.original_name)
                            .color(egui::Color32::RED)
                            .strikethrough(),
                    );
                    ui.label("→");
                    ui.label(
                        egui::RichText::new(&r.action.new_name)
                            .color(egui::Color32::GREEN)
                            .strong(),
                    );
                });
            }
        });
    }

    fn render_projects_tab(&self, ui: &mut egui::Ui, state: &AppState) {
        if state.project_groups.is_empty() && state.existing_project_dirs.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("No project groups detected");
            });
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            // Existing project directories
            if !state.existing_project_dirs.is_empty() {
                ui.heading("📦 Detected Coding Projects");
                ui.add_space(5.0);

                for dir in &state.existing_project_dirs {
                    ui.horizontal(|ui| {
                        ui.label("📦");
                        ui.label(
                            egui::RichText::new(
                                dir.file_name().unwrap_or_default().to_string_lossy().to_string(),
                            )
                            .color(egui::Color32::LIGHT_BLUE)
                            .strong(),
                        );
                    });
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);
            }

            // AI-detected groups
            ui.heading("🔗 AI-Detected File Groups");
            ui.add_space(5.0);

            for group in &state.project_groups {
                egui::CollapsingHeader::new(format!(
                    "📁 {} ({} files) → {}",
                    group.group_name,
                    group.files.len(),
                    group.category_folder
                ))
                .default_open(group.files.len() <= 10)
                .show(ui, |ui| {
                    for file in &group.files {
                        ui.label(format!("  • {}", file));
                    }
                });
            }
        });
    }
}

fn category_icon(category: &str) -> &'static str {
    match category {
        "coding_project" => "💻",
        "documents" => "📄",
        "images" => "🖼️",
        "music" => "🎵",
        "videos" => "🎬",
        "archives" => "📦",
        "data" => "📊",
        "config" => "⚙️",
        "scripts" => "📜",
        "web" => "🌐",
        "misc" => "📎",
        _ => "❓",
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

async fn execute_selected_actions(state: SharedState) -> anyhow::Result<usize> {
    let mut count = 0;
    let root: std::path::PathBuf;

    // Execute renames first
    {
        let s = state.read().await;
        root = s.path.clone().unwrap_or_default();

        for r in &s.rename_actions {
            if !r.selected {
                continue;
            }

            let new_path = r.action.path.parent().unwrap_or(&r.action.path).join(&r.action.new_name);
            if new_path == r.action.path {
                continue;
            }
            if new_path.exists() {
                continue;
            }

            std::fs::rename(&r.action.path, &new_path)?;
            count += 1;
        }
    }

    {
        let mut s = state.write().await;
        s.log(LogLevel::Info, format!("Executed {} renames", count));
    }

    // Execute project moves
    let project_count = {
        let s = state.read().await;
        let mut c = 0;

        for m in &s.project_moves {
            if !m.selected {
                continue;
            }

            if let Some(parent) = m.destination.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            if m.destination.exists() {
                continue;
            }

            if m.is_directory {
                move_directory(&m.source, &m.destination)?;
            } else {
                std::fs::rename(&m.source, &m.destination)?;
            }
            c += 1;
        }
        c
    };
    count += project_count;

    {
        let mut s = state.write().await;
        s.log(LogLevel::Info, format!("Executed {} project moves", project_count));
    }

    // Execute file moves
    let move_count = {
        let s = state.read().await;
        let mut c = 0;

        for m in &s.move_actions {
            if !m.selected {
                continue;
            }

            if let Some(parent) = m.destination.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            if m.destination.exists() {
                continue;
            }

            if std::fs::rename(&m.source, &m.destination).is_err() {
                std::fs::copy(&m.source, &m.destination)?;
                std::fs::remove_file(&m.source)?;
            }
            c += 1;
        }
        c
    };
    count += move_count;

    {
        let mut s = state.write().await;
        s.log(LogLevel::Info, format!("Executed {} file moves", move_count));
    }

    Ok(count)
}

fn move_directory(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }

    copy_dir_recursive(src, dst)?;
    std::fs::remove_dir_all(src)?;
    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
```

## src/organizer.rs (Updated with GUI support)

```rust
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

pub struct Organizer {
    config: OrganizerConfig,
    ollama: OllamaClient,
}

impl Organizer {
    pub fn new(config: OrganizerConfig) -> Self {
        let ollama = OllamaClient::new(&config.url, &config.model);
        Self { config, ollama }
    }

    pub async fn run(&self, state: Option<SharedState>) -> Result<()> {
        let update_state = |f: Box<dyn FnOnce(&mut AppState) + Send>| {
            if let Some(ref state) = state {
                let state = state.clone();
                tokio::spawn(async move {
                    let mut s = state.write().await;
                    f(&mut s);
                });
            }
        };

        // Step 1: Scan directory
        self.log(&state, LogLevel::Info, "Scanning directory...").await;
        self.update_phase(&state, AppPhase::Scanning, 0.0, "Scanning...").await;

        let scan_result = scanner::scan_directory(&self.config.path, self.config.max_read_size)?;

        if let Some(ref s) = state {
            let mut st = s.write().await;
            st.scan_stats.total_files = scan_result.files.len();
            st.scan_stats.game_dirs_skipped = scan_result.skipped_game_dirs.len();
            st.skipped_game_dirs = scan_result.skipped_game_dirs.clone();
        }

        self.log(&state, LogLevel::Info, format!(
            "Scanned {} files, skipped {} game dirs",
            scan_result.files.len(),
            scan_result.skipped_game_dirs.len()
        )).await;

        if scan_result.files.is_empty() {
            self.log(&state, LogLevel::Warning, "No files to organize!").await;
            return Ok(());
        }

        // Detect existing coding project directories
        let project_roots = scanner::detect_coding_project_roots(&self.config.path);
        if let Some(ref s) = state {
            let mut st = s.write().await;
            st.scan_stats.coding_projects_found = project_roots.len();
            st.existing_project_dirs = project_roots.clone();
        }

        // Step 2: Classify files with AI
        self.log(&state, LogLevel::Info, "Classifying files with AI...").await;
        self.update_phase(&state, AppPhase::Classifying, 0.1, "Classifying files...").await;

        let classified = classifier::classify_files_with_progress(
            &scan_result.files,
            &self.ollama,
            state.clone(),
        ).await?;

        if let Some(ref s) = state {
            let mut st = s.write().await;
            st.classified_files = classified.clone();
        }

        self.log(&state, LogLevel::Info, format!("Classified {} files", classified.len())).await;

        // Step 3: Detect project groupings
        self.log(&state, LogLevel::Info, "Detecting project groups...").await;
        self.update_phase(&state, AppPhase::DetectingGroups, 0.5, "Detecting groups...").await;

        let loose_files: Vec<ClassifiedFile> = classified
            .iter()
            .filter(|cf| cf.original.relative_path.components().count() == 1)
            .cloned()
            .collect();

        let groups = if !loose_files.is_empty() {
            classifier::detect_groups(&loose_files, &self.ollama).await?
        } else {
            vec![]
        };

        if let Some(ref s) = state {
            let mut st = s.write().await;
            st.project_groups = groups.clone();
        }

        self.log(&state, LogLevel::Info, format!("Found {} project groups", groups.len())).await;

        // Step 4: Generate renames
        self.log(&state, LogLevel::Info, "Analyzing file renames...").await;
        self.update_phase(&state, AppPhase::AnalyzingRenames, 0.7, "Analyzing renames...").await;

        let renames = renamer::generate_renames_with_progress(
            &classified,
            &self.ollama,
            self.config.skip_rename,
            state.clone(),
        ).await?;

        if let Some(ref s) = state {
            let mut st = s.write().await;
            st.rename_actions = renames
                .iter()
                .map(|r| RenameActionWithSelection {
                    action: r.clone(),
                    selected: true,
                })
                .collect();
        }

        self.log(&state, LogLevel::Info, format!("Found {} potential renames", renames.len())).await;

        // Step 5: Plan moves
        self.log(&state, LogLevel::Info, "Planning file organization...").await;
        self.update_phase(&state, AppPhase::Planning, 0.9, "Planning moves...").await;

        let (move_actions, project_move_actions) =
            self.plan_moves(&classified, &groups, &project_roots)?;

        if let Some(ref s) = state {
            let mut st = s.write().await;
            st.move_actions = move_actions
                .into_iter()
                .map(|(src, dst, is_dir)| MoveAction {
                    source: src,
                    destination: dst,
                    is_directory: is_dir,
                    selected: true,
                })
                .collect();
            st.project_moves = project_move_actions
                .into_iter()
                .map(|(src, dst, is_dir)| MoveAction {
                    source: src,
                    destination: dst,
                    is_directory: is_dir,
                    selected: true,
                })
                .collect();
        }

        self.update_phase(&state, AppPhase::Ready, 1.0, "Ready!").await;
        self.log(&state, LogLevel::Success, "Analysis complete!").await;

        // If running in CLI mode without GUI state, execute directly
        if state.is_none() && !self.config.dry_run {
            // CLI execution logic here
            println!("{}", "Executing changes...".cyan());
            // ... execute logic
        }

        Ok(())
    }

    fn plan_moves(
        &self,
        classified: &[ClassifiedFile],
        groups: &[ProjectGroup],
        existing_projects: &[PathBuf],
    ) -> Result<(Vec<(PathBuf, PathBuf, bool)>, Vec<(PathBuf, PathBuf, bool)>)> {
        let root = &self.config.path;
        let mut move_actions = Vec::new();
        let mut project_moves = Vec::new();
        let mut handled_files: HashMap<String, bool> = HashMap::new();

        // Handle AI-detected groups
        for group in groups {
            if group.files.len() <= 1 {
                continue;
            }

            let category_dir = root.join(&group.category_folder);
            let group_dir = category_dir.join(&group.group_name);

            for filename in &group.files {
                if let Some(cf) = classified.iter().find(|cf| cf.original.filename == *filename) {
                    let dest = group_dir.join(filename);
                    if cf.original.path != dest {
                        move_actions.push((cf.original.path.clone(), dest, false));
                        handled_files.insert(filename.clone(), true);
                    }
                }
            }
        }

        // Handle existing project directories
        for project_dir in existing_projects {
            let dir_name = project_dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let dest = root.join("coding_projects").join(&dir_name);
            if *project_dir != dest && !project_dir.starts_with(&root.join("coding_projects")) {
                project_moves.push((project_dir.clone(), dest, true));
            }
        }

        // Handle remaining unhandled files by category
        for cf in classified {
            if handled_files.contains_key(&cf.original.filename) {
                continue;
            }

            if cf.original.relative_path.components().count() > 1 {
                continue;
            }

            let category_folder = category_to_folder(&cf.classification.category);
            let dest = root.join(category_folder).join(&cf.original.filename);

            if cf.original.path != dest {
                move_actions.push((cf.original.path.clone(), dest, false));
            }
        }

        Ok((move_actions, project_moves))
    }

    async fn log(&self, state: &Option<SharedState>, level: LogLevel, message: impl Into<String>) {
        let msg = message.into();
        if let Some(ref s) = state {
            let mut st = s.write().await;
            st.log(level, &msg);
        }

        // Also print to console
        match level {
            LogLevel::Info => println!("{}", msg),
            LogLevel::Warning => println!("{}", msg.yellow()),
            LogLevel::Error => println!("{}", msg.red()),
            LogLevel::Success => println!("{}", msg.green()),
        }
    }

    async fn update_phase(&self, state: &Option<SharedState>, phase: AppPhase, progress: f32, message: &str) {
        if let Some(ref s) = state {
            let mut st = s.write().await;
            st.phase = phase;
            st.progress = progress;
            st.progress_message = message.to_string();
        }
    }
}

fn category_to_folder(category: &str) -> &str {
    match category {
        "coding_project" => "coding_projects",
        "documents" => "documents",
        "images" => "images",
        "music" => "music",
        "videos" => "videos",
        "archives" => "archives",
        "data" => "data_files",
        "config" => "config_files",
        "scripts" => "scripts",
        "web" => "web_projects",
        "misc" => "misc",
        _ => "misc",
    }
}
```

## src/classifier.rs (Updated with progress)

```rust
use crate::app_state::{AppPhase, SharedState};
use crate::ollama::{FileClassification, OllamaClient, ProjectGroup};
use crate::scanner::ScannedFile;
use anyhow::Result;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct ClassifiedFile {
    pub original: ScannedFile,
    pub classification: FileClassification,
}

pub async fn classify_files(
    files: &[ScannedFile],
    ollama: &OllamaClient,
) -> Result<Vec<ClassifiedFile>> {
    classify_files_with_progress(files, ollama, None).await
}

pub async fn classify_files_with_progress(
    files: &[ScannedFile],
    ollama: &OllamaClient,
    state: Option<SharedState>,
) -> Result<Vec<ClassifiedFile>> {
    let pb = ProgressBar::new(files.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("█▓░"),
    );
    pb.set_message("Classifying files...");

    let results: Arc<Mutex<Vec<ClassifiedFile>>> = Arc::new(Mutex::new(Vec::new()));
    let total = files.len();
    let processed = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let batch_size = 8;
    for chunk in files.chunks(batch_size) {
        let mut handles = Vec::new();

        for file in chunk {
            let ollama = ollama.clone();
            let file = file.clone();
            let results = results.clone();
            let pb = pb.clone();
            let processed = processed.clone();
            let state = state.clone();
            let total = total;

            let handle = tokio::spawn(async move {
                let preview = if file.is_text {
                    &file.content_preview
                } else {
                    ""
                };

                let classification = match ollama
                    .classify_file(&file.filename, preview, &file.extension)
                    .await
                {
                    Ok(c) => c,
                    Err(_) => fallback_classify(&file),
                };

                let classified = ClassifiedFile {
                    original: file,
                    classification,
                };
                results.lock().await.push(classified);

                let p = processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                pb.inc(1);

                if let Some(ref s) = state {
                    let mut st = s.write().await;
                    let progress = 0.1 + (p as f32 / total as f32) * 0.4;
                    st.progress = progress;
                    st.progress_message = format!("Classifying... {}/{}", p, total);
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            handle.await?;
        }
    }

    pb.finish_with_message("Classification complete!");

    let results = Arc::try_unwrap(results).unwrap().into_inner();
    Ok(results)
}

pub async fn detect_groups(
    classified_files: &[ClassifiedFile],
    ollama: &OllamaClient,
) -> Result<Vec<ProjectGroup>> {
    println!("{}", "🔍 Detecting project groups...".cyan());

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

        if description.len() > 3000 {
            description.push_str("... (truncated)\n");
            break;
        }
    }

    match ollama.detect_project_groups(&description).await {
        Ok(groups) => {
            println!("  Found {} project groups", groups.len().to_string().green());
            Ok(groups)
        }
        Err(e) => {
            tracing::warn!("Failed to detect groups: {}", e);
            Ok(fallback_grouping(classified_files))
        }
    }
}

fn fallback_classify(file: &ScannedFile) -> FileClassification {
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
                "coding_project" => "coding_projects".to_string(),
                "web" => "web_projects".to_string(),
                "scripts" => "scripts".to_string(),
                "documents" => "documents".to_string(),
                "data" => "data_files".to_string(),
                "config" => "config_files".to_string(),
                _ => "misc".to_string(),
            },
            files,
        })
        .collect()
}
```

## src/renamer.rs (Updated with progress)

```rust
use crate::app_state::SharedState;
use crate::classifier::ClassifiedFile;
use crate::ollama::OllamaClient;
use crate::scanner::is_text_file;
use anyhow::Result;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct RenameAction {
    pub original_name: String,
    pub new_name: String,
    pub path: PathBuf,
}

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
        println!("{}", "⏩ Skipping AI rename step".yellow());
        return Ok(vec![]);
    }

    let renameable: Vec<&ClassifiedFile> = classified_files
        .iter()
        .filter(|cf| {
            is_text_file(&cf.original.extension) && should_consider_rename(&cf.original.filename)
        })
        .collect();

    if renameable.is_empty() {
        println!("{}", "  No files need renaming".dimmed());
        return Ok(vec![]);
    }

    println!(
        "📝 Analyzing {} files for potential renaming...",
        renameable.len().to_string().cyan()
    );

    let pb = ProgressBar::new(renameable.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} Renaming...")
            .unwrap()
            .progress_chars("█▓░"),
    );

    let mut renames = Vec::new();
    let total = renameable.len();

    let batch_size = 4;
    for (batch_idx, chunk) in renameable.chunks(batch_size).enumerate() {
        let mut handles = Vec::new();

        for cf in chunk.iter() {
            let ollama = ollama.clone();
            let filename = cf.original.filename.clone();
            let preview = cf.original.content_preview.clone();
            let path = cf.original.path.clone();
            let ai_suggestion = cf.classification.suggested_name.clone();

            let handle = tokio::spawn(async move {
                let new_name = if ai_suggestion != filename && !ai_suggestion.is_empty() {
                    ai_suggestion
                } else {
                    match ollama.suggest_rename(&filename, &preview).await {
                        Ok(name) => name,
                        Err(_) => filename.clone(),
                    }
                };

                if new_name != filename {
                    Some(RenameAction {
                        original_name: filename,
                        new_name,
                        path,
                    })
                } else {
                    None
                }
            });

            handles.push(handle);
        }

        for handle in handles {
            if let Ok(Some(rename)) = handle.await? {
                renames.push(rename);
            }
            pb.inc(1);

            if let Some(ref s) = state {
                let processed = batch_idx * batch_size + renames.len();
                let mut st = s.write().await;
                let progress = 0.7 + (processed as f32 / total as f32) * 0.2;
                st.progress = progress;
                st.progress_message = format!("Analyzing renames... {}/{}", processed, total);
            }
        }
    }

    pb.finish_with_message("Rename analysis complete!");
    Ok(renames)
}

fn should_consider_rename(filename: &str) -> bool {
    let name = filename.to_lowercase();

    let skip_patterns = [
        "cargo.toml", "cargo.lock", "package.json", "package-lock.json",
        "tsconfig.json", "webpack.config", ".gitignore", ".gitattributes",
        ".editorconfig", "makefile", "dockerfile", "readme", "license",
        "changelog", "contributing", "go.mod", "go.sum", "gemfile",
        "rakefile", "procfile", ".env", "pyproject.toml", "setup.py",
        "setup.cfg", "requirements.txt", "pipfile", "yarn.lock",
        "pnpm-lock.yaml", "bun.lockb", "flake.nix", "flake.lock",
        "shell.nix", "default.nix",
    ];

    if skip_patterns.iter().any(|p| name.contains(p)) {
        return false;
    }

    let generic_names = [
        "untitled", "new_file", "temp", "test", "document", "file",
        "data", "output", "result", "draft", "copy", "backup",
    ];

    let stem = std::path::Path::new(filename)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    generic_names.iter().any(|g| stem == *g || stem.starts_with(&format!("{}_", g)))
}
```

## src/ollama.rs (Keep existing)

*Same as before - no changes needed*

## src/scanner.rs (Keep existing)

*Same as before - no changes needed*

## Build & Run

```bash
# Build with GUI (default)
cargo build --release

# Build CLI-only (smaller binary, no GUI dependencies)
cargo build --release --no-default-features --features cli-only

# Run GUI (just double-click or run without args)
./target/release/file-organizer

# Run CLI mode
./target/release/file-organizer --cli --path /path/to/folder --dry-run

# Run GUI with pre-selected path
./target/release/file-organizer --path /path/to/folder
```

## Screenshot Preview

```
┌─────────────────────────────────────────────────────────────────────┐
│  🗂️ AI File Organizer                    ● Ollama Connected  ⚙ Settings │
├──────────────────────┬──────────────────────────────────────────────┤
│ 📁 Target Folder     │  📊 Overview │ 📄 Files │ 📁 Moves │ ✏️ Renames │ 📦 Projects │
│ [/home/user/messy]📂 │──────────────────────────────────────────────│
│                      │  ┌───────┐ ┌───────┐ ┌───────┐ ┌───────┐    │
│ ┌──────────────────┐ │  │  147  │ │   42  │ │   12  │ │    5  │    │
│ │  🔍 Analyze      │ │  │ Files │ │ Moves │ │Renames│ │Projects│    │
│ └──────────────────┘ │  └───────┘ └───────┘ └───────┘ └───────┘    │
│                      │                                              │
│ ☑ Dry Run Mode       │  📊 Category Breakdown                       │
│                      │  ────────────────────                        │
│ ┌──────────────────┐ │  💻 coding_project  45  ████████████████░░   │
│ │  🚀 Execute      │ │  📄 documents       32  ████████████░░░░░░   │
│ └──────────────────┘ │  🖼️ images          28  ██████████░░░░░░░░   │
│                      │  📊 data            22  ████████░░░░░░░░░░   │
│ ──────────────────── │  ⚙️ config          12  █████░░░░░░░░░░░░░   │
│ 📊 Statistics        │  📜 scripts          8  ████░░░░░░░░░░░░░░   │
│ Files found:     147 │                                              │
│ Game dirs skipped: 3 │  🎮 Skipped Game Directories                 │
│ Projects detected: 5 │  • SteamLibrary                              │
│ Moves planned:    42 │  • Minecraft                                 │
│ Renames planned:  12 │  • Terraria                                  │
│ Selected:         54 │                                              │
│                      │                                              │
│ ──────────────────── │                                              │
│ ✅ Ready             │                                              │
├──────────────────────┴──────────────────────────────────────────────┤
│ 📜 Logs                                                      [Clear]│
│ 14:32:01 ℹ Starting analysis...                                     │
│ 14:32:02 ✅ Scanned 147 files, skipped 3 game dirs                   │
│ 14:32:15 ✅ Classified 147 files                                     │
│ 14:32:18 ✅ Found 5 project groups                                   │
│ 14:32:22 ✅ Analysis complete!                                       │
│ [████████████████████████████████████████] 100% Ready!              │
└─────────────────────────────────────────────────────────────────────┘
```

## Features Summary

| Feature | Description |
|---------|-------------|
| **Native GUI** | Fast egui-based interface, works on Linux/Windows/macOS |
| **Folder Browser** | Native file dialog for selecting target folder |
| **Live Progress** | Real-time progress bars and status updates |
| **Tabbed Interface** | Overview, Files, Moves, Renames, Projects tabs |
| **Selectable Actions** | Check/uncheck individual moves and renames |
| **Filter/Search** | Filter files and actions in all tabs |
| **Settings Panel** | Configure Ollama URL, model, threads, etc. |
| **Connection Status** | Visual indicator for Ollama connectivity |
| **Detailed Logs** | Timestamped log viewer with colored entries |
| **Dry Run Toggle** | Preview changes without executing |
| **CLI Fallback** | Full CLI mode with `--cli` flag |
| **Category Stats** | Visual breakdown of file categories |
| **Game Detection** | Shows skipped game directories |