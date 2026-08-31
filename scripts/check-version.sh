#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cargo_version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
lock_version="$(awk '
  $0 == "name = \"teral\"" { in_teral = 1; next }
  in_teral && /^version = / {
    value = $0
    sub(/^version = \"/, "", value)
    sub(/\"$/, "", value)
    print value
    exit
  }
' "$ROOT/Cargo.lock")"
pkgbuild_version="$(sed -n 's/^pkgver=//p' "$ROOT/packaging/PKGBUILD" | head -1)"

if [[ -z "$cargo_version" || -z "$lock_version" || -z "$pkgbuild_version" ]]; then
  echo "Could not read Teral's version from Cargo.toml, Cargo.lock, and PKGBUILD." >&2
  exit 1
fi

if [[ "$cargo_version" != "$lock_version" || "$cargo_version" != "$pkgbuild_version" ]]; then
  echo "Teral version mismatch:" >&2
  echo "  Cargo.toml: $cargo_version" >&2
  echo "  Cargo.lock: $lock_version" >&2
  echo "  PKGBUILD:   $pkgbuild_version" >&2
  exit 1
fi

echo "Teral version $cargo_version is consistent."
