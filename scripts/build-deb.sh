#!/usr/bin/env bash
# Genera paquetes .deb (binario estático, sin dependencias) con Docker.
# Uso: scripts/build-deb.sh [arm64|amd64|all] [--lint]      Salida: dist/lanwatch_<versión>_<arch>.deb
set -euo pipefail
cd "$(dirname "$0")/.."

which_arch="${1:-all}"
lint=0
for a in "$@"; do [ "$a" = "--lint" ] && lint=1; done
[ "$which_arch" = "--lint" ] && which_arch=all
case "$which_arch" in all) archs="arm64 amd64" ;; arm64|amd64) archs="$which_arch" ;; *) echo "uso: build-deb.sh [arm64|amd64|all] [--lint]" >&2; exit 1 ;; esac

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
mkdir -p dist

for arch in $archs; do
  ./scripts/build-binary.sh "$arch"
  pkg="dist/pkg-$arch"
  rm -rf "$pkg"
  mkdir -p "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/share/doc/lanwatch" "$pkg/usr/share/man/man1"

  install -m 755 "dist/lanwatch-$arch" "$pkg/usr/bin/lanwatch"
  gzip -9n < packaging/lanwatch.1 > "$pkg/usr/share/man/man1/lanwatch.1.gz"
  gzip -9n < packaging/changelog   > "$pkg/usr/share/doc/lanwatch/changelog.gz"
  install -m 644 packaging/copyright packaging/lanwatch.conf.example README.md docs/AYUDA.md docs/EDGEROUTER.md docs/ARCHITECTURE.md "$pkg/usr/share/doc/lanwatch/"
  find "$pkg/usr/share" -type f -exec chmod 644 {} +
  mkdir -p "$pkg/usr/share/lintian/overrides"
  install -m 644 packaging/lintian-overrides "$pkg/usr/share/lintian/overrides/lanwatch"
  install -m 755 packaging/postinst "$pkg/DEBIAN/postinst"

  size_kb=$(du -sk "$pkg/usr" | cut -f1)
  cat > "$pkg/DEBIAN/control" <<CONTROL
Package: lanwatch
Version: $version
Architecture: $arch
Maintainer: mgoreiro <mgoreiro@gmail.com>
Installed-Size: $size_kb
Recommends: libcap2-bin, iperf3, iw
Section: net
Priority: optional
Description: monitor de red en terminal de consumo mínimo
 Interfaz de terminal con pestañas: dispositivos de la red local (IP, MAC,
 fabricante, sistema operativo estimado, puertos abiertos y tráfico por equipo
 con NetFlow), tráfico de la puerta de enlace por SNMP, test de DNS, test de
 velocidad, cliente/servidor iperf3 y redes WiFi. Binario estático de pocos
 MB pensado para una Orange Pi o una Raspberry Pi. Incluye una pestaña de
 ayuda que explica cómo activar NetFlow y
 SNMP en el router. El paquete concede CAP_NET_RAW al binario para medir el
 TTL sin ejecutar como root.
CONTROL

  out="lanwatch_${version}_${arch}.deb"
  docker run --rm -v "$PWD/dist":/d debian:stable-slim \
    dpkg-deb --root-owner-group -Zxz --build "/d/pkg-$arch" "/d/$out" >/dev/null
  echo "dist/$out: $(du -h "dist/$out" | cut -f1)"

  if [ "$lint" = 1 ]; then
    docker run --rm -v "$PWD/dist":/d debian:stable-slim sh -c \
      "apt-get update -qq && apt-get install -y -qq lintian >/dev/null 2>&1 && lintian --no-tag-display-limit /d/$out || true"
  fi
done
