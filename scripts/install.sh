#!/usr/bin/env bash
# Install Teral: the binary, updater, desktop entry, icon, metadata and license.
#
#   sudo TERAL_BINARY="$PWD/target/release/teral" ./scripts/install.sh
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
METAINFO_DIR="$DESTDIR$PREFIX/share/metainfo"
LICENSE_DIR="$DESTDIR$PREFIX/share/licenses/teral"

APP_ID="dev.zuhaibullahbaig.Teral"

uninstall() {
  rm -f "$BIN_DIR/teral" \
        "$BIN_DIR/teral-update" \
        "$DESKTOP_DIR/$APP_ID.desktop" \
        "$ICON_DIR/$APP_ID.svg" \
        "$METAINFO_DIR/$APP_ID.metainfo.xml" \
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

case "${1:-}" in
  "") ;;
  --uninstall)
    uninstall
    exit 0
    ;;
  --help|-h)
    sed -n '2,8p' "$0" | sed 's/^# *//'
    exit 0
    ;;
  *)
    echo "usage: $0 [--uninstall]" >&2
    exit 2
    ;;
esac

if [[ -n "${TERAL_BINARY:-}" ]]; then
  BINARY="$TERAL_BINARY"
elif [[ -x "$ROOT/teral" && ! -f "$ROOT/Cargo.toml" ]]; then
  # A release tarball carries its already-checked binary beside this script.
  BINARY="$ROOT/teral"
else
  # A source checkout is rebuilt every time. Re-running the installer after a pull
  # must never silently reinstall an older binary left in target/release.
  if [[ "$EUID" -eq 0 && -n "${SUDO_USER:-}" ]]; then
    echo "error: build Teral as your normal user before installing it system-wide" >&2
    echo '       cargo build --release --locked' >&2
    echo '       sudo TERAL_BINARY="$PWD/target/release/teral" ./scripts/install.sh' >&2
    exit 1
  fi
  echo "Building Teral in release mode…"
  (cd "$ROOT" && cargo build --release --locked)
  BINARY="$ROOT/target/release/teral"
fi

if [[ ! -x "$BINARY" ]]; then
  echo "error: Teral binary is missing or not executable: $BINARY" >&2
  exit 1
fi

install -Dm755 "$BINARY" "$BIN_DIR/teral"
install -Dm755 "$ROOT/scripts/teral-update.sh" "$BIN_DIR/teral-update"
install -Dm644 "$ROOT/packaging/$APP_ID.desktop" "$DESKTOP_DIR/$APP_ID.desktop"
install -Dm644 "$ROOT/packaging/$APP_ID.svg" "$ICON_DIR/$APP_ID.svg"
install -Dm644 "$ROOT/packaging/$APP_ID.metainfo.xml" "$METAINFO_DIR/$APP_ID.metainfo.xml"
install -Dm644 "$ROOT/LICENSE" "$LICENSE_DIR/LICENSE"

refresh_caches

echo "Installed Teral into $PREFIX. Run teral-update to install future releases."
if [[ ":$PATH:" != *":$PREFIX/bin:"* ]]; then
  echo "Note: $PREFIX/bin is not on your PATH."
fi
