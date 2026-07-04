# aws-s3-explorer

A personal desktop GUI application for browsing and managing files between a local filesystem
and AWS S3. Loosely modelled on CloudBerry Explorer.

## Features

- Dual-pane GUI: local filesystem (left) and S3 browser (right)
- Browse local directories and S3 buckets / prefixes
- Upload / download individual files (toolbar buttons + right-click menu)
- **Recursive folder upload** — right-click any local folder → "Upload folder to S3 →"
- **Recursive folder download** — right-click any S3 prefix → "Download folder to local ↓"
- Refresh buttons on both panes (⟳)
- Delete local files and S3 objects (with confirmation dialog)
- Sync plan engine (local ↔ S3) with 2-second mtime tolerance and dry-run mode
- Transfer queue panel: every queued / in-progress / done / failed job is shown
- AWS credentials resolved from `~/.aws/credentials` / `~/.aws/config` (same chain as AWS CLI v2)
- Config-file-driven upload storage class (default `STANDARD_IA`)

## Platform Support

- Ubuntu Linux 22.04 LTS and later (native Wayland + X11)
- Windows 11

---

## Linux Quick Start (Ubuntu 22.04 / 24.04)

### 1 — Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup show      # confirm stable ≥ 1.96
```

### 2 — Install system build dependencies

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

These are required by eframe's native backend (wgpu + winit + GTK file dialog).
They are **not** needed on Windows.

### 3 — Configure AWS credentials

```bash
# Use AWS CLI (recommended):
aws configure

# Or create the files manually:
mkdir -p ~/.aws
# Then edit ~/.aws/credentials and ~/.aws/config as you would for the CLI.
```

### 4 — Clone and build

```bash
git clone https://github.com/yuribudilov/aws-s3-explorer.git
cd aws-s3-explorer
cargo build --release        # first build downloads ~300 crates — takes several minutes
```

### 5 — Run

```bash
cargo run --release
# or run the binary directly:
./target/release/aws-s3-explorer
```

> **Always use `--release`.** egui is an immediate-mode renderer. Debug builds produce
> single-digit FPS because every widget is re-evaluated every frame with zero optimisations.
> Release builds are required for any real testing.

---

## Windows 11

No additional system packages are needed beyond the Rust toolchain (stable ≥ 1.96).

```powershell
git clone https://github.com/yuribudilov/aws-s3-explorer.git
cd aws-s3-explorer
cargo run --release
```

---

## Linux Troubleshooting

### "Choose Folder" dialog does not appear (Wayland)

The folder picker uses GTK 3 via the `rfd` crate. On some Wayland compositors the GTK
dialog may not receive focus or may appear behind the app window.

**Workaround:** force the GTK dialog to use X11 (XWayland):

```bash
GDK_BACKEND=x11 ./target/release/aws-s3-explorer
```

If you are on a pure Wayland desktop without XWayland available, install XWayland:
```bash
sudo apt-get install xwayland
```

### wgpu / GPU errors on launch

eframe uses `wgpu` for rendering. If the GPU driver does not support Vulkan or OpenGL,
set the software renderer:

```bash
WGPU_BACKEND=gl ./target/release/aws-s3-explorer
```

### "Failed to open display" in SSH / headless

This is a GUI app and requires a running desktop session. Use `ssh -X` (X11 forwarding)
or run it on the console of the Ubuntu machine directly.

---

## Configuration

Created automatically on first run:

| Platform | Path |
|---|---|
| Linux | `~/.config/aws-s3-explorer/config.json` |
| Windows | `%APPDATA%\aws-s3-explorer\config.json` |

Default content:

```json
{
  "upload_storage_class": "STANDARD_IA",
  "confirm_before_delete": true,
  "max_completed_transfers_shown": 500
}
```

Valid values for `upload_storage_class`:
`"STANDARD"`, `"STANDARD_IA"`, `"ONEZONE_IA"`, `"INTELLIGENT_TIERING"`, `"GLACIER"`,
`"GLACIER_IR"`, `"DEEP_ARCHIVE"`

Changes take effect on next app restart.

## Logging

```bash
RUST_LOG=aws_s3_explorer=debug,warn ./target/release/aws-s3-explorer
```

## Phase 1 Limitations

- Single-level directory sync only (recursive sync deferred to Phase 2)
- Folder upload/download are recursive; Sync buttons compare one directory level at a time
- No byte-level progress bars (file/object-level only)
- No multipart upload (single `PutObject` for all sizes)
- No S3 bucket creation or deletion
- No drag-and-drop between panes
- No runtime AWS profile switching
