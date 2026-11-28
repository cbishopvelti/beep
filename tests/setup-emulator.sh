#!/bin/sh
# Bangle.js 2 emulator for headless testing. Installs to $BANGLE_DIR (default /opt/bangle).
# The tests also use Android Integration (apps/android) from BangleApps.
set -e
BANGLE_DIR=${BANGLE_DIR:-/opt/bangle}
mkdir -p "$BANGLE_DIR" && cd "$BANGLE_DIR"
[ -d EspruinoWebIDE ] || git clone -q --depth 1 https://github.com/espruino/EspruinoWebIDE
if [ ! -d BangleApps ]; then
  git clone -q --depth 1 --filter=blob:none --sparse https://github.com/espruino/BangleApps
  cd BangleApps
  git sparse-checkout set --skip-checks bin core modules apps/boot apps/android
  git submodule update --init --depth 1 core webtools
fi
[ -d "$BANGLE_DIR/BangleApps/apps/android" ] || git -C "$BANGLE_DIR/BangleApps" sparse-checkout add --skip-checks apps/android
