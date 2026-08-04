#!/usr/bin/env bash
# Install the prebuilt Kindle Drop binary as a desktop app for the current user.
# Everything goes under ~/.local (no root needed). Run ./uninstall.sh to remove.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"

BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"
mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR"

echo "==> Installing binary and icon…"
install -Dm755 kindle-drop "$BIN_DIR/kindle-drop"
install -Dm644 kindle-drop.svg "$ICON_DIR/kindle-drop.svg"

echo "==> Writing desktop entry…"
# Use an absolute Exec so it launches regardless of whether ~/.local/bin is on PATH.
sed "s|^Exec=kindle-drop$|Exec=$BIN_DIR/kindle-drop|" \
    kindle-drop.desktop > "$APP_DIR/kindle-drop.desktop"

echo "==> Refreshing caches…"
update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
command -v kbuildsycoca6 >/dev/null 2>&1 && kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && \
    gtk-update-icon-cache -f "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true

echo
echo "Done. Search your launcher for \"Kindle Drop\" (or run: $BIN_DIR/kindle-drop)."
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "Tip: add $BIN_DIR to your PATH to run 'kindle-drop' from any terminal." ;;
esac
