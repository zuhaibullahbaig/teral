#!/usr/bin/env bash
set -euo pipefail

bash "$(dirname "$0")/check-system.sh"
bash "$(dirname "$0")/check-version.sh"
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets
cargo build --locked
cargo build --release --locked
