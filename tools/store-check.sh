#!/usr/bin/env bash
# Checks Bleep Test the way the official Bangle.js app store (espruino/BangleApps)
# checks a pull request: its bin/sanitycheck.js over every app, plus its
# ESLint rules (no warnings allowed) over apps/bleeptest.
#
#   sh tools/store-check.sh [bangleapps dir]
#
# Uses (or makes) a BangleApps clone, default $BANGLE_DIR/BangleApps-store
# (BANGLE_DIR defaults to /opt/bangle, as for the tests), and copies the
# app's files into its apps/bleeptest with tools/app-files.js. Needs git and
# Node 18+; it runs `npm ci` in the clone for ESLint.
set -eu

REPO_DIR=$(cd "$(dirname "$0")/.." && pwd)
DIR=${1:-${BANGLE_DIR:-/opt/bangle}/BangleApps-store}
UPSTREAM=${BANGLEAPPS_URL:-https://github.com/espruino/BangleApps.git}

if [ ! -d "$DIR/.git" ]; then
  git clone -q --depth 1 --filter=blob:none "$UPSTREAM" "$DIR"
  git -C "$DIR" submodule update -q --init --depth 1
fi
cd "$DIR"
[ -d node_modules ] || npm ci --silent --no-audit --no-fund

rm -rf apps/bleeptest
node "$REPO_DIR/tools/app-files.js" apps/bleeptest "$REPO_DIR"

status=0
out=$(node bin/sanitycheck.js 2>&1) || status=1
echo "$out" | tail -3
# Other apps' known problems are upstream's; any line naming Bleep Test is ours.
if echo "$out" | grep -i bleeptest; then status=1; fi
npx eslint --max-warnings 0 apps/bleeptest || status=1

if [ "$status" = 0 ]; then echo "store check passed"; else echo "store check FAILED"; fi
exit $status
