#!/usr/bin/env bash
# Proves the Rust app needs nothing from the Electron app's sources (phase 10 review finding 19):
# hides the repository's `src/` (renamed for the duration, restored on any exit), then builds the
# app and runs the whole workspace's tests.
#
#   rune/scripts/check-without-src.sh            # build + cargo test --workspace
#   rune/scripts/check-without-src.sh --build    # build only
#
# Still outside `rune/` (and not under `src/`): `packs/catalog.json` and `packs/withdrawn.json`
# (compiled in by mdaw-packs / mdaw-model). What still needs `src/` is listed in
# docs/rune-port-plan.md ("Deleting src/").
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
repo="$(cd "$here/.." && pwd)"
hidden="$repo/.src-hidden-$$"
[ -d "$repo/src" ] || { echo "no src/ to hide"; exit 1; }
mv "$repo/src" "$hidden"
trap 'mv "$hidden" "$repo/src"' EXIT
echo "[check-without-src] src/ hidden at $hidden"
cd "$here"
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
# Every crate rebuilds against the hidden tree (an include_str! of src/ would fail here).
find crates -name '*.rs' -exec touch {} +
cargo build -p mdaw-app
if [ "${1:-}" != "--build" ]; then
  cargo test --workspace --no-fail-fast
fi
echo "[check-without-src] OK: the Rust tree builds and tests without src/"
