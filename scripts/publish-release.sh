#!/usr/bin/env bash
# Publica en GitHub Releases los .deb de dist/ para la versión de Cargo.toml (requiere `gh` autenticado).
# Uso: scripts/publish-release.sh        (antes: scripts/build-deb.sh y git push)
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
tag="v$version"
debs=(dist/lanwatch_"${version}"_*.deb)
[ -e "${debs[0]}" ] || { echo "No hay paquetes de la versión $version en dist/ (ejecuta scripts/build-deb.sh)" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "Hay cambios sin commitear: la release no coincidiría con el código." >&2; exit 1; }
git fetch -q origin
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || { echo "HEAD no está subido a origin/main (git push primero)." >&2; exit 1; }

# Notas = entrada superior del changelog de Debian
notes=$(awk 'NR>1 && /^ -- /{exit} NR>1{print}' packaging/changelog | sed 's/^  //')
if gh release view "$tag" >/dev/null 2>&1; then
  gh release upload "$tag" "${debs[@]}" --clobber
else
  gh release create "$tag" "${debs[@]}" --target main --title "lanwatch $version" --notes "$notes"
fi
echo "Publicado $tag"
