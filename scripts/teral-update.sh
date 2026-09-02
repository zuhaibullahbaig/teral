#!/usr/bin/env bash
# Update an installed Teral to the newest published GitHub release.
set -euo pipefail

REPOSITORY="https://github.com/zuhaibullahbaig/teral"
LATEST_URL="$REPOSITORY/releases/latest"

fail() {
  echo "teral-update: $*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || fail "'$1' is required"
}

privileged() {
  if [[ "$EUID" -eq 0 ]]; then
    "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo "$@"
  else
    fail "administrator access is required; install sudo or run this command as root"
  fi
}

need curl
need sha256sum
need sort

TERAL_PATH="$(command -v teral || true)"
[[ -n "$TERAL_PATH" ]] || fail "Teral is not installed on PATH"
TERAL_PATH="$(readlink -f "$TERAL_PATH")"

CURRENT_VERSION="$(teral --version 2>/dev/null | awk '$1 == "teral" { print $2; exit }')"
[[ -n "$CURRENT_VERSION" ]] || fail "the installed Teral does not report its version"

EFFECTIVE_URL="$(curl -fsSL -o /dev/null -w '%{url_effective}' "$LATEST_URL")"
LATEST_TAG="${EFFECTIVE_URL%/}"
LATEST_TAG="${LATEST_TAG##*/}"
[[ "$LATEST_TAG" == v* ]] || fail "GitHub did not return a release tag"
LATEST_VERSION="${LATEST_TAG#v}"

if [[ "$CURRENT_VERSION" == "$LATEST_VERSION" ]]; then
  echo "Teral $CURRENT_VERSION is already up to date."
  exit 0
fi

NEWEST="$(printf '%s\n%s\n' "$CURRENT_VERSION" "$LATEST_VERSION" | sort -V | tail -n 1)"
if [[ "$NEWEST" == "$CURRENT_VERSION" ]]; then
  fail "installed version $CURRENT_VERSION is newer than published version $LATEST_VERSION"
fi

if command -v pgrep >/dev/null 2>&1 && pgrep -x teral >/dev/null 2>&1; then
  fail "close Teral before updating it"
fi

if command -v pacman >/dev/null 2>&1 && pacman -Q teral >/dev/null 2>&1; then
  if command -v paru >/dev/null 2>&1; then
    paru -S --needed teral
  elif command -v yay >/dev/null 2>&1; then
    yay -S --needed teral
  elif pacman -Si teral >/dev/null 2>&1; then
    privileged pacman -S --needed teral
  else
    fail "this package is managed by pacman; update it with your AUR helper"
  fi
  exit 0
fi

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) DEB_ARCH="amd64" ;;
  aarch64) DEB_ARCH="arm64" ;;
  *) fail "release binaries are not published for $ARCH" ;;
esac

TEMP_DIR="$(mktemp -d)"
trap 'rm -rf -- "$TEMP_DIR"' EXIT
BASE_URL="$REPOSITORY/releases/download/$LATEST_TAG"
curl -fsSL "$BASE_URL/SHA256SUMS" -o "$TEMP_DIR/SHA256SUMS"

download_and_verify() {
  local asset="$1"
  local checksum
  checksum="$(awk -v asset="$asset" '$2 == asset { print; exit }' "$TEMP_DIR/SHA256SUMS")"
  [[ -n "$checksum" ]] || fail "$asset is missing from SHA256SUMS"
  curl -fL --progress-bar "$BASE_URL/$asset" -o "$TEMP_DIR/$asset"
  (cd "$TEMP_DIR" && printf '%s\n' "$checksum" | sha256sum -c -)
}

if command -v dpkg-query >/dev/null 2>&1 \
  && dpkg-query -W -f='${Status}' teral 2>/dev/null | grep -q 'install ok installed'; then
  need apt
  ASSET="teral_${LATEST_VERSION}_${DEB_ARCH}.deb"
  download_and_verify "$ASSET"
  privileged apt install "$TEMP_DIR/$ASSET"
  echo "Updated Teral to $LATEST_VERSION."
  exit 0
fi

need tar
ASSET="teral-${LATEST_VERSION}-${ARCH}-linux.tar.gz"
download_and_verify "$ASSET"
tar -xzf "$TEMP_DIR/$ASSET" -C "$TEMP_DIR"
INSTALLER="$TEMP_DIR/teral-$LATEST_VERSION/scripts/install.sh"
[[ -x "$INSTALLER" ]] || fail "the release archive does not contain its installer"

case "$TERAL_PATH" in
  */bin/teral) PREFIX="${TERAL_PATH%/bin/teral}" ;;
  *) fail "cannot determine the installation prefix from $TERAL_PATH" ;;
esac

if [[ -w "$PREFIX/bin" ]]; then
  PREFIX="$PREFIX" "$INSTALLER"
else
  privileged env PREFIX="$PREFIX" "$INSTALLER"
fi

echo "Updated Teral to $LATEST_VERSION."
