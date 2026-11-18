#!/usr/bin/env bash
# Phase-7 parity screenshots: each state from the Electron renderer (ui-snap, headless
# Chromium) and from the Rune app (--snap), 1440x900 at scale 1, into docs/parity/phase-7/.
# Usage: scripts/parity-phase-7/run.sh [rune-binary] [state...]
set -euo pipefail
cd "$(dirname "$0")/../.."
BIN=${1:-rune/target/debug/micro-daw}
shift || true
OUT=$PWD/docs/parity/phase-7
STEPS=$PWD/scripts/parity-phase-7/steps.mjs
# ui-snap needs a checkout with its own node_modules (Vite refuses fonts behind a symlink).
SNAP_DIR=${SNAP_DIR:-$PWD}
mkdir -p "$OUT"
STATES=("$@")
if [ ${#STATES[@]} -eq 0 ]; then
  STATES=(start beat browser pad-selected pattern playing pad-menu kit-menu drum-track-menu quantize-menu chop count-in recording note-repeat key-hints layout-2x8 dock-open edited-pad missing-packs)
fi
for s in "${STATES[@]}"; do
  at=0.6
  case $s in playing|recording) at=0.9 ;; esac
  "$BIN" --snap "$OUT/$s-rune.png" --scene "$s" --size 1440x900 --scale 1 --at "$at" >/dev/null 2>&1 || echo "rune: $s failed"
  case $s in
    missing-packs) continue ;; # no Electron reference: the browser-only renderer has no pack store
    start) (cd "$SNAP_DIR" && node scripts/ui-snap.mjs --no-start --out "$OUT/$s-electron.png" >/dev/null 2>&1 || echo "electron: $s failed") ;;
    *) (cd "$SNAP_DIR" && STATE=$s node scripts/ui-snap.mjs --steps "$STEPS" --out "$OUT/$s-electron.png" >/dev/null 2>&1 || echo "electron: $s failed") ;;
  esac
done
