# Cross-Platform Application Icons — Design

**Status:** Approved for planning (implementation not yet started)
**Date:** 2026-07-06

## Problem

The project has icon assets checked in (`assets/icon.png`, `assets/icon.ico`,
`assets/icon.icns`, `assets/icons/{16,32,48,64,128,256,512,1024}.png`) but
nothing in the codebase or build configuration uses them. The app currently
runs with no window icon, and no build step produces a Windows executable
with an embedded icon, a macOS `.app` bundle, or a Linux desktop entry.

"Embedding icons" on a cross-platform native Rust GUI app is actually two
distinct concerns that use unrelated mechanisms per OS:

1. **Runtime window icon** — the icon shown in the title bar, taskbar/dock,
   and alt-tab switcher while the app is running. Pure Rust, identical
   mechanism on all three OSes.
2. **OS-level executable/bundle icon** — what Windows Explorer shows for the
   `.exe`, what Finder shows for a macOS `.app`, and what a Linux desktop
   launcher shows. Each OS has a completely different mechanism for this,
   and none of it is expressible as a single cross-platform Cargo.toml field.

This spec covers both, scoped per platform.

## Confirmed scope decisions

| Question | Decision |
|---|---|
| Runtime window icon vs. OS-level packaging icon | Both. |
| Existing release/packaging process | None — only `cargo run`/`cargo build` today. This is the first packaging-adjacent work in the project. |
| Where Windows/macOS builds happen | Natively on each OS (or a same-OS CI runner) — no cross-compilation toolchain concerns for `build.rs`/`cargo-bundle`. |
| Windows icon-embedding crate | `winresource` (actively maintained fork of the dead `winres`). No `.rc` file authoring needed. |
| macOS bundler | `cargo-bundle` (verified actively maintained via direct commit history — some secondary sources claiming it's dead and recommending a Zed fork are outdated). `cargo-packager` was considered but is Tauri-ecosystem scoped (installers, auto-updater) — more than this project needs. |
| Linux desktop icon | Absolute path to `assets/icon.png` in the `.desktop` file's `Icon=` field, not a full XDG hicolor theme install. Simpler, no install-time resizing/theme-cache step, acceptable tradeoff for a personal tool. |

## Design

### 1. Runtime window icon (all platforms)

`eframe` 0.35 already vendors the `image` crate specifically for this
purpose (confirmed in `eframe`'s own `Cargo.toml`, commented "Needed for app
icon") and ships `eframe::icon_data::from_png_bytes(bytes: &[u8]) ->
Result<egui::IconData, image::ImageError>`. No new dependency is needed.

In `src/main.rs`, where `eframe::NativeOptions` is currently constructed:

```rust
let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
    .expect("valid embedded icon PNG");

let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default().with_icon(icon),
    ..Default::default() // merged with whatever options already exist
};
```

`include_bytes!` embeds the 512px PNG into the binary at compile time;
`from_png_bytes` decodes it to raw RGBA once at startup. The icon is set
once via `ViewportBuilder` rather than changed at runtime — `egui`'s
`ViewportCommand::Icon` runtime-swap path has known reliability issues on
macOS/Windows, and this app never needs to change its icon after launch.

### 2. Windows `.exe` icon

New `build.rs` at the project root:

```rust
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("failed to embed Windows icon resource");
    }
}
```

`Cargo.toml` adds `winresource` as a **Windows-only** build-dependency, so
it is never even compiled on Linux/macOS:

```toml
[target.'cfg(windows)'.build-dependencies]
winresource = "0.1"
```

The existing `assets/icon.ico` already contains multiple embedded
resolutions (confirmed: 16×16 and 32×32 present at minimum), so no
additional icon generation is needed — `set_icon` embeds the file as-is.

Building on Windows requires no extra toolchain setup beyond the standard
Rust MSVC or GNU toolchain already needed to build the project there at
all.

### 3. macOS `.app` bundle

`cargo-bundle` is a separate dev-time CLI tool (`cargo install cargo-bundle`,
then `cargo bundle --release`), not a project dependency. It reads a
`[package.metadata.bundle]` section from `Cargo.toml`:

```toml
[package.metadata.bundle]
name = "AWS S3 Explorer"
identifier = "com.ebonazzi.aws-s3-explorer"
icon = ["assets/icon.icns"]
```

`identifier` is a placeholder reverse-DNS-style bundle ID — trivial to
change later, has no functional effect beyond macOS's own app identity
bookkeeping (Launch Services, preferences domain, etc.).

Pointing `icon` directly at the existing `assets/icon.icns` (rather than a
list of PNGs) tells `cargo-bundle` to use that file as-is in
`Contents/Resources/`, rather than regenerating an `.icns` from PNGs via its
own conversion path — preserving whatever fidelity is already baked into
the hand-produced `.icns`.

Running `cargo bundle --release` on macOS produces
`target/release/bundle/osx/AWS S3 Explorer.app`, a real double-clickable
bundle with `Contents/Info.plist` referencing the icon.

### 4. Linux desktop icon

Two new files under `packaging/linux/`:

**`aws-s3-explorer.desktop.in`** — a template with placeholder tokens for
the two paths that depend on where the project is checked out (there is no
fixed install prefix for this personal tool):

```ini
[Desktop Entry]
Type=Application
Name=AWS S3 Explorer
Comment=Personal AWS S3 / local file manager
Exec=__EXEC_PATH__
Icon=__ICON_PATH__
Terminal=false
Categories=Utility;FileTools;
```

**`install.sh`** — resolves the repository root, substitutes
`__EXEC_PATH__` with the absolute path to
`target/release/aws-s3-explorer` and `__ICON_PATH__` with the absolute
path to `assets/icon.png`, and writes the result to
`~/.local/share/applications/aws-s3-explorer.desktop`:

```bash
#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec_path="$repo_root/target/release/aws-s3-explorer"
icon_path="$repo_root/assets/icon.png"

if [ ! -x "$exec_path" ]; then
    echo "error: $exec_path not found — run 'cargo build --release' first" >&2
    exit 1
fi

dest_dir="$HOME/.local/share/applications"
mkdir -p "$dest_dir"

sed \
    -e "s|__EXEC_PATH__|$exec_path|" \
    -e "s|__ICON_PATH__|$icon_path|" \
    "$repo_root/packaging/linux/aws-s3-explorer.desktop.in" \
    > "$dest_dir/aws-s3-explorer.desktop"

echo "Installed $dest_dir/aws-s3-explorer.desktop"
```

Running this after `cargo build --release` makes the app appear in the
desktop environment's application launcher/menu with the correct icon,
using an absolute path (no XDG hicolor theme install, no icon-cache
refresh needed).

## Verification limits

This is a Linux dev machine. What can actually be verified here:

- **Runtime window icon**: fully verifiable — build and run the app, see
  the icon in the window/taskbar.
- **Windows build.rs config**: verifiable for correctness (compiles, the
  `cfg(windows)` gating means it's inert on this machine) but the actual
  `.exe`-with-embedded-icon can only be produced and confirmed by building
  on Windows.
- **macOS bundle config**: the `Cargo.toml` TOML is verifiable for
  correctness, but `cargo bundle --release` producing an actual `.app`
  requires running on macOS (it needs a macOS binary to bundle).
- **Linux desktop file**: fully verifiable — run `install.sh`, confirm the
  entry with correct icon appears in the application launcher.

## Out of scope

- Windows installer generation (MSI/NSIS) — not requested, `cargo-bundle`
  doesn't focus on this anyway.
- Linux `.deb`/AppImage packaging — not requested; the `.desktop` +
  absolute-path-icon approach is sufficient for a personal tool.
- XDG hicolor icon theme installation — explicitly decided against in favor
  of the simpler absolute-path approach.
- Runtime icon changes (e.g. different icon per theme/dark-mode) — the app
  only ever needs one static icon.
