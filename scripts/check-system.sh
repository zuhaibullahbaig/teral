#!/usr/bin/env bash
set -euo pipefail

if ! command -v pkg-config >/dev/null 2>&1; then
  echo "error: pkg-config is required to verify Teral's system libraries" >&2
  exit 1
fi

if ! pkg-config --atleast-version=4.12 gtk4; then
  echo "error: GTK 4.12 or newer is required" >&2
  exit 1
fi

if ! pkg-config --atleast-version=0.66 vte-2.91-gtk4; then
  echo "error: VTE for GTK4 0.66 or newer is required" >&2
  exit 1
fi

echo "GTK $(pkg-config --modversion gtk4) and VTE $(pkg-config --modversion vte-2.91-gtk4) satisfy Teral's system requirements."
