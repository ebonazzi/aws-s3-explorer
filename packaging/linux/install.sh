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
