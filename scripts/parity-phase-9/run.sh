#!/usr/bin/env bash
# Phase-9 parity screenshots: each state from the Electron renderer (ui-snap, headless
# Chromium) and from the Rune app (--snap), 1440x900 at scale 1, into docs/parity/phase-9/.
# Usage: scripts/parity-phase-9/run.sh [rune-binary] [state...]
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN=${1:-${CARGO_TARGET_DIR:-rune/target}/debug/micro-daw}
shift || true
OUT=$PWD/docs/parity/phase-9
STEPS=$PWD/scripts/parity-phase-9/steps.mjs
SNAP_DIR=${SNAP_DIR:-$PWD}
mkdir -p "$OUT"
STATES=("$@")
if [ ${#STATES[@]} -eq 0 ]; then
  STATES=(samples-empty samples-sine samples-trim samples-process samples-generate packs packs-builtin packs-remote packs-search settings dev dev-events palette palette-search export templates controller synth-editor instrument-menu live-fx song-audio)
fi
for s in "${STATES[@]}"; do
  "$BIN" --snap "$OUT/$s-rune.png" --scene "$s" --size 1440x900 --scale 1 --at 0.6 >/dev/null 2>&1 || echo "rune: $s failed"
  (cd "$SNAP_DIR" && STATE=$s node scripts/ui-snap.mjs --steps "$STEPS" --out "$OUT/$s-electron.png" >/dev/null 2>&1 || echo "electron: $s failed")
done
