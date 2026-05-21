#!/usr/bin/env bash
set -euo pipefail

if ! command -v wasm-pack &> /dev/null; then
  echo "Installing wasm-pack..."
  curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
fi

cd kernel && wasm-pack build --target web --release --out-dir ../pkg
