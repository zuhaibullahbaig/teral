#!/usr/bin/env bash
# Build candidate release artifacts from the current checkout:
#
#   dist/teral-<version>-x86_64-linux.tar.gz   binary + desktop entry + icon + installer
#   dist/teral_<version>_amd64.deb             Debian/Ubuntu package (needs dpkg-deb)
#
# Run it from anywhere; it always packages this checkout.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
ARCH="$(uname -m)"
APP_ID="dev.zuhaibullahbaig.Teral"
DIST="$ROOT/dist"

echo "Packaging Teral $VERSION"
cargo build --release --locked

rm -rf "$DIST"
mkdir -p "$DIST"

# ------------------------------------------------------------------ tarball ----
STAGE="$DIST/teral-$VERSION"
mkdir -p "$STAGE"
cp target/release/teral "$STAGE/"
cp README.md LICENSE "$STAGE/"
mkdir -p "$STAGE/packaging" "$STAGE/scripts"
cp "packaging/$APP_ID.desktop" "packaging/$APP_ID.svg" "$STAGE/packaging/"
cp scripts/install.sh "$STAGE/scripts/"
# The tarball installs the binary it ships rather than rebuilding it.
sed -i 's|target/release/teral|teral|' "$STAGE/scripts/install.sh"

tar -czf "$DIST/teral-$VERSION-$ARCH-linux.tar.gz" -C "$DIST" "teral-$VERSION"
rm -rf "$STAGE"

# ---------------------------------------------------------------------- deb ----
if command -v dpkg-deb >/dev/null 2>&1; then
  case "$ARCH" in
    x86_64) DEB_ARCH="amd64" ;;
    aarch64) DEB_ARCH="arm64" ;;
    *) DEB_ARCH="$ARCH" ;;
  esac

  DEB_ROOT="$DIST/deb"
  install -Dm755 target/release/teral "$DEB_ROOT/usr/bin/teral"
  install -Dm644 "packaging/$APP_ID.desktop" "$DEB_ROOT/usr/share/applications/$APP_ID.desktop"
  install -Dm644 "packaging/$APP_ID.svg" \
    "$DEB_ROOT/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
  install -Dm644 LICENSE "$DEB_ROOT/usr/share/doc/teral/copyright"

  mkdir -p "$DEB_ROOT/DEBIAN"
  cat > "$DEB_ROOT/DEBIAN/control" <<EOF
Package: teral
Version: $VERSION
Section: utils
Priority: optional
Architecture: $DEB_ARCH
Depends: libgtk-4-1 (>= 4.12), libvte-2.91-gtk4-0, libglib2.0-0
Maintainer: Zuhaib Ullah Baig <noreply@users.noreply.github.com>
Description: A modern native Linux file manager
 Teral is a fast, information-rich file manager written in Rust with GTK4.
 It adopts the appearance of the desktop it runs on, including the active
 Omarchy theme.
EOF

  dpkg-deb --build --root-owner-group "$DEB_ROOT" \
    "$DIST/teral_${VERSION}_${DEB_ARCH}.deb" >/dev/null
  rm -rf "$DEB_ROOT"
else
  echo "dpkg-deb not found: skipping the .deb"
fi

echo
ls -lh "$DIST"
