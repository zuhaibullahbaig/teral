#!/usr/bin/env bash
set -euo pipefail

bash "$(dirname "$0")/check-system.sh"
bash "$(dirname "$0")/check-version.sh"
for script in "$(dirname "$0")"/*.sh; do
  bash -n "$script"
done
if command -v desktop-file-validate >/dev/null 2>&1; then
  desktop-file-validate packaging/dev.zuhaibullahbaig.Teral.desktop
fi
if command -v appstreamcli >/dev/null 2>&1; then
  appstreamcli validate --no-net packaging/dev.zuhaibullahbaig.Teral.metainfo.xml
fi
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
cargo build --locked
cargo build --release --locked
