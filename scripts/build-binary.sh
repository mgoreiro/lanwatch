#!/usr/bin/env bash
# Compila un binario estático (musl) para Linux con Docker. Uso: build-binary.sh arm64|amd64
# Salida: dist/lanwatch-<arch>
set -euo pipefail
cd "$(dirname "$0")/.."
arch="${1:?uso: build-binary.sh arm64|amd64}"
case "$arch" in arm64|amd64) ;; *) echo "arquitectura no soportada: $arch" >&2; exit 1 ;; esac
mkdir -p dist
docker run --rm --platform "linux/$arch" \
  -v "$PWD":/src -w /src \
  -v "lanwatch-cargo-$arch":/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR="/src/target-linux-$arch" \
  rust:alpine sh -c 'apk add --no-cache build-base >/dev/null && cargo build --release'
cp "target-linux-$arch/release/lanwatch" "dist/lanwatch-$arch"
echo "dist/lanwatch-$arch: $(du -h "dist/lanwatch-$arch" | cut -f1)"
