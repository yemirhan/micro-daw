#!/usr/bin/env bash
# Phase-8 parity screenshots: each state from the Electron renderer (ui-snap, headless
# Chromium) and from the Rune app (--snap), 1440x900 at scale 1, into docs/parity/phase-8/.
# Usage: scripts/parity-phase-8/run.sh [rune-binary] [state...]
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN=${1:-${CARGO_TARGET_DIR:-rune/target}/debug/micro-daw}
shift || true
OUT=$PWD/docs/parity/phase-8
STEPS=$PWD/scripts/parity-phase-8/steps.mjs
# ui-snap needs a checkout with its own node_modules (Vite refuses fonts behind a symlink).
SNAP_DIR=${SNAP_DIR:-$PWD}
mkdir -p "$OUT"
STATES=("$@")
if [ ${#STATES[@]} -eq 0 ]; then
  STATES=(song song-region-selected song-regions-edited song-automation song-markers-loop song-track-fx song-piano-roll song-piano-velocity song-piano-drums song-dock-keys song-mixer song-empty song-automation-menu song-instrument-menu song-region-menu song-grouped song-playing)
fi
for s in "${STATES[@]}"; do
  at=0.6
  case $s in song-playing) at=0.9 ;; esac
  "$BIN" --snap "$OUT/$s-rune.png" --scene "$s" --size 1440x900 --scale 1 --at "$at" >/dev/null 2>&1 || echo "rune: $s failed"
  case $s in
    *) (cd "$SNAP_DIR" && STATE=$s node scripts/ui-snap.mjs --steps "$STEPS" --out "$OUT/$s-electron.png" >/dev/null 2>&1 || echo "electron: $s failed") ;;
  esac
done
