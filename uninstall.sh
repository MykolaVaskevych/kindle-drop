#!/usr/bin/env bash
# Remove the per-user Kindle Drop desktop install.
set -euo pipefail

rm -f "$HOME/.local/bin/kindle-drop"
rm -f "$HOME/.local/share/applications/kindle-drop.desktop"
rm -f "$HOME/.local/share/icons/hicolor/scalable/apps/kindle-drop.svg"

update-desktop-database "$HOME/.local/share/applications" >/dev/null 2>&1 || true
command -v kbuildsycoca6 >/dev/null 2>&1 && kbuildsycoca6 --noincremental >/dev/null 2>&1 || true

echo "Removed Kindle Drop from ~/.local. (Config at ~/.config/kindle-drop left intact.)"
