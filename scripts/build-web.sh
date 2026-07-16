#!/usr/bin/env bash
# ABOUTME: Builds the browser adapter and generated JavaScript glue for the static demo.
# ABOUTME: The resulting web/pkg directory can be served by any static HTTP server.

set -euo pipefail

repository_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v wasm-pack >/dev/null 2>&1; then
    echo "fenrin: wasm-pack is required (https://rustwasm.github.io/wasm-pack/installer/)" >&2
    exit 1
fi

cd "$repository_root"
CARGO_TARGET_DIR="$repository_root/target/web-wasm" \
    wasm-pack build web-wasm --target web --release --out-dir ../web/pkg -- --locked
