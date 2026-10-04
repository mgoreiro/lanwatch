#!/usr/bin/env bash
# Compila un binario estático (musl) para Linux/ARM64 (Orange Pi, Raspberry Pi…) usando Docker.
# Salida: dist/lanwatch-aarch64
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p dist
docker run --rm --platform linux/arm64 \
  -v "$PWD":/src -w /src \
  -v lanwatch-cargo:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/src/target-linux \
  rust:alpine sh -c 'apk add --no-cache build-base >/dev/null && cargo build --release'
cp target-linux/release/lanwatch dist/lanwatch-aarch64
ls -lh dist/lanwatch-aarch64
