/**
 * One owner for the Song view's open drag gestures (region move / trim, loop handles, track-FX
 * knobs). Each gesture opens an undo group on press and must close it exactly once; a gesture
 * whose target goes away mid-drag (region deleted, view switched, drawer closed, loop turned
 * off, pointer cancelled) used to leave its group open, and every later edit was swallowed into
 * it. Electron bug fixed in both apps (user decision 2026-11-18); the Rust side is
 * `App::end_song_gestures` / `settle_song_gestures` in `rune/crates/mdaw-app/src/song.rs`.
 */

const open = new Set<() => void>();

/**
 * Register an open gesture's `end` (idempotent: it runs once, whoever calls it first). Returns
 * the guarded end: call it from the gesture's own release / cancel / unmount paths.
 */
export function trackGesture(end: () => void): () => void {
  let done = false;
  const finish = () => {
    if (done) return;
    done = true;
    open.delete(finish);
    end();
  };
  open.add(finish);
  return finish;
}

/** End every open gesture now (before a destructive key: Delete, L, a view switch). */
export function endOpenGestures(): void {
  for (const end of [...open]) end();
}

/** Open gestures (tests). */
export function openGestureCount(): number {
  return open.size;
}
