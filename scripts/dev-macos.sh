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

launchctl remove "$LABEL" >/dev/null 2>&1 || true
launchctl submit -l "$LABEL" -o /tmp/frank-out.log -e /tmp/frank-err.log -- "$BIN"

echo "Frank Tappo started (launchctl: $LABEL)"
echo "Logs: tail -f /tmp/frank-err.log"
echo "Stop: launchctl remove $LABEL"
echo "Note: a rebuild changes the binary signature; re-add it in Input Monitoring if reactions stop."
