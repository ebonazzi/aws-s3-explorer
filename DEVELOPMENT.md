# aws-s3-explorer — Development Guide

> **Audience:** This document is the primary technical reference for both human developers
> (Rust programmers) and Claude Sonnet working on this project in future sessions.
> It covers architecture, setup, build, configuration, internals, standards, and known
> limitations in enough depth that work can resume in any new session without loss of context.

---

## Table of Contents

1. [Project Overview](#1-project-overview)
2. [Technology Stack](#2-technology-stack)
3. [Module Structure](#3-module-structure)
4. [Architecture — Threading Model](#4-architecture--threading-model)
5. [Architecture — Data Flow](#5-architecture--data-flow)
6. [Setup: Windows 11](#6-setup-windows-11)
7. [Setup: Ubuntu Linux 24.04 LTS](#7-setup-ubuntu-linux-2404-lts)
8. [Build and Run](#8-build-and-run)
9. [AWS Credentials and Configuration](#9-aws-credentials-and-configuration)
10. [Application Configuration (config.json)](#10-application-configuration-configjson)
11. [How the Application Works — User Perspective](#11-how-the-application-works--user-perspective)
12. [How the Application Works — Internal Architecture](#12-how-the-application-works--internal-architecture)
13. [Module Reference](#13-module-reference)
14. [Code Standards](#14-code-standards)
15. [Testing](#15-testing)
16. [Key Design Decisions and Rationale](#16-key-design-decisions-and-rationale)
17. [Known Limitations (Phase 1)](#17-known-limitations-phase-1)
18. [Phase 2 Planned Features](#18-phase-2-planned-features)
19. [Troubleshooting](#19-troubleshooting)
20. [Git Conventions](#20-git-conventions)

---

## 1. Project Overview

`aws-s3-explorer` is a personal desktop GUI application for managing files between a local
filesystem and AWS S3. It is written in pure Rust using the `egui` immediate-mode GUI toolkit
and the official AWS Rust SDK.

**Core capabilities:**

| Feature | Description |
|---|---|
| Dual-pane browser | Left: local filesystem. Right: S3 buckets and prefixes. |
| File upload | Single files via toolbar button or right-click "Copy to S3". |
| File download | Single objects via toolbar button or right-click "Download". |
| Recursive folder upload | Right-click any local folder → "Upload folder to S3 →". Creates the full sub-tree in S3. |
| Recursive folder download | Right-click any S3 prefix → "Download folder to local ↓". Recreates local folder tree. |
| Delete | Local files and S3 objects, with optional confirmation dialog. |
| Sync | Compare local directory vs. S3 prefix by name + size + mtime (±2 s tolerance). Show a plan; execute or dry-run. |
| Transfer queue | Bottom panel shows every job: Queued / In Progress / Done / Failed / Skipped. |
| Refresh | ⟳ button on both panes to re-fetch the current listing. |

**Explicit non-goals (Phase 1):**

- No Azure, GCP or other cloud providers.
- No multipart upload — single `PutObject` for all file sizes.
- No byte-level progress bars — file/object-level only.
- No S3 bucket creation/deletion.
- No drag-and-drop.
- No recursive Sync (single-level only — recursive folder ops use Upload/Download buttons).
- No runtime AWS profile switching — uses whatever `aws configure` set up.

---

## 2. Technology Stack

All crate versions are pinned to the values below. Do not upgrade without verifying API
compatibility and re-running `cargo clippy -- -D warnings -W clippy::pedantic`.

| Crate | Version | Role |
|---|---|---|
| `eframe` | `0.35.0` | App shell: OS window, event loop, wgpu GPU rendering surface |
| `egui` | `0.35.0` | Immediate-mode GUI widgets (direct dep required; eframe re-exports but not all paths) |
| `egui_extras` | `0.35.0` | `TableBuilder` for resizable/striped file-list columns |
| `rfd` | `0.17.2` | Native OS folder picker dialog (GTK 3 on Linux, Explorer on Windows) |
| `aws-config` | `1.8.18` | AWS credential + region resolution (same chain as CLI v2) |
| `aws-sdk-s3` | `1.137.0` | S3 operations: list, put, get, delete, head |
| `tokio` | `1.52.3` | Async runtime (required by AWS SDK) |
| `flume` | `0.12.0` | MPMC channel bridging tokio tasks ↔ synchronous eframe render loop |
| `walkdir` | `2` | Recursive local directory traversal for folder uploads |
| `anyhow` | `1` | Application-level error handling |
| `serde` | `1` | Serialisation framework |
| `serde_json` | `1` | JSON for config file |
| `humansize` | `2` | Format bytes as "1.4 MiB", "230 KiB" |
| `chrono` | `0.4` | DateTime formatting for "last modified" column |
| `dirs` | `6` | Platform config directory without `#[cfg]` (`~/.config` vs `%APPDATA%`) |
| `tracing` | `0.1` | Structured logging |
| `tracing-subscriber` | `0.3` | Log output with env-filter |

### Critical egui 0.35.0 API notes

These API details differ from older egui tutorials and examples online:

- **`App::ui` not `App::update`**: Since eframe 0.34, the render entry point is
  `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`.
  The old `fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame)` is deprecated.

- **`Panel::top/bottom/left/right`** replaces the old `TopBottomPanel` and `SidePanel` types.
  Use `egui::Panel::top("id")`, `egui::Panel::left("id")`, etc.

- **`show_inside` → `show`**: When called from `App::ui` (which receives `&mut Ui`),
  use `.show(ui, |ui| { ... })`. The old `.show_inside()` is now deprecated.

- **`egui` must be a direct Cargo dependency** (`egui = "0.35.0"`). While `eframe` re-exports
  `egui`, some panel types (`Panel`, etc.) are not accessible via `eframe::egui::Panel` in this
  version. Adding `egui` directly as a dep resolves this.

- **No `egui_extras::install_image_loaders`**: Not needed unless displaying images.
  Calling it produces a runtime warning.

---

## 3. Module Structure

```
aws-s3-explorer/
├── Cargo.toml              ← All pinned dependencies with inline rationale comments
├── Cargo.lock              ← Committed (binary crate)
├── README.md               ← User-facing: setup and quick-start
├── DEVELOPMENT.md          ← This file: technical reference for developers and Claude
├── .gitattributes          ← Enforces LF line endings in repo (critical for cross-platform)
├── .gitignore
└── src/
    ├── main.rs             ← Entry point: tracing init, tokio runtime, eframe::run_native
    ├── app.rs              ← S3ExplorerApp struct, eframe::App impl, all background launchers
    ├── config.rs           ← AppConfig: load/save JSON config file
    ├── types.rs            ← ALL shared domain types (no intra-crate imports from here)
    ├── ui/
    │   ├── mod.rs          ← Re-exports, format_bytes(), status_colour(), kind_icon()
    │   ├── toolbar.rs      ← Top bar: address paths, action buttons, status bar
    │   ├── local_pane.rs   ← Left pane: TableBuilder file browser, context menus
    │   ├── s3_pane.rs      ← Right pane: bucket list + prefix browser, context menus
    │   └── transfer_panel.rs ← Bottom panel: transfer queue rows
    ├── fs/
    │   ├── mod.rs
    │   └── local.rs        ← list_directory() + collect_files_recursive() (spawn_blocking)
    ├── s3/
    │   ├── mod.rs
    │   └── client.rs       ← All S3 operations + StorageClass conversion
    └── sync/
        ├── mod.rs
        └── engine.rs       ← Pure sync plan logic (no I/O) + unit tests
```

---

## 4. Architecture — Threading Model

```
┌──────────────────────────────────────────────────────────────┐
│ OS Main Thread                                               │
│                                                              │
│  main()                                                      │
│   ├── init tracing                                           │
│   ├── load AppConfig from JSON                               │
│   ├── build tokio Runtime (4 worker threads)                 │
│   ├── runtime.block_on(build_s3_client())  ← credentials    │
│   └── eframe::run_native()  ← takes over thread forever     │
│                                                              │
│  eframe render loop (~60 fps or on input)                    │
│   └── App::ui(&mut self, ui: &mut Ui)                        │
│        ├── drain msg_rx (try_recv loop)                      │
│        │    └── apply_message() mutates App state            │
│        ├── draw_toolbar(ui)                                  │
│        ├── draw_main_panels(ui)                              │
│        │    ├── Panel::top → toolbar                         │
│        │    ├── Panel::bottom → transfer_panel               │
│        │    ├── Panel::left → local_pane                     │
│        │    └── CentralPanel → s3_pane                       │
│        └── modal dialogs (sync plan, delete confirm)         │
└──────────────────────────────────────────────────────────────┘
          ↕  msg_tx / msg_rx   (flume::unbounded::<AppMsg>)
          ↕  ctx.request_repaint()  wakes render loop
┌──────────────────────────────────────────────────────────────┐
│ tokio Runtime (background OS threads)                        │
│                                                              │
│  Spawned tasks (tokio_handle.spawn):                         │
│   • list_directory()      → LocalListingDone/Error           │
│   • list_buckets()        → BucketsLoaded/Error              │
│   • list_prefix()         → S3ListingDone/Error              │
│   • collect_files_recursive() → FolderScanComplete           │
│   • list_all_objects_recursive() → S3RecursiveListComplete   │
│   • compute_plan_*()      → SyncPlanReady/Error              │
│                                                              │
│  Transfer worker (single long-lived task):                   │
│   • reads from job_rx (flume::unbounded::<TransferJob>)      │
│   • executes ONE job at a time (sequential)                  │
│   • sends TransferStarted / TransferDone / TransferFailed    │
└──────────────────────────────────────────────────────────────┘
```

### Why manual `tokio::Runtime` instead of `#[tokio::main]`

`eframe::run_native()` never returns — it becomes the OS event loop. `#[tokio::main]`
wraps `main()` in `block_on()`, which would conflict. Building the runtime manually lets
us clone the `Handle` into `App` before `run_native()` takes over, then spawn tasks from
within the synchronous render loop.

### Why `flume` instead of `tokio::sync::mpsc`

The transfer queue consumer is the **synchronous** eframe render loop. `tokio::sync::mpsc`'s
`Receiver::try_recv()` works only inside an async context. `flume::Receiver::try_recv()` is
synchronous — it can be called freely from `App::ui()`.

### Transfer worker design

A single long-lived tokio task is spawned in `App::new()`. It reads `TransferJob` values
from `job_rx` one at a time, executes the transfer (upload / download / delete), and sends
`TransferStarted` / `TransferDone` / `TransferFailed` messages back via `msg_tx`. Sequential
execution is guaranteed by the blocking `recv_async().await` on the channel — the next job
only starts after the current one finishes.

---

## 5. Architecture — Data Flow

### S3 key construction for folder uploads

When a user right-clicks folder `C:\Photos\Italy\` and selects "Upload folder to S3 →"
while the S3 pane shows prefix `backups/`:

```
local_root  = C:\Photos\Italy\          (the folder right-clicked)
s3_prefix   = "backups/"               (current S3 browsing prefix)
folder_name = "Italy"                  (local_root.file_name())
dest_prefix = "backups/Italy/"         (s3_prefix + folder_name + "/")

For file: C:\Photos\Italy\Venice\IMG_001.jpg
  rel  = "Venice/IMG_001.jpg"           (strip local_root, replace \ with /)
  key  = "backups/Italy/Venice/IMG_001.jpg"
```

### S3 → local path construction for folder downloads

When a user right-clicks S3 prefix `2023/Italy/` and selects "Download folder to local ↓"
while the local pane shows `C:\Backup\` and S3 pane browsing prefix is `2023/`:

```
s3_folder_prefix  = "2023/Italy/"       (the prefix right-clicked)
s3_prefix (strip) = "2023/"             (current browsing prefix)
local_root        = C:\Backup\

For object: "2023/Italy/Venice/IMG_001.jpg"
  rel        = "Italy/Venice/IMG_001.jpg"  (strip "2023/" prefix)
  local_path = C:\Backup\Italy\Venice\IMG_001.jpg  (split on '/', PathBuf::push)
  (create_dir_all creates Italy\ and Italy\Venice\ automatically)
```

### Sync engine

`sync::engine::compute_plan_local_to_s3()` and `compute_plan_s3_to_local()` are **pure
functions** with no I/O. They take two entry lists and return a `SyncPlan`.

Match criterion: `size_bytes` equal AND `last_modified` within ±2 seconds.
The 2-second tolerance accounts for FAT32/NTFS/S3 timestamp precision differences.

The engine runs in a tokio task (CPU work, but kept off the render thread):
1. User clicks Sync →
2. Task runs `compute_plan_*()` on cloned entry lists
3. Sends `AppMsg::SyncPlanReady(plan)`
4. UI shows confirmation dialog
5. User clicks Execute → `execute_sync_plan()` enqueues all jobs

---

## 6. Setup: Windows 11

### Install Rust

```powershell
# Download and run rustup installer from https://rustup.rs
# Accept defaults (stable toolchain, MSVC ABI)
rustup show   # confirm: stable-x86_64-pc-windows-msvc, rustc ≥ 1.96
```

### No additional system packages needed

Windows 11 includes the necessary graphics drivers (Direct3D / Vulkan). eframe uses wgpu
which selects the best available backend automatically.

### AWS credentials

```powershell
# Option 1 — AWS CLI (recommended)
aws configure
# Prompts for: Access Key ID, Secret Access Key, Region, Output format

# Option 2 — Manual files
# Create %USERPROFILE%\.aws\credentials :
[default]
aws_access_key_id = AKIA...
aws_secret_access_key = ...

# Create %USERPROFILE%\.aws\config :
[default]
region = eu-west-1
```

### Clone and build

```powershell
git clone https://github.com/yuribudilov/aws-s3-explorer.git
cd aws-s3-explorer
cargo build --release   # first build: 5–15 min (downloads ~300 crates)
cargo run --release
```

### Environment variables (optional)

```powershell
# Use a non-default AWS profile
$env:AWS_PROFILE = "work"

# Override region
$env:AWS_DEFAULT_REGION = "us-east-1"

# Verbose logging
$env:RUST_LOG = "aws_s3_explorer=debug,warn"

# Force software GPU renderer (rare, for GPU-less VMs)
$env:WGPU_BACKEND = "gl"
```

---

## 7. Setup: Ubuntu Linux 24.04 LTS

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Choose option 1 (default install)
source "$HOME/.cargo/env"
rustup show   # confirm: stable-x86_64-unknown-linux-gnu, rustc ≥ 1.96
```

### Install system build dependencies

These are required by eframe (wgpu + winit) and by the rfd folder picker (GTK 3).
They are needed at **build time** and at **runtime**.

```bash
sudo apt-get update
sudo apt-get install -y \
  libclang-dev \
  libgtk-3-dev \
  libxcb-render0-dev \
  libxcb-shape0-dev \
  libxcb-xfixes0-dev \
  libxkbcommon-dev \
  libssl-dev \
  pkg-config \
  build-essential
```

| Package | Required by |
|---|---|
| `libclang-dev` | AWS SDK build scripts (bindgen) |
| `libgtk-3-dev` | `rfd` folder picker dialog |
| `libxcb-*`, `libxkbcommon-dev` | eframe X11 backend (winit) |
| `libssl-dev` | AWS SDK TLS (rustls-native-certs) |
| `pkg-config`, `build-essential` | Linker and C compilation |

### AWS credentials

```bash
# Option 1 — AWS CLI
aws configure

# Option 2 — Manual
mkdir -p ~/.aws

# ~/.aws/credentials:
[default]
aws_access_key_id = AKIA...
aws_secret_access_key = ...

# ~/.aws/config:
[default]
region = eu-west-1
```

### Clone and build

```bash
git clone https://github.com/yuribudilov/aws-s3-explorer.git
cd aws-s3-explorer
cargo build --release
```

### Run

```bash
# Standard (recommended for Ubuntu 24.04 with GNOME 46 Wayland):
cargo run --release
# or:
./target/release/aws-s3-explorer
```

### Wayland vs X11 notes for Ubuntu 24.04

**Ubuntu 24.04 with GNOME 46 (the default desktop):**
- Runs natively on Wayland.
- GTK 3 (used by `rfd` for the folder picker) has a native Wayland backend.
- **`GDK_BACKEND=x11` is NOT needed** on Ubuntu 24.04 in standard configuration.
- eframe auto-selects the Wayland backend when `WAYLAND_DISPLAY` is set.

**If the folder picker ("Choose Folder") does not appear:**
- This can happen in some non-GNOME Wayland compositors (Sway, Hyprland, etc.).
- Workaround: force X11/XWayland mode:
  ```bash
  GDK_BACKEND=x11 ./target/release/aws-s3-explorer
  ```
- XWayland is installed by default on Ubuntu 24.04 with GNOME.

**If eframe fails to open a window (GPU errors):**
```bash
WGPU_BACKEND=gl ./target/release/aws-s3-explorer
```

### Environment variables (Linux)

```bash
# Non-default AWS profile
export AWS_PROFILE=work

# Override region
export AWS_DEFAULT_REGION=eu-west-1

# Verbose logging
export RUST_LOG=aws_s3_explorer=debug,warn

# Force X11 for folder picker (only if needed — not needed on Ubuntu 24.04 GNOME)
export GDK_BACKEND=x11

# Force software GPU renderer (VMs, headless, old hardware)
export WGPU_BACKEND=gl
```

---

## 8. Build and Run

### ⚠️ Always use `--release` for testing

```bash
# WRONG — debug mode is unusable for GUI testing:
cargo run

# CORRECT — always for any real usage:
cargo run --release
cargo build --release
```

**Why:** `egui` is an **immediate-mode** renderer. Every frame (up to 60/s), every widget
is evaluated, every layout is computed, every draw call is issued — in Rust code running
directly on the CPU. In debug mode, all of this runs with zero compiler optimisations,
producing **single-digit FPS** that makes the application completely unusable for testing.
Release mode enables full LLVM optimisation (O3 + LTO + codegen-units=1) and typically
gives 100–300× faster rendering.

Debug builds are only useful for `cargo check`, `cargo test`, and `cargo clippy`.

### Development workflow commands

```bash
# Type-check and find errors (fastest — no codegen):
cargo check

# Run unit tests (sync engine):
cargo test

# Lint with pedantic clippy (must pass with zero warnings):
cargo clippy -- -D warnings -W clippy::pedantic

# Format code:
cargo fmt

# Check format without modifying:
cargo fmt -- --check

# Build release binary:
cargo build --release

# Run in release mode:
cargo run --release

# Run with verbose logging:
RUST_LOG=aws_s3_explorer=debug,warn cargo run --release
```

### Release binary locations

| Platform | Path |
|---|---|
| Linux | `target/release/aws-s3-explorer` |
| Windows | `target\release\aws-s3-explorer.exe` |

---

## 9. AWS Credentials and Configuration

### Credential resolution order (same as AWS CLI v2)

1. Environment variables: `AWS_ACCESS_KEY_ID` + `AWS_SECRET_ACCESS_KEY`
2. `AWS_PROFILE` environment variable → named profile in `~/.aws/`
3. Default profile in `~/.aws/credentials`
4. EC2 instance metadata (not relevant for desktop use)

### Region resolution order

1. `AWS_DEFAULT_REGION` environment variable
2. `region` in the active profile in `~/.aws/config`

### Credentials file format

**`~/.aws/credentials`** (Linux) / **`%USERPROFILE%\.aws\credentials`** (Windows):
```ini
[default]
aws_access_key_id = AKIAIOSFODNN7EXAMPLE
aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY

[work]
aws_access_key_id = AKIA...
aws_secret_access_key = ...
```

**`~/.aws/config`** (Linux) / **`%USERPROFILE%\.aws\config`** (Windows):
```ini
[default]
region = eu-west-1
output = json

[profile work]
region = us-east-1
```

### Switching profiles at runtime

The app does not support runtime profile switching. To use a different profile, launch with:
```bash
AWS_PROFILE=work ./target/release/aws-s3-explorer
```

### Storage classes

Configured in `config.json` (see Section 10). The storage class is applied to every
`PutObject` call (both individual file uploads and recursive folder uploads).

| Value (JSON string) | Description |
|---|---|
| `"STANDARD"` | Frequent access, highest cost |
| `"STANDARD_IA"` | **Default.** Infrequent access — good for backups/archives |
| `"ONEZONE_IA"` | Single AZ, cheaper, lower durability |
| `"INTELLIGENT_TIERING"` | Auto-moves between tiers based on access patterns |
| `"GLACIER"` | Archive, minutes-to-hours retrieval |
| `"GLACIER_IR"` | Glacier Instant Retrieval — millisecond access |
| `"DEEP_ARCHIVE"` | Cheapest, 12-hour retrieval |

---

## 10. Application Configuration (config.json)

The app writes a JSON config file on first run. Edit it manually; changes take effect
on next launch.

### File location

| Platform | Path |
|---|---|
| Linux | `~/.config/aws-s3-explorer/config.json` |
| Windows | `%APPDATA%\aws-s3-explorer\config.json` (e.g. `C:\Users\Yuri\AppData\Roaming\...`) |

### Full config.json with all fields and valid values

```json
{
  "upload_storage_class": "STANDARD_IA",
  "confirm_before_delete": true,
  "max_completed_transfers_shown": 500
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `upload_storage_class` | string | `"STANDARD_IA"` | S3 storage class for all uploads. See Section 9 for valid values. |
| `confirm_before_delete` | boolean | `true` | Show confirmation dialog before deleting files or S3 objects. Set to `false` to skip dialogs. |
| `max_completed_transfers_shown` | integer | `500` | Maximum Done/Skipped rows kept in the transfer panel. Oldest are pruned automatically. |

### Parse error behaviour

If the config file is corrupt or has invalid JSON, the app logs a warning and continues
with all defaults. The bad file is **not overwritten**. Fix it manually.

### Session persistence (separate from config.json)

eframe automatically persists window state between sessions (last-browsed local directory,
last-browsed S3 location, sync options). This is stored by eframe's own persistence
mechanism, separate from `config.json`:

| Platform | Path |
|---|---|
| Linux | `~/.local/share/aws-s3-explorer/` |
| Windows | `%APPDATA%\aws-s3-explorer\` |

---

## 11. How the Application Works — User Perspective

### First launch

1. App opens a 1280×800 window.
2. The local pane shows your home directory.
3. The S3 pane shows "Loading…" then your bucket list.
4. Status bar shows "Ready".
5. `config.json` is created with defaults if it doesn't exist.

### Navigating the local pane

- **Double-click** a folder → navigate into it.
- **Click `[..]`** → go up one level.
- **Click `⟳`** → refresh current folder from disk.
- **Single-click** → select/deselect a file or folder.
- **Right-click a file** → "Copy to S3" (uploads to current S3 prefix), "Delete".
- **Right-click a folder** → "Upload folder to S3 →" (recursive), or hint to select a bucket first.

### Navigating the S3 pane

- **Click a bucket** (in bucket list view) → navigate into it.
- **Double-click a folder** (common prefix) → navigate deeper.
- **Click `[..]`** → go up one level (or back to bucket list if at bucket root).
- **Click `⟳`** → refresh current S3 listing.
- **Single-click** → select/deselect an object.
- **Right-click a file** → "Download", "Delete".
- **Right-click a folder** → "Download folder to local ↓" (recursive).
- Address bar shows current location: `bucket-name/prefix/path/`.

### Uploading files

1. Navigate the local pane to the folder containing the files.
2. Click files to select them (highlighted).
3. Click **↑ Upload** in the toolbar.
4. Selected files appear in the transfer panel as Queued → In Progress → Done.

### Uploading a folder recursively

1. Navigate the S3 pane to the destination prefix.
2. Right-click the folder in the local pane → "Upload folder to S3 →".
3. The app scans the folder tree, reports how many files were found in the status bar,
   and enqueues them all.
4. The S3 key includes the folder name:
   `{current_s3_prefix}{folder_name}/{relative_path}`
5. After upload, click **⟳** in the S3 pane to see the new prefix appear.

### Downloading a folder recursively

1. Navigate the S3 pane to the prefix containing the folder.
2. Navigate the local pane to the destination directory.
3. Right-click the folder (common prefix) in the S3 pane → "Download folder to local ↓".
4. The app lists all objects under that prefix (no delimiter — gets everything at all depths).
5. Local sub-directories are created automatically.
6. Status bar shows how many objects were queued.

### Syncing a directory

1. Navigate both panes to the directories you want to compare.
2. Click **⇄ Sync →** (local → S3) or **⇄ Sync ←** (S3 → local).
3. A dialog shows the plan: files to copy, files to delete (if delete_extra is on),
   files already up to date.
4. Click **Execute** to run, **Dry Run Only** to see the plan without acting, or **Cancel**.

> **Note:** Sync operates on the **currently visible single level** only (Phase 1).
> For full directory tree mirroring, use "Upload folder to S3 →" or "Download folder to local ↓".

### Transfer panel

- Every transfer job appears as a row: description, size, status.
- Status is colour-coded: Gray=Queued, Yellow=In Progress, Green=Done, Red=Failed.
- Click **Clear Completed** to remove Done and Skipped rows.
- Failed jobs show the error message in the status column.

---

## 12. How the Application Works — Internal Architecture

### Startup sequence (`src/main.rs`)

```
main()
  ├── tracing_subscriber::fmt() init (reads RUST_LOG)
  ├── AppConfig::load_or_create()  → reads/creates config.json
  ├── tokio::runtime::Builder::new_multi_thread()
  │    .worker_threads(4).enable_all().build()
  ├── runtime.block_on(s3::client::build_client())
  │    → aws_config::load_from_env().await
  │    → aws_sdk_s3::Client::new(&sdk_config)
  └── eframe::run_native("aws-s3-explorer", options,
         Box::new(|cc| Ok(Box::new(S3ExplorerApp::new(cc, handle, client, config)))))
```

### App construction (`src/app.rs — App::new`)

```
S3ExplorerApp::new(cc, tokio_handle, s3_client, config)
  ├── Restore AppSettings from eframe persistence (last paths, sync options)
  ├── Create (msg_tx, msg_rx) = flume::unbounded::<AppMsg>()
  ├── Create (job_tx, job_rx) = flume::unbounded::<TransferJob>()
  ├── Clone cc.egui_ctx → self.egui_ctx  (for request_repaint in async tasks)
  ├── Spawn transfer worker task (captures job_rx, msg_tx, s3_client, egui_ctx)
  ├── load_local_directory(last_local_path or home_dir)
  ├── load_buckets()
  └── load_s3_prefix(last_s3_location)  if bucket non-empty
```

### Render loop (`App::ui`)

Called ~60 fps or on input events:

```
App::ui(ui, frame)
  ├── while let Ok(msg) = self.msg_rx.try_recv() { self.apply_message(msg); }
  │    ← drain ALL pending messages before drawing (may be several per frame)
  ├── draw_toolbar(ui)          ← Panel::top
  ├── draw_main_panels(ui)      ← Panel::bottom + Panel::left + CentralPanel
  └── modal dialogs if show_sync_dialog || show_delete_confirm
```

### Message passing pattern

Every background task follows this pattern:

```rust
let tx  = self.msg_tx.clone();   // cheap clone — flume Sender is Arc inside
let ctx = self.egui_ctx.clone(); // cheap clone — egui Context is Arc inside
let s3  = self.s3_client.clone();// cheap clone — AWS Client is Arc inside

self.tokio_handle.spawn(async move {
    let result = /* ... async work ... */;
    tx.send(AppMsg::SomeVariant(result)).ok();
    ctx.request_repaint();  // ← MANDATORY: wakes the render loop
});
```

`request_repaint()` is essential. Without it, the UI sleeps until the next user input
event, meaning results would not appear until the user moves the mouse.

### `apply_message` — state mutations

```rust
fn apply_message(&mut self, msg: AppMsg) {
    match msg {
        AppMsg::LocalListingDone { path, entries } => {
            // Guard: only apply if this response matches the CURRENT path.
            // Stale responses from superseded navigations are silently dropped.
            if path == self.local_path {
                self.local_entries = entries;
                self.local_loading = false;
            }
        }
        // ... other variants
    }
}
```

The stale-response guard (`if path == self.local_path`) is critical. Without it, rapid
navigation would show results from the wrong directory.

Both `load_local_directory` and `load_s3_prefix` update `self.local_path` / `self.s3_location`
**before** spawning the task, so the guard works correctly even if the user navigates again
before the response arrives.

### S3 browsing — delimiter-based virtual folders

`list_prefix` calls `list_objects_v2` **with** `.delimiter("/")`. AWS returns:
- `common_prefixes`: virtual sub-folders (e.g., `"photos/2023/Italy/"`)
- `contents`: real objects at this exact level

Without the delimiter, all objects at all depths would be returned flat — the S3 pane would
show every file in every sub-folder simultaneously instead of navigable folders.

`list_all_objects_recursive` calls `list_objects_v2` **without** a delimiter, getting every
object at every depth as a flat list — used for folder downloads.

### Transfer worker — sequential execution

```rust
tokio_handle.spawn(async move {
    while let Ok(job) = job_rx.recv_async().await {
        msg_tx.send(AppMsg::TransferStarted(job.id)).ok();
        ctx.request_repaint();

        let result = execute_transfer(&s3, &job, storage_class).await;

        match result {
            Ok(()) => msg_tx.send(AppMsg::TransferDone(job.id)).ok(),
            Err(e) => msg_tx.send(AppMsg::TransferFailed { id: job.id, error: e.to_string() }).ok(),
        }
        ctx.request_repaint();
    }
    // Loop ends only when job_tx is dropped (app shutdown).
});
```

`execute_transfer` matches on `TransferKind` and calls the appropriate S3 or filesystem
function. All transfers — uploads, downloads, deletes — go through this single sequential
worker. This prevents concurrent uploads from overwhelming bandwidth or confusing S3.

---

## 13. Module Reference

### `src/types.rs`

The **only** source of shared domain types. No imports from other crate modules.
Every other module may import from here safely.

Key types:
- `LocalEntry` — one row in the local pane
- `S3Entry` — one row in the S3 pane
- `S3Location { bucket, prefix }` — current S3 navigation state
- `TransferJob { id, kind, size_bytes, status }` — one transfer queue row
- `TransferKind` — Upload / Download / DeleteRemote / DeleteLocal
- `TransferStatus` — Queued / InProgress / Done / Failed(String) / Skipped
- `AppMsg` — all messages sent from tokio tasks to the render loop
- `SyncPlan` — output of the sync engine
- `SyncOptions` — delete_extra, dry_run
- `UploadStorageClass` — enum with serde `SCREAMING_SNAKE_CASE` serialisation
- `AppSettings` — eframe-persisted session state (last paths, sync options)

### `src/config.rs`

`AppConfig` struct with `load_or_create()` and `save()`. Handles platform path resolution
via `dirs::config_dir()`. Parse errors fall back to defaults and log a warning (the bad
file is NOT overwritten).

### `src/main.rs`

Entry point only. No business logic. Sets up tracing, builds the tokio runtime manually,
resolves AWS credentials via `block_on(build_client())`, configures the eframe window
(1280×800, min 800×500), and calls `run_native()`.

### `src/app.rs`

The largest file. Contains:
- `S3ExplorerApp` struct with all runtime state
- `impl eframe::App` (`fn ui` entry point, `fn save` for persistence)
- `load_local_directory`, `load_buckets`, `load_s3_prefix` — spawn listing tasks
- `start_folder_upload`, `start_folder_download` — spawn recursive operation tasks
- `enqueue_transfer` — push to both `transfer_jobs` (display) and `job_tx` (worker)
- `start_sync` — spawn sync plan computation
- `execute_sync_plan` — enqueue all plan jobs
- `apply_message` — the single match on `AppMsg` that mutates app state
- `draw_toolbar`, `draw_main_panels`, `draw_sync_dialog`, etc. — delegate to ui modules
- `execute_transfer` (private async fn) — match on `TransferKind`, call s3/fs functions

### `src/s3/client.rs`

All S3 I/O. Notable:
- `build_client()` — resolves credentials via `aws_config::load_from_env()`
- `list_prefix()` — **with** delimiter; uses paginator for >1000 object buckets
- `list_all_objects_recursive()` — **without** delimiter; returns everything flat
- `upload_file()` — reads entire file into memory, calls `PutObject`
- `download_object()` — calls `GetObject`, calls `create_dir_all(parent)`, writes file
- `From<UploadStorageClass> for aws_sdk_s3::types::StorageClass` — type conversion

### `src/fs/local.rs`

Filesystem I/O. All functions use `tokio::task::spawn_blocking` because `std::fs` is
synchronous.

- `list_directory()` — one level, sorted dirs-first
- `collect_files_recursive()` — uses `walkdir`; skips inaccessible entries gracefully

### `src/sync/engine.rs`

Pure functions, no I/O, fully unit-tested.

- `compute_plan_local_to_s3()` — generates Upload and DeleteRemote jobs
- `compute_plan_s3_to_local()` — generates Download and DeleteLocal jobs
- `entries_match()` — private helper: size equal AND mtime within ±2 seconds

### `src/ui/`

Each UI file has a public `draw(app: &mut S3ExplorerApp, ui: &mut egui::Ui)` function.

**Pattern used throughout:** Since the egui `TableBuilder` closure borrows `ui` mutably,
state needed inside the closure is **snapshotted** before the table and **actions are
accumulated** in local variables. These are processed **after** the table returns.

```rust
let entries = app.local_entries.clone(); // snapshot
let mut nav_path: Option<PathBuf> = None; // accumulator

TableBuilder::new(ui).body(|body| {
    // ... uses entries, sets nav_path
});

// Apply actions AFTER table is done (no borrow conflict):
if let Some(path) = nav_path { app.load_local_directory(&path); }
```

---

## 14. Code Standards

### Rust edition and minimum version

```toml
edition = "2024"
rust-version = "1.96"
```

### Zero warnings policy

All code must compile clean under:

```bash
cargo clippy -- -D warnings -W clippy::pedantic
```

Common pedantic lints actively enforced:
- `#[must_use]` on all pure functions returning non-trivial values
- `# Errors` doc section on all `async fn` returning `Result`
- `is_some_and()` instead of `map_or(false, ...)` (Rust 1.70+)
- `let...else` instead of `match { Ok(x) => x, Err(_) => continue }`
- Inline format args: `format!("{x}")` not `format!("{}", x)`
- `clone_from` / `clone_into` instead of `= value.clone()`
- No redundant closures: `TransferJob::description` not `|j| j.description()`
- `map_or` instead of `map(...).unwrap_or(...)`

### Error handling

```rust
// Production code: always use ?
let val = something().await?;

// Tests and examples only:
.expect("message")

// Never in production:
.unwrap()
```

All S3 and filesystem functions return `anyhow::Result<T>`.

### Formatting

```bash
cargo fmt   # default settings, no rustfmt.toml overrides
```

### Import grouping (rustfmt handles this automatically)

```rust
// 1. std
use std::path::PathBuf;

// 2. External crates
use anyhow::Result;

// 3. Internal crate (crate::)
use crate::types::AppMsg;
```

### No `unsafe` code

No `unsafe` blocks anywhere in the codebase. The `#![forbid(unsafe_code)]` attribute is
not added (this is a binary crate) but is enforced by convention and clippy.

---

## 15. Testing

### Unit tests (sync engine)

```bash
cargo test
```

10 unit tests in `src/sync/engine.rs`, all in `#[cfg(test)]`. They cover:

| Test | Scenario |
|---|---|
| `local_to_s3_new_file_goes_to_transfer` | File in source, not in dest → Upload |
| `local_to_s3_matching_file_is_skipped` | Same size + mtime → already_current |
| `local_to_s3_size_mismatch_triggers_upload` | Different size → Upload |
| `local_to_s3_extra_at_dest_ignored_when_delete_false` | Extra at dest, delete=false → ignored |
| `local_to_s3_extra_at_dest_deleted_when_delete_true` | Extra at dest, delete=true → DeleteRemote |
| `local_to_s3_timestamp_within_2s_tolerance_is_current` | mtime diff = 2s → already_current |
| `local_to_s3_timestamp_beyond_2s_triggers_upload` | mtime diff = 3s → Upload |
| `s3_to_local_new_object_goes_to_transfer` | Object in S3, not local → Download |
| `s3_to_local_matching_object_is_skipped` | Same size + mtime → already_current |
| `s3_to_local_extra_local_deleted_when_delete_true` | Extra local, delete=true → DeleteLocal |

### Integration / UI tests

There are no automated integration tests (they would require real AWS credentials and
produce real S3 costs). All S3 functionality is tested manually.

### Manual testing checklist (release mode)

```bash
cargo run --release
```

- [ ] App opens; local pane shows home directory; S3 pane lists buckets
- [ ] Navigate local pane: double-click folders, `[..]` up, `⟳` refresh
- [ ] Navigate S3 pane: click bucket, double-click prefix, `[..]` up, `⟳` refresh
- [ ] Select local file → Upload → appears in transfer panel → Done
- [ ] Select S3 object → Download → appears in local pane after refresh
- [ ] Right-click local file → "Copy to S3" → queued and done
- [ ] Right-click local folder → "Upload folder to S3 →" → status shows dest prefix → files appear in S3 after ⟳
- [ ] Right-click S3 folder → "Download folder to local ↓" → local directory tree recreated
- [ ] Choose Folder button → OS dialog opens → local pane updates to new path
- [ ] Sync → → plan dialog shows → Execute → transfer panel shows all jobs
- [ ] Delete local file with confirmation → file gone → local pane refreshes
- [ ] Delete S3 object with confirmation → object gone → S3 pane refreshes
- [ ] config.json created at correct platform path on first run
- [ ] Last-browsed paths restored on re-launch

---

## 16. Key Design Decisions and Rationale

### Why immediate-mode GUI (egui) instead of retained-mode (GTK, Qt, etc.)

Immediate-mode GUIs (egui, Dear ImGui) are simpler to integrate with async Rust:
- No widget tree to keep synchronised with background state
- State is read directly from `App` fields every frame
- No callbacks or signals — just `if button.clicked() { ... }`
- Cross-platform without platform-specific code

Trade-off: higher CPU usage (redraw every frame), and debug builds are unusably slow
(see Section 8). Release builds are fast enough for a file manager.

### Why `flume` instead of `std::sync::mpsc` or `tokio::sync::mpsc`

- `std::sync::mpsc` is single-producer; `flume` is MPMC (needed — multiple tasks write)
- `tokio::sync::mpsc::Receiver::try_recv()` only works in async context; the render loop
  is synchronous
- `flume::Receiver::try_recv()` works in both sync and async contexts

### Why manual tokio runtime instead of `#[tokio::main]`

`eframe::run_native()` never returns. `#[tokio::main]` wraps `main()` in `block_on()`,
which would conflict with eframe taking over the thread. Building the runtime manually
gives us the `Handle` before eframe takes over.

### Why sequential transfer execution (one job at a time)

- Prevents saturating upload bandwidth with parallel transfers
- Makes the transfer panel display predictable (no interleaving)
- Simplifies error handling (each job succeeds or fails independently)
- Future: could add a configurable concurrency limit (e.g., 3 parallel uploads)

### Why `walkdir` for folder traversal

`std::fs::read_dir` is non-recursive. A manual recursive implementation with
`tokio::task::spawn_blocking` is possible but fragile around symlinks, permission errors,
and very deep trees. `walkdir` handles all these cases correctly in ~3 lines.

### Why `dirs` crate for config path

Avoids `#[cfg(target_os = "linux")]` / `#[cfg(target_os = "windows")]` guards scattered
throughout the code. `dirs::config_dir()` returns the correct platform path.

---

## 17. Known Limitations (Phase 1)

| Limitation | Impact | Phase 2 plan |
|---|---|---|
| Sync is single-level only | Sync button compares only the currently visible directory level | Recursive sync option |
| No multipart upload | Files >5 GB cannot be uploaded (S3 PutObject limit) | Multipart via `aws_sdk_s3::primitives::ByteStream` |
| No byte-level progress | Transfer panel shows file-level status only | Progress bar with `Content-Length` |
| No S3 bucket operations | Cannot create or delete buckets | Bucket management panel |
| No drag-and-drop | Must use buttons and right-click menus | `egui` DnD support |
| No runtime profile switching | Must restart to change AWS profile | Profile selector UI |
| No S3 versioning | Only latest version visible | Version selector |
| No inline image preview | No thumbnails in file listing | Optional thumbnail column |

---

## 18. Phase 2 Planned Features

In priority order:

1. **Recursive Sync** — apply the sync engine recursively across all sub-directories,
   not just the current level. Required for true folder mirroring.

2. **Multipart Upload** — split large files (>100 MB) into parts, upload in parallel,
   then `CompleteMultipartUpload`. Required for files >5 GB.

3. **Transfer concurrency control** — configurable number of parallel transfer jobs
   (default 1 = sequential, max configurable in config.json).

4. **Byte-level progress** — wrap download/upload streams to track bytes transferred
   and report progress to the UI.

5. **Drag-and-drop** — drag files from local pane to S3 pane and vice versa.

---

## 19. Troubleshooting

### App opens but S3 pane stays empty / shows error

- Check `~/.aws/credentials` and `~/.aws/config` exist and have valid entries.
- Test with: `aws s3 ls` — if this fails, the app will also fail.
- Check `RUST_LOG=aws_s3_explorer=debug,warn` output for credential errors.

### Folder picker ("Choose Folder") does not respond

- **Ubuntu 24.04 GNOME:** Should work without any flags. If not, try:
  ```bash
  GDK_BACKEND=x11 ./target/release/aws-s3-explorer
  ```
- **Windows:** The rfd dialog is synchronous and briefly freezes the window while open.
  This is expected for a modal OS dialog.

### Upload reports "Access Denied (OS error 5)" or EISDIR

- You right-clicked a **folder** and tried to upload it as a file. Use right-click →
  "Upload folder to S3 →" instead. The single-file upload path (`PutObject`) cannot
  read a directory.

### Uploaded folder — files not visible in S3 pane

- The S3 pane does not auto-refresh after uploads complete. Click **⟳** to refresh.
- The uploaded folder appears as a **new prefix** (virtual folder) at the current S3
  location. Double-click it to navigate in.
- Check the status bar for the exact S3 destination prefix that was used.

### App is very slow / laggy

- You are running in **debug mode**. Always use `cargo run --release` or
  `./target/release/aws-s3-explorer`.

### GPU / rendering errors on Linux

```bash
WGPU_BACKEND=gl ./target/release/aws-s3-explorer
```

### "Failed to open display" error

The app requires a running desktop session (X11 or Wayland). Run it directly on the
machine, not over headless SSH. For SSH with display: `ssh -X user@host`.

### Transfer shows "Failed" in red

- Hover over or check the status text for the specific error.
- Common causes: S3 permission denied, network timeout, local disk full, path not found.
- The app continues with the next job — one failure does not stop the queue.

---

## 20. Git Conventions

### Commit message format (Conventional Commits)

```
<type>: <short description>

<optional body>

Co-Authored-By: Claude Sonnet 4.6 (1M context) <noreply@anthropic.com>
```

Types:
- `feat:` — new feature or user-visible improvement
- `fix:` — bug fix
- `refactor:` — code change with no behaviour change
- `docs:` — documentation only
- `chore:` — build, CI, dependency updates
- `test:` — adding or fixing tests

### Before committing

```bash
cargo fmt
cargo clippy -- -D warnings -W clippy::pedantic
cargo test
```

All three must pass with zero errors/warnings.

### Branch strategy

`main` is the primary branch. For significant features, use a feature branch:
```bash
git checkout -b feat/multipart-upload
# ... develop ...
git push origin feat/multipart-upload
# create PR via GitHub
```

### GitHub repository

`https://github.com/yuribudilov/aws-s3-explorer.git`
