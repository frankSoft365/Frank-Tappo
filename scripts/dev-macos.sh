#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

LABEL="com.franktappo.dev"
BIN="$PWD/src-tauri/target/debug/frank-tappo"

if [ ! -x node_modules/.bin/vite ]; then
  npm install
fi
if ! lsof -nP -iTCP:1420 -sTCP:LISTEN >/dev/null 2>&1; then
  nohup npx vite >/tmp/frank-vite.log 2>&1 &
fi

cargo build --manifest-path src-tauri/Cargo.toml

# Signing with a stable identity keeps the TCC (Input Monitoring) grant valid
# across rebuilds; an ad-hoc signature changes its cdhash every link.
IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null \
  | sed -n 's/.*"\(Apple Development[^"]*\)".*/\1/p' | head -n 1)
if [ -n "$IDENTITY" ]; then
  codesign --force --timestamp=none --sign "$IDENTITY" --identifier com.franktappo.app "$BIN"
else
  echo "warning: no Apple Development identity found; TCC grant will reset on rebuild"
fi

launchctl remove "$LABEL" >/dev/null 2>&1 || true
launchctl submit -l "$LABEL" -o /tmp/frank-out.log -e /tmp/frank-err.log -- "$BIN"

echo "Frank Tappo started (launchctl: $LABEL)"
echo "Logs: tail -f /tmp/frank-err.log"
echo "Stop: launchctl remove $LABEL"
echo "Note: a rebuild changes the binary signature; re-add it in Input Monitoring if reactions stop."
