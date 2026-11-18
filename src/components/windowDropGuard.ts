/**
 * Window-level file-drop guard. Without it, a file dropped anywhere that is
 * not a drop target (e.g. next to a pad) makes Chromium / Electron navigate
 * the window to that file, unloading the app and the open project.
 *
 * Listeners run in the bubble phase, after the real drop targets (pads,
 * browser rows, arrangement) have handled the event: a drag over a target
 * that accepted it (default prevented) is left alone; anywhere else the
 * cursor shows "no drop" and the drop is swallowed.
 */
export function installWindowDropGuard(target: Window = window): () => void {
  const onDragOver = (e: DragEvent) => {
    if (e.defaultPrevented) return; // a drop target accepted it
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'none';
  };
  const onDrop = (e: DragEvent) => {
    // Handled or not, the browser default (open the file) must never run.
    e.preventDefault();
  };
  target.addEventListener('dragover', onDragOver);
  target.addEventListener('drop', onDrop);
  return () => {
    target.removeEventListener('dragover', onDragOver);
    target.removeEventListener('drop', onDrop);
  };
}
