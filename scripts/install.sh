#!/usr/bin/env bash
# Install Teral: the binary, its desktop entry and its icon.
#
#   ./scripts/install.sh                 # build and install into /usr/local (needs sudo)
#   PREFIX=~/.local ./scripts/install.sh # install for one user, no root needed
#   ./scripts/install.sh --uninstall     # remove what was installed
#
# DESTDIR is honoured so packagers can stage an install into a build root.
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
DESTDIR="${DESTDIR:-}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

BIN_DIR="$DESTDIR$PREFIX/bin"
DESKTOP_DIR="$DESTDIR$PREFIX/share/applications"
ICON_DIR="$DESTDIR$PREFIX/share/icons/hicolor/scalable/apps"
LICENSE_DIR="$DESTDIR$PREFIX/share/licenses/teral"

APP_ID="dev.zuhaibullahbaig.Teral"

uninstall() {
  rm -f "$BIN_DIR/teral" \
        "$DESKTOP_DIR/$APP_ID.desktop" \
        "$ICON_DIR/$APP_ID.svg" \
        "$LICENSE_DIR/LICENSE"
  rmdir "$LICENSE_DIR" 2>/dev/null || true
  echo "Removed Teral from $PREFIX."
  refresh_caches
}

refresh_caches() {
  # Both are optional: a desktop without them simply picks the changes up later.
  command -v update-desktop-database >/dev/null 2>&1 &&
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 &&
    gtk-update-icon-cache -qtf "$DESTDIR$PREFIX/share/icons/hicolor" 2>/dev/null || true
}

if [[ "${1:-}" == "--uninstall" ]]; then
  uninstall
  exit 0
fi

BINARY="${TERAL_BINARY:-$ROOT/target/release/teral}"
if [[ ! -x "$BINARY" ]]; then
  echo "Building Teral in release mode…"
  (cd "$ROOT" && cargo build --release)
  BINARY="$ROOT/target/release/teral"
fi

install -Dm755 "$BINARY" "$BIN_DIR/teral"
install -Dm644 "$ROOT/packaging/$APP_ID.desktop" "$DESKTOP_DIR/$APP_ID.desktop"
install -Dm644 "$ROOT/packaging/$APP_ID.svg" "$ICON_DIR/$APP_ID.svg"
install -Dm644 "$ROOT/LICENSE" "$LICENSE_DIR/LICENSE"

refresh_caches

echo "Installed Teral into $PREFIX."
if [[ ":$PATH:" != *":$PREFIX/bin:"* ]]; then
  echo "Note: $PREFIX/bin is not on your PATH."
fi
