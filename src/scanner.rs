use anyhow::Result;
use dashmap::DashMap;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

// ─────────────────────────────────────────────────────────────────────────────
// Game-detection constants
// ─────────────────────────────────────────────────────────────────────────────

const GAME_INDICATORS: &[&str] = &[
    "unitycrashhandler",
    "unitycrashandler64",
    "ue4prereqsetup",
    "unrealceceditor",
    "unitysubsystems",
    "mono",
    "il2cpp",
    "engine",
    "binaries",
    "content",
    "pakchunks",
    "gamedata",
    "game_data",
    "savedata",
    "save_data",
    "saves",
    "steamapps",
    "steam_api.dll",
    "steam_api64.dll",
    "steam_appid.txt",
    "installscript.vdf",
    "unrealengine",
    "godot",
    "gamemaker",
    "level0",
    "level1",
    "sharedassets",
    "resources.assets",
    "globalgamemanagers",
    "data.unity3d",
    "boot.config",
];

const GAME_EXTENSIONS: &[&str] = &[
    "pak", "pck", "wad", "bsp", "vpk", "gcf", "unity3d", "assets", "bank", "fsb", "bnk",
];

const GAME_FOLDER_NAMES: &[&str] = &[
    "steamapps",
    "common",
    "epic games",
    "gog galaxy",
    "origin games",
    "ubisoft game launcher",
    "riot games",
    "battle.net",
];

// ─────────────────────────────────────────────────────────────────────────────
// Public types  — all Clone so AppState can hold and clone them
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub filename: String,
    pub extension: String,
    pub size: u64,
    pub content_preview: String,
    pub is_text: bool,
}

#[derive(Debug)]
pub struct ScanResult {
    pub files: Vec<ScannedFile>,
    pub skipped_game_dirs: Vec<PathBuf>,
    pub total_scanned: usize,
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

pub fn scan_directory(root: &Path, max_read_size: usize) -> Result<ScanResult> {
    let files: DashMap<PathBuf, ScannedFile> = DashMap::new();
    let total_scanned = std::sync::atomic::AtomicUsize::new(0);

    let game_dir_set: HashSet<PathBuf> = identify_game_directories(root);

    // Collect top-level entries
    let entries: Vec<_> = WalkDir::new(root)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .collect();

    entries.par_iter().for_each(|entry| {
        let path = entry.path().to_path_buf();
        total_scanned.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        if game_dir_set.contains(&path) {
            return;
        }

        if path.is_file() {
            if let Some(scanned) = scan_single_file(&path, root, max_read_size) {
                files.insert(path, scanned);
            }
        } else if path.is_dir() {
            scan_directory_recursive(&path, root, max_read_size, &files, &game_dir_set);
        }
    });

    let files_vec: Vec<ScannedFile> = files.into_iter().map(|(_, v)| v).collect();
    let game_dirs_vec: Vec<PathBuf> = game_dir_set.into_iter().collect();

    Ok(ScanResult {
        total_scanned: total_scanned.load(std::sync::atomic::Ordering::Relaxed),
        files: files_vec,
        skipped_game_dirs: game_dirs_vec,
    })
}

pub fn detect_coding_project_roots(root: &Path) -> Vec<PathBuf> {
    const MARKERS: &[&str] = &[
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "setup.py",
        "pom.xml",
        "build.gradle",
        "go.mod",
        "Gemfile",
        "CMakeLists.txt",
        "Makefile",
        "pubspec.yaml",
        "mix.exs",
        "stack.yaml",
        "dune-project",
    ];

    let mut project_dirs = Vec::new();

    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            for marker in MARKERS {
                if path.join(marker).exists() {
                    project_dirs.push(path.clone());
                    break;
                }
            }
        }
    }

    project_dirs
}

/// Returns `true` for file extensions whose content is human-readable text
/// and therefore worth sending to Ollama for classification / renaming.
pub fn is_text_file(ext: &str) -> bool {
    matches!(
        ext,
        "txt" | "md" | "rs" | "py" | "js" | "ts" | "jsx" | "tsx"
            | "html" | "htm" | "css" | "scss" | "sass" | "less"
            | "json" | "yaml" | "yml" | "toml" | "xml" | "csv"
            | "c" | "cpp" | "h" | "hpp" | "java" | "kt" | "kts"
            | "go" | "rb" | "php" | "swift" | "r" | "sql"
            | "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd"
            | "dockerfile" | "makefile" | "cmake"
            | "gitignore" | "gitattributes" | "editorconfig"
            | "env" | "ini" | "cfg" | "conf" | "config"
            | "log" | "tex" | "rst" | "adoc" | "org"
            | "vue" | "svelte" | "astro"
            | "lock" | "sum"
            | "cs" | "fs" | "fsx" | "vb"
            | "lua" | "pl" | "pm" | "ex" | "exs"
            | "zig" | "nim" | "d" | "dart" | "scala" | "clj"
            | "erl" | "hrl" | "hs" | "lhs" | "ml" | "mli"
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Private helpers
// ─────────────────────────────────────────────────────────────────────────────

fn scan_directory_recursive(
    dir: &Path,
    root: &Path,
    max_read_size: usize,
    files: &DashMap<PathBuf, ScannedFile>,
    game_dirs: &HashSet<PathBuf>,
) {
    let entries: Vec<_> = WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| !game_dirs.contains(e.path()))
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .collect();

    entries.par_iter().for_each(|entry| {
        let path = entry.path().to_path_buf();
        if let Some(scanned) = scan_single_file(&path, root, max_read_size) {
            files.insert(path, scanned);
        }
    });
}

fn scan_single_file(path: &Path, root: &Path, max_read_size: usize) -> Option<ScannedFile> {
    let metadata = fs::metadata(path).ok()?;
    let filename = path.file_name()?.to_string_lossy().to_string();
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let relative_path = path.strip_prefix(root).unwrap_or(path).to_path_buf();

    let is_text = is_text_file(&extension);
    let content_preview = if is_text {
        read_preview(path, max_read_size).unwrap_or_default()
    } else {
        String::new()
    };

    Some(ScannedFile {
        path: path.to_path_buf(),
        relative_path,
        filename,
        extension,
        size: metadata.len(),
        content_preview,
        is_text,
    })
}

fn read_preview(path: &Path, max_size: usize) -> Result<String> {
    let content = fs::read(path)?;
    let len = content.len().min(max_size);
    let text = String::from_utf8_lossy(&content[..len]);
    Ok(text.chars().take(500).collect())
}

fn identify_game_directories(root: &Path) -> HashSet<PathBuf> {
    let mut game_dirs = HashSet::new();

    let entries: Vec<_> = WalkDir::new(root)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    for entry in entries {
        let path = entry.path().to_path_buf();
        if is_game_directory(&path) {
            game_dirs.insert(path);
        }
    }

    game_dirs
}

fn is_game_directory(dir: &Path) -> bool {
    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if GAME_FOLDER_NAMES.iter().any(|g| dir_name.contains(g)) {
        return true;
    }

    let mut game_score = 0u32;
    let mut total_files = 0u32;

    for entry in WalkDir::new(dir).max_depth(3).into_iter().filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        total_files += 1;
        if total_files > 500 {
            break;
        }

        for indicator in GAME_INDICATORS {
            if name.contains(indicator) {
                game_score += 3;
            }
        }

        if let Some(ext) = entry.path().extension() {
            let ext = ext.to_string_lossy().to_lowercase();
            if GAME_EXTENSIONS.contains(&ext.as_str()) {
                game_score += 2;
            }
            if ext == "exe" || ext == "dll" {
                game_score += 1;
            }
        }
    }

    game_score >= 5
}