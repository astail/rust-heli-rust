#!/usr/bin/env bash
# wasm をビルドして dist/ に公開用ファイルを揃える。
#   ./build.sh          リリースビルド
#   ./build.sh --dev    デバッグビルド (速い)
set -euo pipefail
cd "$(dirname "$0")"
profile=release
flag=--release
if [[ "${1:-}" == "--dev" ]]; then profile=debug; flag=; fi
cargo build $flag --target wasm32-unknown-unknown
rm -rf dist
mkdir -p dist
wasm-bindgen --target web --no-typescript --out-dir dist/pkg \
  "target/wasm32-unknown-unknown/$profile/rust_heli_rust.wasm"
cp index.html dist/
echo "dist/ ready ($(du -sh dist/pkg/*.wasm | cut -f1) wasm)"
