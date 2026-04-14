
# 🗂️ AI File Organizer

A blazing-fast, AI-powered file organizer built in Rust that uses a local
[Ollama](https://ollama.ai) model to intelligently classify, group, and rename
files — completely offline, completely private.

---

## 📋 Table of Contents

- [Features](#-features)
- [Screenshots](#-screenshots)
- [Requirements](#-requirements)
- [Installation](#-installation)
- [Quick Start](#-quick-start)
- [GUI Guide](#-gui-guide)
- [CLI Guide](#-cli-guide)
- [How It Works](#-how-it-works)
- [File Categories](#-file-categories)
- [Game Detection](#-game-detection)
- [Project Detection](#-project-detection)
- [AI Renaming](#-ai-renaming)
- [Configuration](#-configuration)
- [Supported Models](#-supported-models)
- [Building from Source](#-building-from-source)
- [Architecture](#-architecture)
- [Troubleshooting](#-troubleshooting)
- [FAQ](#-faq)
- [Contributing](#-contributing)
- [License](#-license)

---

## ✨ Features

### Core
| Feature | Description |
|---|---|
| 🤖 **AI Classification** | Every file is sent to a local Ollama model for intelligent categorisation |
| 🏎️ **Multithreaded** | Parallel file scanning with `rayon`, async AI calls with `tokio` |
| 🔍 **Dry Run Mode** | Preview every action before a single file is touched |
| 🎮 **Game Detection** | Automatically identifies and skips game directories |
| 📦 **Project Grouping** | Detects coding projects and groups related files together |
| ✏️ **Smart Renaming** | Renames files with generic names based on their content |
| 🖥️ **GUI + CLI** | Full native GUI via `egui`; also works headless from the terminal |
| 🔒 **100 % Local** | No data ever leaves your machine — Ollama runs offline |

### GUI Highlights
- Live progress bars during analysis and execution
- Tabbed interface: Overview · Files · Moves · Renames · Projects
- Per-action checkboxes — approve or skip individual moves/renames
- Real-time log viewer with timestamps
- Connection status indicator for Ollama
- Native folder-picker dialog
- Filter / search across all tabs

### Safety
- **Dry Run by default** in the GUI — you must explicitly disable it to write
- Collision detection — never overwrites an existing file
- Cross-filesystem fallback (copy + delete when `rename(2)` fails)
- Game directories are never touched regardless of settings

---


---

## 📦 Requirements

### Runtime
| Dependency | Version | Purpose |
|---|---|---|
| [Ollama](https://ollama.ai) | ≥ 0.1.32 | Local AI inference |
| A supported Ollama model | — | See [Supported Models](#-supported-models) |
| Linux / macOS / Windows | — | GUI uses native OpenGL via `glow` |

### Build
| Dependency | Version |
|---|---|
| Rust toolchain | ≥ 1.75 (2024 edition) |
| `cargo` | ships with Rust |
| OpenGL drivers | for the GUI |
| `pkg-config` + `libssl-dev` | Linux only |

---

## 🚀 Installation

### Option A — Download a pre-built binary

> Binaries for Linux x86-64, macOS arm64, and Windows x86-64 are attached to
> every [GitHub Release](https://github.com/yourname/file-organizer/releases).

```bash
# Linux / macOS
chmod +x file-organizer
./file-organizer

# Windows
file-organizer.exe
```

### Option B — Build from source

```bash
git clone https://github.com/yourname/file-organizer.git
cd file-organizer

# Release build (recommended — ~10× faster than debug)
cargo build --release

# The binary is at:
./target/release/file-organizer        # Linux / macOS
./target/release/file-organizer.exe    # Windows
```

### Option C — Install via Cargo

```bash
cargo install --git https://github.com/yourname/file-organizer
```

---

## ⚡ Quick Start

### 1 — Start Ollama

```bash
# Install Ollama (if not already installed)
curl -fsSL https://ollama.ai/install.sh | sh

# Pull a model (llama3.2 is the default)
ollama pull llama3.2

# Start the server (runs in background)
ollama serve
```

### 2 — Launch the GUI

```bash
./file-organizer
```

### 3 — Organise a folder

1. Click **📂** and pick your messy folder
2. Make sure **Dry Run Mode** is checked (it is by default)
3. Click **🔍 Analyze Folder** and wait for the AI to finish
4. Review the **Moves**, **Renames**, and **Projects** tabs
5. Uncheck any actions you don't want
6. Uncheck **Dry Run Mode**
7. Click **🚀 Execute Selected**

---

## 🖥️ GUI Guide

### Layout

```
┌─ Top bar ──────────────────────────────────────────────┐
│  Title            Connection status         Settings   │
├─ Left panel ──┬─ Central panel (tabs) ─────────────────┤
│               │                                        │
│  Folder pick  │  Overview / Files / Moves /            │
│  Buttons      │  Renames / Projects                    │
│  Statistics   │                                        │
│  Phase        │                                        │
├───────────────┴────────────────────────────────────────┤
│  Progress bar + Log viewer                             │
└────────────────────────────────────────────────────────┘
```

### Left Panel

| Control | Description |
|---|---|
| **📂 Browse** | Open a native folder picker |
| **🔍 Analyze Folder** | Run the full AI analysis pipeline (enabled only when Ollama is connected) |
| **☐ Dry Run Mode** | When checked, nothing is written to disk |
| **🚀 Execute Selected** | Apply all ticked actions (disabled in dry-run mode) |
| **Statistics** | Live counts updated after analysis |
| **Phase indicator** | Shows the current pipeline stage and any error |

### Tabs

#### 📊 Overview
- Summary cards (files, moves, renames, projects, games skipped)
- Category breakdown bar chart
- List of skipped game directories

#### 📄 Files
Sortable, filterable table of every analysed file showing:
- Filename
- AI-assigned category and subcategory
- File size
- Relative path

#### 📁 Moves
Every planned file and directory move with:
- Individual checkboxes to include/exclude
- **Select All / Deselect All** buttons
- Source → destination paths relative to the root folder
- Project moves (whole directories) shown separately at the top

#### ✏ Renames
Every AI-suggested rename with:
- Old name (red strikethrough) → new name (green)
- Individual checkboxes
- **Select All / Deselect All** buttons

#### 📦 Projects
- Detected coding-project directories (will be moved into `coding_projects/`)
- AI-detected file groups with collapsible file lists

### Settings Panel

Open with **⚙ Settings** in the top-right corner.

| Setting | Default | Description |
|---|---|---|
| Ollama URL | `http://localhost:11434` | URL of the Ollama HTTP API |
| Model | `llama3.2` | Ollama model name |
| Threads | CPU count | Worker threads for file scanning |
| Max read size | 8 192 B | Max bytes read from each file for AI context |
| Skip renames | off | Disable the rename step entirely |

### Log Viewer

The bottom panel shows a timestamped, colour-coded log of every action:

| Colour | Meaning |
|---|---|
| 🔵 Blue | Informational |
| 🟡 Yellow | Warning |
| 🔴 Red | Error |
| 🟢 Green | Success |

---

## 💻 CLI Guide

### Basic usage

```bash
# Always run dry-run first!
./file-organizer --path /path/to/folder --dry-run

# Apply changes
./file-organizer --path /path/to/folder
```

### All flags

```
USAGE:
    file-organizer [OPTIONS]

OPTIONS:
    -p, --path <PATH>
            Target folder to organise

    -d, --dry-run
            Show what would happen without making any changes

    -m, --model <MODEL>
            Ollama model to use [default: llama3.2]

    -u, --url <URL>
            Ollama server URL [default: http://localhost:11434]

    -t, --threads <N>
            Worker threads for file scanning [default: number of CPUs]

        --max-read-size <BYTES>
            Maximum bytes read from each file for AI context [default: 8192]

        --skip-rename
            Skip the AI rename step

        --cli
            Force CLI mode even when a path is not provided
            (useful when you want a terminal UI on a headless server)

    -h, --help
            Print help

    -V, --version
            Print version
```

### Examples

```bash
# Dry-run with a different model
./file-organizer --path ~/Downloads --dry-run --model mistral

# Organise with 16 threads and a remote Ollama instance
./file-organizer \
  --path /mnt/nas/unsorted \
  --url http://192.168.1.50:11434 \
  --threads 16

# Skip renaming, just move files
./file-organizer --path ~/Desktop --skip-rename --dry-run

# Full verbose example
./file-organizer \
  --path /home/alice/chaos \
  --model codellama \
  --threads 8 \
  --max-read-size 16384 \
  --dry-run
```

### CLI output example

```
╔══════════════════════════════════════════╗
║   AI File Organizer (Ollama-powered)     ║
╚══════════════════════════════════════════╝

🔍 DRY RUN MODE - No changes will be made

📁 Target:  /home/alice/chaos
🤖 Model:   llama3.2
🧵 Threads: 8

✅ Ollama server is reachable

📂 Step 1: Scanning directory...
  Scanned 312 entries, found 289 files
  3 game directories detected and skipped:
    🎮 SteamLibrary
    🎮 Minecraft
    🎮 GOGGalaxy
  5 coding project directories detected:
    📦 my-rust-app
    📦 flask-api
    📦 dotfiles
    📦 unity-game
    📦 scripts

🤖 Step 2: Classifying files with AI...
  ████████████████████████████████ 289/289

🔗 Step 3: Detecting project groups...
  Found 7 project groups

📝 Step 4: Analyzing file renames...
  ████████████████░░░░░░░░░░░░░░░░  14/22
  22 files will be renamed

📋 Step 5: Planning file organization...
  Planned 241 file moves, 5 project moves

📝 Renames:
  untitled.txt        → meeting_notes_q4.txt
  temp.py             → image_resizer.py
  draft.md            → api_design_document.md

📦 Project Moves:
  my-rust-app/        → coding_projects/my-rust-app/
  flask-api/          → coding_projects/flask-api/

📁 File Moves:
  📂 documents (47 files):
    → report_2023.pdf
    → invoice_march.docx
    ... and 45 more
  📂 images (38 files):
    → photo_001.jpg
    ...

📊 Total: 241 moves, 5 project moves, 22 renames = 268 total

🔍 DRY RUN — No changes were made. Remove --dry-run to apply.

✨ Done!
```

---

## 🔬 How It Works

### Pipeline

```
┌────────────┐    ┌────────────┐    ┌────────────┐    ┌────────────┐    ┌──────────┐
│  1. Scan   │───▶│ 2. Classify│───▶│ 3. Group   │───▶│ 4. Rename  │───▶│ 5. Plan  │
│  (rayon)   │    │  (tokio +  │    │  (Ollama)  │    │  (Ollama)  │    │          │
│            │    │   Ollama)  │    │            │    │            │    │          │
└────────────┘    └────────────┘    └────────────┘    └────────────┘    └──────────┘
                                                                               │
                                                       ┌───────────────────────┘
                                                       ▼
                                                ┌────────────┐
                                                │ 6. Execute │
                                                │ (optional) │
                                                └────────────┘
```

### Step 1 — Scan
- `walkdir` traverses the target directory
- `rayon` parallelises the traversal across all CPU cores
- Game directories are identified by a heuristic scoring system and skipped
- Text files are read (up to `max_read_size` bytes) for AI context

### Step 2 — Classify
- Each file is sent to Ollama with its name, extension, and content preview
- Ollama returns a JSON object with `category`, `subcategory`,
  `suggested_name`, `project_hint`, and `confidence`
- Up to 4 concurrent requests to Ollama (controlled by a semaphore)
- Files are batched in groups of 8 to keep memory usage flat
- Falls back to extension-based classification if Ollama fails

### Step 3 — Group
- Loose files at the top level are described to Ollama in a single prompt
- Ollama returns a list of groups — related files that belong together
- Groups with more than one file get their own sub-folder inside the
  category directory (e.g. `coding_projects/my_flask_app/`)

### Step 4 — Rename
- Only text-based files with generic stems are considered
  (`untitled`, `temp`, `draft`, `copy`, `backup`, `test`, etc.)
- Well-known config files (`Cargo.toml`, `.gitignore`, `Makefile`, etc.)
  are always skipped
- Ollama suggests a descriptive snake_case name based on file content
- The rename is skipped if the new name already exists

### Step 5 — Plan
- All moves and renames are collected into an action list
- In GUI mode the list is shown for review; individual actions can be
  deselected before execution
- In CLI mode the plan is printed; `--dry-run` stops here

### Step 6 — Execute
- Renames are applied first (in-place, same directory)
- Project directory moves happen next
- Individual file moves happen last
- `rename(2)` is tried first; if it fails (cross-device) the tool falls
  back to `copy` + `delete`
- Parent directories are created automatically
- Existing destinations are skipped (never overwritten)

---

## 📁 File Categories

| Category | Folder | Typical extensions |
|---|---|---|
| `coding_project` | `coding_projects/` | `.rs` `.py` `.js` `.ts` `.go` `.java` `.cpp` `.cs` … |
| `web` | `web_projects/` | `.html` `.css` `.scss` `.vue` `.svelte` `.jsx` … |
| `documents` | `documents/` | `.txt` `.md` `.pdf` `.docx` `.odt` `.tex` … |
| `images` | `images/` | `.png` `.jpg` `.gif` `.svg` `.webp` `.bmp` … |
| `music` | `music/` | `.mp3` `.flac` `.wav` `.ogg` `.aac` … |
| `videos` | `videos/` | `.mp4` `.mkv` `.avi` `.mov` `.webm` … |
| `archives` | `archives/` | `.zip` `.tar` `.gz` `.7z` `.rar` `.xz` … |
| `data` | `data_files/` | `.json` `.csv` `.xml` `.yaml` `.sql` `.db` … |
| `config` | `config_files/` | `.toml` `.ini` `.cfg` `.conf` `.env` … |
| `scripts` | `scripts/` | `.sh` `.bash` `.ps1` `.bat` `.fish` … |
| `misc` | `misc/` | Anything not matched above |

Related files within the same category are further grouped into named
sub-folders when the AI detects they belong to the same project or topic.

---

## 🎮 Game Detection

Game directories are detected by a heuristic scoring system. A directory is
considered a game installation if it scores **≥ 5 points** based on:

| Signal | Points |
|---|---|
| Directory name matches a known launcher (`steamapps`, `Epic Games`, `GOG Galaxy` …) | immediate |
| Contains a file/folder matching a game-engine pattern | +3 per match |
| Contains a game-data file extension (`.pak`, `.unity3d`, `.bank` …) | +2 per match |
| Contains `.exe` or `.dll` files | +1 per match |

### Recognised patterns

**Engines / launchers:** Unity, Unreal Engine, Godot, GameMaker, Steam, Epic Games,
GOG Galaxy, Origin, Ubisoft Connect, Riot Games, Battle.net

**File types:** `.pak` `.pck` `.wad` `.bsp` `.vpk` `.gcf` `.unity3d` `.assets`
`.bank` `.fsb` `.bnk`

**Marker files:** `steam_api.dll` `steam_appid.txt` `installscript.vdf`
`boot.config` `globalgamemanagers` `resources.assets`

> ⚠️ Game directories are **always skipped** regardless of other settings.
> They are never moved, renamed, or modified in any way.

---

## 📦 Project Detection

A directory is classified as a **coding project** when it contains one of
these marker files at its root:

| File | Ecosystem |
|---|---|
| `Cargo.toml` | Rust |
| `package.json` | Node.js / JavaScript / TypeScript |
| `pyproject.toml` / `setup.py` | Python |
| `go.mod` | Go |
| `pom.xml` / `build.gradle` | Java / Kotlin |
| `Gemfile` | Ruby |
| `CMakeLists.txt` / `Makefile` | C / C++ |
| `pubspec.yaml` | Dart / Flutter |
| `mix.exs` | Elixir |
| `stack.yaml` / `dune-project` | Haskell / OCaml |
| `*.sln` / `*.csproj` | .NET / C# |

Detected project directories are moved as a whole unit into
`coding_projects/<project-name>/` preserving their internal structure.

---

## ✏️ AI Renaming

### Which files are considered

A file is a candidate for renaming if **all** of the following are true:

1. It has a text-based extension (`.txt`, `.md`, `.rs`, `.py`, `.html`, etc.)
2. Its stem matches a generic pattern:

| Pattern | Examples |
|---|---|
| Exact match | `untitled.txt` `temp.py` `test.md` `file.rs` |
| Prefixed | `untitled_1.txt` `temp_backup.py` |
| Suffixed with digits | `untitled1.txt` `temp2.md` |

### Which files are always skipped

Well-known config and project files are never renamed:

`Cargo.toml` · `Cargo.lock` · `package.json` · `package-lock.json` ·
`tsconfig.json` · `go.mod` · `go.sum` · `Gemfile` · `Makefile` ·
`Dockerfile` · `README` · `LICENSE` · `CHANGELOG` · `.gitignore` ·
`.gitattributes` · `.editorconfig` · `.env` · `pyproject.toml` ·
`requirements.txt` · `yarn.lock` · `pnpm-lock.yaml` · `flake.nix` ·
`flake.lock` · `shell.nix` · `default.nix` · and more

### Rename rules enforced on Ollama's output

- Must be a valid filename (no `/` or `\`)
- Maximum 100 characters
- Same extension as the original
- snake_case preferred
- If Ollama returns an invalid suggestion the original name is kept

---

## ⚙️ Configuration

There is no config file — all settings are passed as CLI flags or set
through the GUI settings panel. Settings chosen in the GUI are applied
immediately and persist for the current session only.

### Environment variables

| Variable | Description |
|---|---|
| `RUST_LOG` | Log level filter (e.g. `RUST_LOG=file_organizer=debug`) |

---

## 🤖 Supported Models

Any model available in Ollama will work. Recommended options:

| Model | Pull command | Notes |
|---|---|---|
| `llama3.2` ⭐ | `ollama pull llama3.2` | Best balance of speed and quality |
| `llama3.1` | `ollama pull llama3.1` | Larger, slower, slightly better |
| `mistral` | `ollama pull mistral` | Fast, good for classification |
| `codellama` | `ollama pull codellama` | Best for coding-project detection |
| `phi3` | `ollama pull phi3` | Smallest, fastest, good on low-RAM systems |
| `gemma2` | `ollama pull gemma2` | Google's open model, good quality |

### Minimum hardware

| Model | RAM | VRAM (GPU) |
|---|---|---|
| `phi3` (3.8 B) | 4 GB | 4 GB |
| `mistral` (7 B) | 8 GB | 6 GB |
| `llama3.2` (3 B) | 4 GB | 4 GB |
| `llama3.1` (8 B) | 12 GB | 8 GB |
| `codellama` (7 B) | 8 GB | 6 GB |

> The tool works on CPU-only machines — it will just be slower.
> GPU acceleration is handled entirely by Ollama.

---

## 🔧 Building from Source

```bash
# Prerequisites (Ubuntu / Debian)
sudo apt install build-essential pkg-config libssl-dev

# Prerequisites (Fedora)
sudo dnf install gcc pkg-config openssl-devel

# Prerequisites (macOS)
xcode-select --install   # Clang + build tools

# Clone
git clone https://github.com/yourname/file-organizer.git
cd file-organizer

# Build optimised release binary (GUI enabled)
cargo build --release

# Build CLI-only binary (no GUI, smaller binary, no OpenGL needed)
cargo build --release --no-default-features --features cli-only

# Run tests
cargo test

# Check for issues
cargo clippy -- -D warnings

# Format code
cargo fmt
```

### Feature flags

| Flag | Default | Description |
|---|---|---|
| `gui` | ✅ on | Includes `eframe`/`egui` GUI |
| `cli-only` | ❌ off | Strips all GUI code for headless deployments |

```bash
# Headless server / container build
cargo build --release --no-default-features --features cli-only
```

### Release profile settings

```toml
[profile.release]
opt-level     = 3        # Maximum optimisation
lto           = true     # Link-time optimisation (smaller + faster binary)
codegen-units = 1        # Single codegen unit (slower compile, faster runtime)
strip         = true     # Strip debug symbols
panic         = "abort"  # Smaller binary, faster panics
```

---

## 🏗️ Architecture

### Module overview

```
src/
├── main.rs          Entry point — parses args, routes to GUI or CLI
├── cli.rs           CLI runner — sets up Tokio runtime, calls Organizer
├── gui.rs           Full egui GUI — panels, tabs, command channel
├── app_state.rs     Shared state struct (Arc<RwLock<AppState>>)
├── organizer.rs     Pipeline orchestrator — scan → classify → group → rename → plan
├── scanner.rs       Parallel directory walker, game-dir detector, project-root finder
├── classifier.rs    Async batch classifier, fallback classifier, group detector
├── renamer.rs       Async batch renamer, generic-name filter
└── ollama.rs        Ollama HTTP client, JSON prompt/response helpers
```

### Concurrency model

```
Main thread (UI)
    │
    ├── egui render loop (60 fps)
    │       reads:  Arc<RwLock<AppState>>  (try_read, non-blocking)
    │       writes: std::sync::mpsc::Sender<UiCmd>
    │
    └── Tokio runtime (separate OS thread pool)
            │
            ├── UiCmd processor  (spawned tasks mutate AppState via write())
            │
            ├── Scanner          (rayon thread pool inside)
            │
            ├── Classifier tasks (up to 8 concurrent, Semaphore limits Ollama to 4)
            │
            └── Renamer tasks    (up to 4 concurrent)
```

### Key design decisions

| Decision | Rationale |
|---|---|
| `Arc<RwLock<AppState>>` | Single source of truth shared between UI and background tasks |
| `mpsc` command channel | UI never calls `block_on` — avoids deadlocks in the render loop |
| `try_read` for snapshots | UI never blocks waiting for a lock — stays responsive |
| `rayon` for scanning | CPU-bound work; saturates all cores without async overhead |
| `tokio` for Ollama calls | I/O-bound work; many concurrent requests with low thread count |
| `Semaphore(4)` on Ollama | Prevents overwhelming a single Ollama instance |
| Fallback classifiers | Ensures the tool works even if Ollama returns malformed JSON |

---

## 🩺 Troubleshooting

### Ollama not connecting

```
● Ollama Disconnected
```

**Fix:**
```bash
# Check if Ollama is running
curl http://localhost:11434/api/tags

# Start Ollama
ollama serve

# If using a custom port
./file-organizer --url http://localhost:11435
```

### Model not found

```
Error: Ollama error 404: model not found
```

**Fix:**
```bash
# Pull the model first
ollama pull llama3.2

# List available models
ollama list
```

### GUI fails to open (Linux)

```
Error: Failed to create window: NoAvailablePixelFormat
```

**Fix:**
```bash
# Install OpenGL drivers
sudo apt install libgl1-mesa-glx   # Ubuntu/Debian
sudo dnf install mesa-libGL        # Fedora

# Or use software rendering
LIBGL_ALWAYS_SOFTWARE=1 ./file-organizer
```

### Out of memory during classification

If you have tens of thousands of files, Ollama may run out of VRAM.

**Fix:**
```bash
# Use a smaller model
./file-organizer --model phi3

# Reduce concurrent reads
./file-organizer --max-read-size 1024

# Use a smaller context model
ollama pull phi3:mini
```

### Files not being classified correctly

**Causes and fixes:**

| Cause | Fix |
|---|---|
| Model too small | Switch to `llama3.2` or `llama3.1` |
| `max-read-size` too small | Increase to `16384` or `32768` |
| Non-UTF-8 files | Expected — binary files get extension-based fallback |
| Ollama timeout | Increase timeout in `ollama.rs` (`from_secs(120)`) |

### Permission denied errors

```
Error: Os { code: 13, kind: PermissionDenied }
```

**Fix:**
```bash
# Check ownership
ls -la /path/to/folder

# Fix permissions
chmod -R u+rw /path/to/folder
```

### Cross-device move errors

This is handled automatically — the tool falls back to `copy` + `delete`
when `rename(2)` fails (e.g. moving between different filesystems or drives).

---

## ❓ FAQ

**Q: Does this send my files to the internet?**

No. Ollama runs entirely on your local machine. No data ever leaves your
network. The only HTTP requests made are to `localhost:11434` (or whatever
URL you configure).

---

**Q: Can I use a remote Ollama server?**

Yes:
```bash
./file-organizer --url http://192.168.1.100:11434
```

---

**Q: Will it modify files inside game directories?**

Never. Game directories are detected before any other processing and
completely excluded from all operations.

---

**Q: What if I don't like a rename suggestion?**

Uncheck the checkbox next to that rename in the **✏ Renames** tab before
clicking Execute. You can also click **Deselect All** and cherry-pick only
the renames you want.

---

**Q: Can I undo an operation?**

Not automatically. Always run with `--dry-run` first. Consider making a
backup or running on a copy of your folder for the first test.

---

**Q: The AI classified a file incorrectly. What happens?**

Each file has a fallback classification based on its extension alone. If
Ollama returns invalid JSON or an unrecognised category, the fallback is used
automatically. The file will still be moved to a reasonable location.

---

**Q: How long does analysis take?**

Depends on file count and hardware:

| Files | Hardware | Time (approx.) |
|---|---|---|
| 100 | CPU-only, phi3 | ~2 min |
| 100 | GPU (RTX 3080), llama3.2 | ~15 sec |
| 1 000 | GPU (RTX 3080), llama3.2 | ~2 min |
| 10 000 | GPU (RTX 3080), llama3.2 | ~20 min |

---

**Q: Can I organise network drives or NAS?**

Yes, as long as the path is mounted and accessible. Cross-device moves
(different filesystems) are handled automatically with copy + delete.

---

**Q: Can I run this on a headless server?**

Yes, use the CLI mode:
```bash
cargo build --release --no-default-features --features cli-only
./file-organizer --path /data/unsorted --dry-run
```

---

## 🤝 Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Run `cargo fmt` and `cargo clippy -- -D warnings`
4. Add tests for new functionality
5. Open a pull request

### Areas that would benefit from contributions

- [ ] Undo / history log
- [ ] Custom category rules (user-defined regex → category mapping)
- [ ] Config file support (`~/.config/file-organizer/config.toml`)
- [ ] Progress persistence (resume interrupted large runs)
- [ ] Windows installer / macOS `.app` bundle
- [ ] More granular game-engine detection
- [ ] Unit tests for the classifier fallback logic
- [ ] Benchmark suite

---

## 📄 License

MIT License — see [LICENSE](LICENSE) for full text.

```
MIT License

Copyright (c) 2024 Your Name

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## 🙏 Acknowledgements

| Project | Role |
|---|---|
| [Ollama](https://ollama.ai) | Local AI inference engine |
| [egui / eframe](https://github.com/emilk/egui) | Immediate-mode GUI framework |
| [Tokio](https://tokio.rs) | Async runtime |
| [Rayon](https://github.com/rayon-rs/rayon) | Data-parallelism library |
| [walkdir](https://github.com/BurntSushi/walkdir) | Directory traversal |
| [rfd](https://github.com/PolyMeilex/rfd) | Native file dialogs |
| [anyhow](https://github.com/dtolnay/anyhow) | Ergonomic error handling |
| [serde](https://serde.rs) | Serialisation framework |
```
