#!/usr/bin/env bash
# Build release binary and package nexus-audio_*.deb
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')"
ARCH="$(dpkg --print-architecture)"
STAGE="$ROOT/packaging/debian/stage"
OUT="$ROOT/packaging/debian/nexus-audio_${VERSION}_${ARCH}.deb"

echo "=== NEXUS//AUDIO ${VERSION} (${ARCH}) ==="

echo "Building release binary..."
cargo build --release

echo "Staging package files..."
install -D -m 755 target/release/nexus-audio "$STAGE/usr/bin/nexus-audio"

if [[ ! -f "$STAGE/usr/share/icons/hicolor/256x256/apps/nexus-audio.png" ]]; then
  if command -v convert >/dev/null 2>&1 && [[ -f icon.ico ]]; then
    mkdir -p "$STAGE/usr/share/icons/hicolor/256x256/apps"
    convert -background none icon.ico[0] \
      "$STAGE/usr/share/icons/hicolor/256x256/apps/nexus-audio.png"
  else
    echo "warning: no menu icon (install imagemagick or keep staged PNG)" >&2
  fi
fi

# Sync control Version with Cargo.toml
sed -i "s/^Version: .*/Version: ${VERSION}/" "$STAGE/DEBIAN/control"

# Installed-Size in KiB (dpkg convention)
SIZE_KB="$(du -sk "$STAGE/usr" | cut -f1)"
sed -i "s/^Installed-Size: .*/Installed-Size: ${SIZE_KB}/" "$STAGE/DEBIAN/control"

echo "Building ${OUT}..."
rm -f "$OUT"
dpkg-deb --root-owner-group --build "$STAGE" "$OUT"

echo "=== Done ==="
echo "$OUT"
dpkg-deb -I "$OUT" | head -12
