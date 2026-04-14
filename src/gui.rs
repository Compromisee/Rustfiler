use crate::app_state::*;
use crate::organizer::{Organizer, OrganizerConfig};
use crate::Args;
use eframe::egui;
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────────────────
// Public entry-point
// ─────────────────────────────────────────────────────────────────────────────

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

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn load_icon() -> egui::IconData {
    egui::IconData {
        rgba: vec![100, 150, 255, 255]
            .into_iter()
            .cycle()
            .take(32 * 32 * 4)
            .collect(),
        width: 32,
        height: 32,
    }
}

/// Set up fonts without relying on hard-coded OS paths.
/// We fall back to egui's built-in fonts so the binary works everywhere.
fn setup_fonts(ctx: &egui::Context) {
    // Start from the defaults (includes a proportional + monospace font)
    let fonts = egui::FontDefinitions::default();
    // You can push custom bytes here if you embed a font with include_bytes!
    // e.g. fonts.font_data.insert("my_font".to_owned(),
    //     Arc::new(egui::FontData::from_static(include_bytes!("../assets/font.ttf"))));
    ctx.set_fonts(fonts);
}

fn category_icon(category: &str) -> &'static str {
    match category {
        "coding_project" => "💻",
        "documents" => "📄",
        "images" => "🖼",
        "music" => "🎵",
        "videos" => "🎬",
        "archives" => "📦",
        "data" => "📊",
        "config" => "⚙",
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

// ─────────────────────────────────────────────────────────────────────────────
// Message channel – avoids block_on inside the render loop
// ─────────────────────────────────────────────────────────────────────────────

/// Commands sent from the UI thread → background runtime.
enum UiCmd {
    StartAnalysis,
    StartExecution,
    CheckOllama,
    SetPath(std::path::PathBuf),
    SetDryRun(bool),
    SetSkipRename(bool),
    SetModel(String),
    SetUrl(String),
    SetThreads(usize),
    SetMaxRead(usize),
    ToggleSettings,
    SetTab(Tab),
    SetFilter(String),
    SelectAllMoves(bool),
    SelectAllRenames(bool),
    SelectMove { idx: usize, project: bool, val: bool },
    SelectRename { idx: usize, val: bool },
    ClearLogs,
    CloseSettings,
}

// ─────────────────────────────────────────────────────────────────────────────
// App struct
// ─────────────────────────────────────────────────────────────────────────────

struct FileOrganizerApp {
    /// Snapshot updated once per frame – read-only in render methods.
    snapshot: AppState,
    /// The real shared state mutated by background tasks.
    state: SharedState,
    /// Tokio runtime kept alive for the lifetime of the app.
    runtime: tokio::runtime::Runtime,
    /// Set to true while a background task is in flight.
    task_running: Arc<std::sync::atomic::AtomicBool>,
    /// UI → background command channel.
    cmd_tx: std::sync::mpsc::Sender<UiCmd>,
    cmd_rx: std::sync::mpsc::Receiver<UiCmd>,
}

impl FileOrganizerApp {
    fn new(state: SharedState) -> Self {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();
        // Build a default snapshot so we never have an uninitialized state.
        let snapshot = {
            // Block briefly at startup – this is fine; we are not in a frame yet.
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async { state.read().await.clone() })
        };
        Self {
            snapshot,
            state,
            runtime: tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime"),
            task_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cmd_tx,
            cmd_rx,
        }
    }

    // ── command helpers ──────────────────────────────────────────────────────

    fn send(&self, cmd: UiCmd) {
        // Silently ignore send errors (app is shutting down)
        let _ = self.cmd_tx.send(cmd);
    }

    // ── background task launchers ────────────────────────────────────────────

    fn start_analysis(&self) {
        if self.task_running.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let state = self.state.clone();
        let running = self.task_running.clone();

        self.runtime.spawn(async move {
            running.store(true, std::sync::atomic::Ordering::SeqCst);

            {
                let mut s = state.write().await;
                s.reset_results();
                s.phase = AppPhase::Scanning;
                s.log(LogLevel::Info, "Starting analysis…");
            }

            let config = {
                let s = state.read().await;
                OrganizerConfig {
                    path: s.path.clone().unwrap_or_default(),
                    dry_run: true,
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
                    s.log(LogLevel::Error, format!("Error: {e}"));
                }
            }

            running.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    fn start_execution(&self) {
        if self.task_running.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let state = self.state.clone();
        let running = self.task_running.clone();

        self.runtime.spawn(async move {
            running.store(true, std::sync::atomic::Ordering::SeqCst);
            {
                let mut s = state.write().await;
                s.phase = AppPhase::Executing;
                s.log(LogLevel::Info, "Executing selected actions…");
            }

            match execute_selected_actions(state.clone()).await {
                Ok(n) => {
                    let mut s = state.write().await;
                    s.phase = AppPhase::Completed;
                    s.log(LogLevel::Success, format!("Completed {n} actions!"));
                }
                Err(e) => {
                    let mut s = state.write().await;
                    s.phase = AppPhase::Error;
                    s.error_message = Some(e.to_string());
                    s.log(LogLevel::Error, format!("Execution error: {e}"));
                }
            }

            running.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    fn check_ollama(&self) {
        let state = self.state.clone();
        self.runtime.spawn(async move {
            let url = state.read().await.ollama_url.clone();
            let client = crate::ollama::OllamaClient::new(&url, "");
            let connected = client.health_check().await.is_ok();
            let mut s = state.write().await;
            s.ollama_connected = Some(connected);
            if connected {
                s.log(LogLevel::Success, "Ollama connection OK");
            } else {
                s.log(LogLevel::Warning, "Ollama not reachable – is `ollama serve` running?");
            }
        });
    }

    // ── command processor (runs once per frame, before rendering) ───────────

    fn process_commands(&mut self) {
        // Drain all pending commands without blocking.
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                UiCmd::StartAnalysis => self.start_analysis(),
                UiCmd::StartExecution => self.start_execution(),
                UiCmd::CheckOllama => self.check_ollama(),

                // All state mutations go through the runtime but we use
                // `spawn` (fire-and-forget), so they never block the UI thread.
                UiCmd::SetPath(p) => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move {
                        let mut s = state.write().await;
                        s.path = Some(p);
                        s.reset_results();
                    });
                }
                UiCmd::SetDryRun(v) => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move { state.write().await.dry_run = v; });
                }
                UiCmd::SetSkipRename(v) => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move { state.write().await.skip_rename = v; });
                }
                UiCmd::SetModel(v) => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move { state.write().await.model = v; });
                }
                UiCmd::SetUrl(v) => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move {
                        let mut s = state.write().await;
                        s.ollama_url = v;
                        s.ollama_connected = None;
                    });
                }
                UiCmd::SetThreads(v) => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move { state.write().await.threads = v; });
                }
                UiCmd::SetMaxRead(v) => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move { state.write().await.max_read_size = v; });
                }
                UiCmd::ToggleSettings => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move {
                        let mut s = state.write().await;
                        s.show_settings = !s.show_settings;
                    });
                }
                UiCmd::CloseSettings => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move { state.write().await.show_settings = false; });
                }
                UiCmd::SetTab(t) => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move { state.write().await.selected_tab = t; });
                }
                UiCmd::SetFilter(f) => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move { state.write().await.filter_text = f; });
                }
                UiCmd::SelectAllMoves(v) => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move {
                        let mut s = state.write().await;
                        s.move_actions.iter_mut().for_each(|m| m.selected = v);
                        s.project_moves.iter_mut().for_each(|m| m.selected = v);
                    });
                }
                UiCmd::SelectAllRenames(v) => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move {
                        state.write().await.rename_actions.iter_mut().for_each(|r| r.selected = v);
                    });
                }
                UiCmd::SelectMove { idx, project, val } => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move {
                        let mut s = state.write().await;
                        if project {
                            if let Some(m) = s.project_moves.get_mut(idx) { m.selected = val; }
                        } else if let Some(m) = s.move_actions.get_mut(idx) {
                            m.selected = val;
                        }
                    });
                }
                UiCmd::SelectRename { idx, val } => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move {
                        if let Some(r) = state.write().await.rename_actions.get_mut(idx) {
                            r.selected = val;
                        }
                    });
                }
                UiCmd::ClearLogs => {
                    let state = self.state.clone();
                    self.runtime.spawn(async move { state.write().await.logs.clear(); });
                }
            }
        }
    }

    /// Pull a fresh snapshot from shared state. Uses `try_read` so we never
    /// block the UI thread; if the lock is held we just keep the old snapshot.
    fn refresh_snapshot(&mut self) {
        if let Ok(guard) = self.state.try_read() {
            self.snapshot = guard.clone();
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// eframe::App implementation
// ─────────────────────────────────────────────────────────────────────────────

impl eframe::App for FileOrganizerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Process UI commands from the previous frame.
        self.process_commands();

        // 2. Refresh our local snapshot (non-blocking).
        self.refresh_snapshot();

        // 3. Request a repaint while a task is running so progress bars animate.
        if self.task_running.load(std::sync::atomic::Ordering::SeqCst) {
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
        }

        // ── Top panel ────────────────────────────────────────────────────────
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("🗂 AI File Organizer").size(22.0));
                ui.add_space(16.0);

                match self.snapshot.ollama_connected {
                    Some(true) => {
                        ui.label(
                            egui::RichText::new("● Connected")
                                .color(egui::Color32::GREEN)
                                .size(12.0),
                        );
                    }
                    Some(false) => {
                        ui.label(
                            egui::RichText::new("● Disconnected")
                                .color(egui::Color32::RED)
                                .size(12.0),
                        );
                    }
                    None => {
                        ui.label(
                            egui::RichText::new("● Checking…")
                                .color(egui::Color32::YELLOW)
                                .size(12.0),
                        );
                        self.send(UiCmd::CheckOllama);
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙ Settings").clicked() {
                        self.send(UiCmd::ToggleSettings);
                    }
                });
            });
            ui.add_space(8.0);
        });

        // ── Settings window ──────────────────────────────────────────────────
        if self.snapshot.show_settings {
            // Collect into locals so the closure is 'static-friendly.
            let mut model   = self.snapshot.model.clone();
            let mut url     = self.snapshot.ollama_url.clone();
            let mut threads = self.snapshot.threads;
            let mut max_read= self.snapshot.max_read_size;
            let mut skip    = self.snapshot.skip_rename;
            let tx = self.cmd_tx.clone();

            egui::Window::new("⚙ Settings")
                .collapsible(false)
                .resizable(true)
                .default_width(420.0)
                .show(ctx, |ui| {
                    egui::Grid::new("settings_grid")
                        .num_columns(2)
                        .spacing([12.0, 8.0])
                        .show(ui, |ui| {
                            ui.label("Ollama URL:");
                            if ui.text_edit_singleline(&mut url).lost_focus() {
                                let _ = tx.send(UiCmd::SetUrl(url.clone()));
                            }
                            ui.end_row();

                            ui.label("Model:");
                            egui::ComboBox::from_id_salt("model_combo")
                                .selected_text(&model)
                                .show_ui(ui, |ui| {
                                    for m in ["llama3.2", "llama3.1", "mistral",
                                        "codellama", "phi3", "gemma2"]
                                    {
                                        if ui.selectable_value(&mut model, m.to_string(), m)
                                            .clicked()
                                        {
                                            let _ = tx.send(UiCmd::SetModel(model.clone()));
                                        }
                                    }
                                });
                            ui.end_row();

                            ui.label("Threads:");
                            if ui.add(egui::Slider::new(&mut threads, 1..=32)).changed() {
                                let _ = tx.send(UiCmd::SetThreads(threads));
                            }
                            ui.end_row();

                            ui.label("Max read:");
                            if ui.add(
                                egui::Slider::new(&mut max_read, 1024..=65536).suffix(" B"),
                            ).changed() {
                                let _ = tx.send(UiCmd::SetMaxRead(max_read));
                            }
                            ui.end_row();

                            ui.label("Skip renames:");
                            if ui.checkbox(&mut skip, "").changed() {
                                let _ = tx.send(UiCmd::SetSkipRename(skip));
                            }
                            ui.end_row();
                        });

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Test connection").clicked() {
                            let _ = tx.send(UiCmd::CheckOllama);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close").clicked() {
                                let _ = tx.send(UiCmd::CloseSettings);
                            }
                        });
                    });
                });
        }

        // ── Left panel ───────────────────────────────────────────────────────
        egui::SidePanel::left("left_panel")
            .resizable(true)
            .default_width(280.0)
            .min_width(220.0)
            .show(ctx, |ui| {
                self.render_left_panel(ui);
            });

        // ── Bottom panel ─────────────────────────────────────────────────────
        egui::TopBottomPanel::bottom("bottom_panel")
            .resizable(true)
            .default_height(160.0)
            .min_height(80.0)
            .show(ctx, |ui| {
                self.render_bottom_panel(ui);
            });

        // ── Central panel ────────────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_central_panel(ui);
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Panel renderers  (all read from self.snapshot, send cmds via self.cmd_tx)
// ─────────────────────────────────────────────────────────────────────────────

impl FileOrganizerApp {
    fn render_left_panel(&self, ui: &mut egui::Ui) {
        let state = &self.snapshot;
        let tx    = &self.cmd_tx;

        ui.add_space(10.0);
        ui.heading("📁 Target Folder");
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let path_str = state
                .path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "No folder selected".into());

            // Read-only path display – allocate a String so TextEdit can take &mut str
            let mut display = path_str.clone();
            ui.add(
                egui::TextEdit::singleline(&mut display)
                    .desired_width(180.0)
                    .interactive(false),
            );

            if ui.button("📂").on_hover_text("Browse for folder").clicked() {
                if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                    let _ = tx.send(UiCmd::SetPath(folder));
                }
            }
        });

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);

        let is_running  = self.task_running.load(std::sync::atomic::Ordering::SeqCst);
        let has_path    = state.path.is_some();
        let ollama_ok   = state.ollama_connected == Some(true);
        let can_analyze = has_path && !is_running && ollama_ok;
        let can_execute = state.phase == AppPhase::Ready && !is_running && !state.dry_run;

        ui.vertical_centered(|ui| {
            let analyze_btn = egui::Button::new(
                egui::RichText::new("🔍 Analyze Folder").size(15.0).strong(),
            )
                .min_size(egui::vec2(200.0, 38.0));

            if ui.add_enabled(can_analyze, analyze_btn).clicked() {
                let _ = tx.send(UiCmd::StartAnalysis);
            }

            ui.add_space(8.0);

            let mut dry = state.dry_run;
            if ui.checkbox(&mut dry, "Dry Run Mode").changed() {
                let _ = tx.send(UiCmd::SetDryRun(dry));
            }

            ui.add_space(16.0);

            let exec_label = if state.dry_run { "📋 Preview Only" } else { "🚀 Execute Selected" };
            let exec_color = if state.dry_run {
                egui::Color32::from_rgb(70, 70, 70)
            } else {
                egui::Color32::from_rgb(0, 130, 70)
            };

            let exec_btn = egui::Button::new(
                egui::RichText::new(exec_label).size(15.0).strong(),
            )
                .min_size(egui::vec2(200.0, 38.0))
                .fill(exec_color);

            if ui.add_enabled(can_execute, exec_btn).clicked() {
                let _ = tx.send(UiCmd::StartExecution);
            }

            if state.dry_run && state.phase == AppPhase::Ready {
                ui.label(
                    egui::RichText::new("Uncheck Dry Run to execute")
                        .size(10.0)
                        .color(egui::Color32::YELLOW),
                );
            }
        });

        ui.add_space(18.0);
        ui.separator();
        ui.add_space(10.0);
        ui.heading("📊 Statistics");
        ui.add_space(6.0);

        egui::Grid::new("stats_grid")
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                let rows: &[(&str, usize, egui::Color32)] = &[
                    ("Files found",       state.scan_stats.total_files,            egui::Color32::LIGHT_BLUE),
                    ("Game dirs skipped", state.scan_stats.game_dirs_skipped,       egui::Color32::YELLOW),
                    ("Projects found",    state.scan_stats.coding_projects_found,   egui::Color32::GREEN),
                    ("Moves planned",     state.move_actions.len(),                 egui::Color32::WHITE),
                    ("Renames planned",   state.rename_actions.len(),               egui::Color32::WHITE),
                    ("Selected actions",  state.total_selected_actions(),           egui::Color32::LIGHT_GREEN),
                ];
                for (label, value, color) in rows {
                    ui.label(*label);
                    ui.label(egui::RichText::new(value.to_string()).strong().color(*color));
                    ui.end_row();
                }
            });

        ui.add_space(18.0);
        ui.separator();
        ui.add_space(8.0);

        let (icon, text, color) = phase_display(&state.phase);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(icon).size(18.0));
            ui.label(egui::RichText::new(text).size(13.0).color(color));
        });

        if let Some(ref err) = state.error_message {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(err).size(10.0).color(egui::Color32::RED));
        }
    }

    fn render_bottom_panel(&self, ui: &mut egui::Ui) {
        let state = &self.snapshot;
        let tx    = &self.cmd_tx;

        // Progress bar (only while a task is in flight)
        let show_progress = !matches!(
            state.phase,
            AppPhase::Idle | AppPhase::Ready | AppPhase::Completed | AppPhase::Error
        );
        if show_progress {
            ui.add_space(4.0);
            ui.add(
                egui::ProgressBar::new(state.progress)
                    .text(state.progress_message.clone())   // String → WidgetText via Into
                    .animate(true),
            );
            ui.add_space(4.0);
        }

        ui.horizontal(|ui| {
            ui.heading("📜 Logs");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Clear").clicked() {
                    let _ = tx.send(UiCmd::ClearLogs);
                }
            });
        });

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for entry in &state.logs {
                    let (icon, color) = log_display(entry.level);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(entry.timestamp.format("%H:%M:%S").to_string())
                                .size(10.0)
                                .color(egui::Color32::GRAY),
                        );
                        ui.label(egui::RichText::new(icon).color(color));
                        ui.label(egui::RichText::new(&entry.message).size(12.0));
                    });
                }
            });
    }

    fn render_central_panel(&self, ui: &mut egui::Ui) {
        let state = &self.snapshot;
        let tx    = &self.cmd_tx;

        // ── Tab bar ──────────────────────────────────────────────────────────
        ui.horizontal(|ui| {
            const TABS: &[(Tab, &str)] = &[
                (Tab::Overview, "📊 Overview"),
                (Tab::Files,    "📄 Files"),
                (Tab::Moves,    "📁 Moves"),
                (Tab::Renames,  "✏ Renames"),
                (Tab::Projects, "📦 Projects"),
            ];

            for &(tab, label) in TABS {
                let selected = state.selected_tab == tab;
                let btn = egui::Button::new(
                    egui::RichText::new(label)
                        .size(13.0)
                        .color(if selected { egui::Color32::WHITE } else { egui::Color32::GRAY }),
                )
                    .fill(if selected {
                        egui::Color32::from_rgb(55, 55, 95)
                    } else {
                        egui::Color32::TRANSPARENT
                    });

                if ui.add(btn).clicked() {
                    let _ = tx.send(UiCmd::SetTab(tab));
                }
            }

            // Filter box (right-aligned)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut filter = state.filter_text.clone();
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut filter)
                        .hint_text("🔍 Filter…")
                        .desired_width(140.0),
                );
                if resp.changed() {
                    let _ = tx.send(UiCmd::SetFilter(filter));
                }
            });
        });

        ui.separator();

        // ── Tab content ──────────────────────────────────────────────────────
        match state.selected_tab {
            Tab::Overview  => self.render_overview_tab(ui),
            Tab::Files     => self.render_files_tab(ui),
            Tab::Moves     => self.render_moves_tab(ui),
            Tab::Renames   => self.render_renames_tab(ui),
            Tab::Projects  => self.render_projects_tab(ui),
            Tab::Logs      => {} // shown in bottom panel
        }
    }

    // ── Tab: Overview ────────────────────────────────────────────────────────

    fn render_overview_tab(&self, ui: &mut egui::Ui) {
        let state = &self.snapshot;

        if state.phase == AppPhase::Idle {
            ui.vertical_centered(|ui| {
                ui.add_space(100.0);
                ui.label(
                    egui::RichText::new("👆 Select a folder and click Analyze to begin")
                        .size(17.0)
                        .color(egui::Color32::GRAY),
                );
            });
            return;
        }

        ui.add_space(10.0);

        // Stat cards row
        ui.horizontal(|ui| {
            self.stat_card(ui, "📄 Files",        state.classified_files.len(),       egui::Color32::LIGHT_BLUE);
            self.stat_card(ui, "📁 Moves",        state.move_actions.len(),            egui::Color32::GREEN);
            self.stat_card(ui, "✏ Renames",      state.rename_actions.len(),          egui::Color32::YELLOW);
            self.stat_card(ui, "📦 Projects",     state.project_groups.len(),          egui::Color32::from_rgb(200, 150, 255));
            self.stat_card(ui, "🎮 Games Skipped",state.skipped_game_dirs.len(),       egui::Color32::RED);
        });

        ui.add_space(16.0);
        ui.separator();
        ui.add_space(10.0);
        ui.heading("Category Breakdown");
        ui.add_space(8.0);

        // Build category counts from the snapshot
        let mut cats: std::collections::HashMap<String, usize> = Default::default();
        for cf in &state.classified_files {
            *cats.entry(cf.classification.category.clone()).or_default() += 1;
        }
        let mut cats: Vec<_> = cats.into_iter().collect();
        cats.sort_by(|a, b| b.1.cmp(&a.1));
        let max = cats.first().map(|(_, c)| *c).unwrap_or(1) as f32;

        egui::Grid::new("cat_grid")
            .num_columns(3)
            .spacing([16.0, 6.0])
            .show(ui, |ui| {
                for (cat, count) in cats.iter().take(12) {
                    let icon = category_icon(cat);
                    ui.label(egui::RichText::new(format!("{icon} {cat}")).strong());
                    ui.label(count.to_string());
                    ui.add(
                        egui::ProgressBar::new(*count as f32 / max)
                            .desired_width(180.0)
                            .show_percentage(),
                    );
                    ui.end_row();
                }
            });

        if !state.skipped_game_dirs.is_empty() {
            ui.add_space(16.0);
            ui.separator();
            ui.add_space(8.0);
            ui.heading("🎮 Skipped Game Directories");
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
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

    fn stat_card(&self, ui: &mut egui::Ui, label: &str, value: usize, color: egui::Color32) {
        // egui 0.29: Frame::new() replaces Frame::none()
        // Most compatible across egui 0.28/0.29:
        egui::Frame::default()
            .fill(egui::Color32::from_rgb(38, 38, 50))
            .rounding(egui::Rounding::same(8.0))
            .inner_margin(egui::Margin::same(14.0))
            .show(ui, |inner_ui| {
                inner_ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new(value.to_string())
                            .size(26.0)
                            .strong()
                            .color(color),
                    );
                    ui.label(egui::RichText::new(label).size(11.0).color(egui::Color32::GRAY));
                });
            });
    }

    // ── Tab: Files ───────────────────────────────────────────────────────────

    fn render_files_tab(&self, ui: &mut egui::Ui) {
        let state = &self.snapshot;

        if state.classified_files.is_empty() {
            ui.centered_and_justified(|ui| { ui.label("No files analysed yet"); });
            return;
        }

        let filter = state.filter_text.to_lowercase();
        let filtered: Vec<_> = state
            .classified_files
            .iter()
            .filter(|cf| {
                filter.is_empty()
                    || cf.original.filename.to_lowercase().contains(&filter)
                    || cf.classification.category.to_lowercase().contains(&filter)
            })
            .collect();

        // TableBuilder manages its OWN scroll area – do NOT wrap it in ScrollArea.
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::initial(200.0).at_least(120.0)) // Filename
            .column(egui_extras::Column::initial(110.0).at_least(80.0))  // Category
            .column(egui_extras::Column::initial(100.0).at_least(60.0))  // Subcat
            .column(egui_extras::Column::initial(80.0).at_least(60.0))   // Size
            .column(egui_extras::Column::remainder())                     // Path
            .header(24.0, |mut h| {
                h.col(|ui| { ui.strong("Filename"); });
                h.col(|ui| { ui.strong("Category"); });
                h.col(|ui| { ui.strong("Subcategory"); });
                h.col(|ui| { ui.strong("Size"); });
                h.col(|ui| { ui.strong("Path"); });
            })
            .body(|body| {
                body.rows(20.0, filtered.len(), |mut row| {
                    let cf = filtered[row.index()];
                    row.col(|ui| { ui.label(&cf.original.filename); });
                    row.col(|ui| {
                        ui.label(format!("{} {}", category_icon(&cf.classification.category),
                                         &cf.classification.category));
                    });
                    row.col(|ui| { ui.label(&cf.classification.subcategory); });
                    row.col(|ui| { ui.label(format_size(cf.original.size)); });
                    row.col(|ui| {
                        ui.label(cf.original.relative_path.display().to_string());
                    });
                });
            });
    }

    // ── Tab: Moves ───────────────────────────────────────────────────────────

    fn render_moves_tab(&self, ui: &mut egui::Ui) {
        let state = &self.snapshot;
        let tx    = &self.cmd_tx;

        if state.move_actions.is_empty() && state.project_moves.is_empty() {
            ui.centered_and_justified(|ui| { ui.label("No moves planned"); });
            return;
        }

        ui.horizontal(|ui| {
            if ui.button("Select All").clicked()   { let _ = tx.send(UiCmd::SelectAllMoves(true));  }
            if ui.button("Deselect All").clicked() { let _ = tx.send(UiCmd::SelectAllMoves(false)); }
        });
        ui.separator();

        let filter = state.filter_text.to_lowercase();
        let root   = state.path.as_deref().unwrap_or(std::path::Path::new(""));

        egui::ScrollArea::vertical().show(ui, |ui| {
            if !state.project_moves.is_empty() {
                ui.heading("📦 Project Directory Moves");
                ui.add_space(4.0);

                for (idx, m) in state.project_moves.iter().enumerate() {
                    let name = m.source.file_name().unwrap_or_default().to_string_lossy();
                    if !filter.is_empty() && !name.to_lowercase().contains(&filter) { continue; }

                    let mut sel = m.selected;
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut sel, "").changed() {
                            let _ = tx.send(UiCmd::SelectMove { idx, project: true, val: sel });
                        }
                        ui.label(egui::RichText::new("📦").size(13.0));
                        ui.label(egui::RichText::new(name.as_ref()).color(egui::Color32::from_rgb(255,180,100)));
                        ui.label("→");
                        ui.label(
                            egui::RichText::new(
                                m.destination.strip_prefix(root).unwrap_or(&m.destination).display().to_string()
                            ).color(egui::Color32::GREEN),
                        );
                    });
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);
            }

            ui.heading("📁 File Moves");
            ui.add_space(4.0);

            for (idx, m) in state.move_actions.iter().enumerate() {
                let name = m.source.file_name().unwrap_or_default().to_string_lossy();
                if !filter.is_empty() && !name.to_lowercase().contains(&filter) { continue; }

                let mut sel = m.selected;
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut sel, "").changed() {
                        let _ = tx.send(UiCmd::SelectMove { idx, project: false, val: sel });
                    }
                    ui.label(egui::RichText::new(name.as_ref()).color(egui::Color32::LIGHT_BLUE));
                    ui.label("→");
                    ui.label(
                        egui::RichText::new(
                            m.destination.strip_prefix(root).unwrap_or(&m.destination).display().to_string()
                        ).color(egui::Color32::GREEN),
                    );
                });
            }
        });
    }

    // ── Tab: Renames ─────────────────────────────────────────────────────────

    fn render_renames_tab(&self, ui: &mut egui::Ui) {
        let state = &self.snapshot;
        let tx    = &self.cmd_tx;

        if state.rename_actions.is_empty() {
            ui.centered_and_justified(|ui| { ui.label("No renames planned"); });
            return;
        }

        ui.horizontal(|ui| {
            if ui.button("Select All").clicked()   { let _ = tx.send(UiCmd::SelectAllRenames(true));  }
            if ui.button("Deselect All").clicked() { let _ = tx.send(UiCmd::SelectAllRenames(false)); }
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

                let mut sel = r.selected;
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut sel, "").changed() {
                        let _ = tx.send(UiCmd::SelectRename { idx, val: sel });
                    }
                    ui.label("✏");
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

    // ── Tab: Projects ────────────────────────────────────────────────────────

    fn render_projects_tab(&self, ui: &mut egui::Ui) {
        let state = &self.snapshot;

        if state.project_groups.is_empty() && state.existing_project_dirs.is_empty() {
            ui.centered_and_justified(|ui| { ui.label("No project groups detected"); });
            return;
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            if !state.existing_project_dirs.is_empty() {
                ui.heading("📦 Detected Coding Projects");
                ui.add_space(4.0);
                for dir in &state.existing_project_dirs {
                    ui.horizontal(|ui| {
                        ui.label("📦");
                        ui.label(
                            egui::RichText::new(
                                dir.file_name().unwrap_or_default().to_string_lossy().as_ref(),
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

            ui.heading("🔗 AI-Detected File Groups");
            ui.add_space(4.0);

            for group in &state.project_groups {
                egui::CollapsingHeader::new(format!(
                    "📁 {} ({} files) → {}",
                    group.group_name, group.files.len(), group.category_folder
                ))
                    .default_open(group.files.len() <= 10)
                    .show(ui, |ui| {
                        for f in &group.files {
                            ui.label(format!("  • {f}"));
                        }
                    });
            }
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Small display helpers
// ─────────────────────────────────────────────────────────────────────────────

fn phase_display(phase: &AppPhase) -> (&'static str, &'static str, egui::Color32) {
    match phase {
        AppPhase::Idle           => ("⏸",  "Idle",               egui::Color32::GRAY),
        AppPhase::Scanning       => ("🔄", "Scanning…",          egui::Color32::YELLOW),
        AppPhase::Classifying    => ("🤖", "Classifying…",       egui::Color32::LIGHT_BLUE),
        AppPhase::DetectingGroups=> ("🔗", "Detecting groups…",  egui::Color32::LIGHT_BLUE),
        AppPhase::AnalyzingRenames=>("📝", "Analyzing renames…", egui::Color32::LIGHT_BLUE),
        AppPhase::Planning       => ("📋", "Planning…",          egui::Color32::LIGHT_BLUE),
        AppPhase::Ready          => ("✅", "Ready",              egui::Color32::GREEN),
        AppPhase::Executing      => ("🚀", "Executing…",         egui::Color32::YELLOW),
        AppPhase::Completed      => ("🎉", "Completed!",         egui::Color32::GREEN),
        AppPhase::Error          => ("❌", "Error",              egui::Color32::RED),
    }
}

fn log_display(level: LogLevel) -> (&'static str, egui::Color32) {
    match level {
        LogLevel::Info    => ("ℹ",  egui::Color32::LIGHT_BLUE),
        LogLevel::Warning => ("⚠",  egui::Color32::YELLOW),
        LogLevel::Error   => ("❌", egui::Color32::RED),
        LogLevel::Success => ("✅", egui::Color32::GREEN),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Execution logic (async, runs on the background runtime)
// ─────────────────────────────────────────────────────────────────────────────

async fn execute_selected_actions(state: SharedState) -> anyhow::Result<usize> {
    let mut total = 0usize;

    // ── renames ──────────────────────────────────────────────────────────────
    let rename_actions: Vec<_> = {
        let s = state.read().await;
        s.rename_actions
            .iter()
            .filter(|r| r.selected)
            .map(|r| (r.action.path.clone(), r.action.new_name.clone()))
            .collect()
    };

    let mut rename_count = 0usize;
    for (path, new_name) in rename_actions {
        let new_path = match path.parent() {
            Some(p) => p.join(&new_name),
            None    => continue,
        };
        if new_path == path || new_path.exists() { continue; }
        std::fs::rename(&path, &new_path)?;
        rename_count += 1;
    }
    total += rename_count;
    state.write().await.log(LogLevel::Info, format!("Renamed {rename_count} files"));

    // ── project moves ────────────────────────────────────────────────────────
    let proj_moves: Vec<_> = {
        let s = state.read().await;
        s.project_moves
            .iter()
            .filter(|m| m.selected)
            .map(|m| (m.source.clone(), m.destination.clone(), m.is_directory))
            .collect()
    };

    let mut proj_count = 0usize;
    for (src, dst, is_dir) in proj_moves {
        if let Some(p) = dst.parent() { if !p.exists() { std::fs::create_dir_all(p)?; } }
        if dst.exists() { continue; }
        if is_dir { move_dir(&src, &dst)?; } else { std::fs::rename(&src, &dst)?; }
        proj_count += 1;
    }
    total += proj_count;
    state.write().await.log(LogLevel::Info, format!("Moved {proj_count} project directories"));

    // ── file moves ───────────────────────────────────────────────────────────
    let file_moves: Vec<_> = {
        let s = state.read().await;
        s.move_actions
            .iter()
            .filter(|m| m.selected)
            .map(|m| (m.source.clone(), m.destination.clone()))
            .collect()
    };

    let mut file_count = 0usize;
    for (src, dst) in file_moves {
        if let Some(p) = dst.parent() { if !p.exists() { std::fs::create_dir_all(p)?; } }
        if dst.exists() { continue; }
        if std::fs::rename(&src, &dst).is_err() {
            std::fs::copy(&src, &dst)?;
            std::fs::remove_file(&src)?;
        }
        file_count += 1;
    }
    total += file_count;
    state.write().await.log(LogLevel::Info, format!("Moved {file_count} files"));

    Ok(total)
}

fn move_dir(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    if std::fs::rename(src, dst).is_ok() { return Ok(()); }
    copy_dir(src, dst)?;
    std::fs::remove_dir_all(src)?;
    Ok(())
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dst.join(entry.file_name());
        if s.is_dir() { copy_dir(&s, &d)?; } else { std::fs::copy(&s, &d)?; }
    }
    Ok(())
}