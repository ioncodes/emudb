#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

export RUST_LOG="${RUST_LOG:-info}"
export WEBHOOK_SECRET="${WEBHOOK_SECRET:-devsecret}"

echo "WEBHOOK_SECRET=$WEBHOOK_SECRET"
cargo run -- --config "${1:-config.toml}"
