#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export VST3_SDK_DIR="${VST3_SDK_DIR:-$HOME/lib/vst3sdk}"
cargo fmt --all -- --check
cargo clippy --locked --release --all-targets -- -D warnings
cargo test --locked --release --lib
./scripts/bundle.sh
if [[ -n "${VST3_VALIDATOR:-}" ]]; then
    "$VST3_VALIDATOR" "$PWD/dist/MBX.vst3"
fi
