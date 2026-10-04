#!/usr/bin/env bash
# Compatibilidad: binario ARM64 en dist/lanwatch-aarch64 (ver build-binary.sh).
set -euo pipefail
cd "$(dirname "$0")/.."
./scripts/build-binary.sh arm64
cp dist/lanwatch-arm64 dist/lanwatch-aarch64
ls -lh dist/lanwatch-aarch64
