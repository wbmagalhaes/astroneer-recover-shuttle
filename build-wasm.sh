#!/usr/bin/env bash
# Build the Rust decoder/fix to WebAssembly and emit browser bindings into web/pkg/.
# Requires: rustup, the wasm target, and wasm-bindgen-cli matching the wasm-bindgen crate version.
set -euo pipefail
cd "$(dirname "$0")"

WB_VER=$(grep -A1 'name = "wasm-bindgen"' Cargo.lock | grep version | head -1 | sed -E 's/.*"(.*)".*/\1/')
echo "wasm-bindgen crate version: ${WB_VER}"

rustup target add wasm32-unknown-unknown
command -v wasm-bindgen >/dev/null || cargo install wasm-bindgen-cli --version "${WB_VER}"

cargo build --release --target wasm32-unknown-unknown --lib
wasm-bindgen --target web --no-typescript \
  --out-dir web/pkg target/wasm32-unknown-unknown/release/astro_recover.wasm

echo "Done. Serve locally with:  python3 -m http.server -d web 8000"
echo "Then open http://localhost:8000  (a static host like GitHub Pages serving web/ also works)."
