#!/usr/bin/env bash
# Phase-10 parity screenshots, 1440x900 at scale 1, into docs/parity/phase-10/: the Rune app
# (--snap) for every phase-10 state, the Electron renderer (ui-snap, headless Chromium) for the
# states it draws (see steps.mjs).
# Usage: scripts/parity-phase-10/run.sh [rune-binary] [state...]
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN=${1:-${CARGO_TARGET_DIR:-rune/target}/debug/micro-daw}
shift || true
OUT=$PWD/docs/parity/phase-10
STEPS=$PWD/scripts/parity-phase-10/steps.mjs
SNAP_DIR=${SNAP_DIR:-$PWD}
mkdir -p "$OUT"
STATES=("$@")
if [ ${#STATES[@]} -eq 0 ]; then
  STATES=(unsaved unsaved-untitled open-report open-error sidebar-recent palette-recent settings-update import-note)
fi
for s in "${STATES[@]}"; do
  "$BIN" --snap "$OUT/$s-rune.png" --scene "$s" --size 1440x900 --scale 1 --at 0.6 >/dev/null 2>&1 || echo "rune: $s failed"
  case "$s" in
    sidebar-recent | palette-recent)
      (cd "$SNAP_DIR" && STATE=$s node scripts/ui-snap.mjs --steps "$STEPS" --out "$OUT/$s-electron.png" >/dev/null 2>&1 || echo "electron: $s failed")
      ;;
  esac
done
