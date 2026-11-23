#!/bin/sh
# Bangle.js 2 emulator for headless testing. Installs to $BANGLE_DIR (default /opt/bangle).
set -e
BANGLE_DIR=${BANGLE_DIR:-/opt/bangle}
mkdir -p "$BANGLE_DIR" && cd "$BANGLE_DIR"
[ -d EspruinoWebIDE ] || git clone -q --depth 1 https://github.com/espruino/EspruinoWebIDE
if [ ! -d BangleApps ]; then
  git clone -q --depth 1 --filter=blob:none --sparse https://github.com/espruino/BangleApps
  cd BangleApps
  git sparse-checkout set --skip-checks bin core modules apps/boot
  git submodule update --init --depth 1 core webtools
fi
