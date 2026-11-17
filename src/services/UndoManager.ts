import type { Arrangement } from '@/types/arrangement';

interface UndoEntry {
  label: string;
  snapshot: Arrangement;
}

type StateCallback = () => void;

const MAX_UNDO = 50;

/**
 * Snapshot undo/redo stacks with nestable undo groups.
 *
 * Groups: `beginUndoGroup` / `endUndoGroup` calls nest (depth counter). The
 * OUTERMOST begin captures the "before" snapshot and its label; inner begins
 * only bump the depth, and the LAST end commits one entry. Every pushUndo while
 * a group is open is folded into the group (its snapshot is ignored), so
 * independent callers (knob gestures, pad edits, paste/chop) can't clobber
 * each other's groups.
 */
class UndoManager {
  private undoStack: UndoEntry[] = [];
  private redoStack: UndoEntry[] = [];
  private callbacks: StateCallback[] = [];
  private groupDepth = 0;
  private groupLabel: string | null = null;
  private groupSnapshot: Arrangement | null = null;
  /** Something was pushed (an undoable change happened) inside the open group. */
  private groupDirty = false;

  pushUndo(label: string, snapshot: Arrangement): void {
    if (this.groupDepth > 0) {
      this.groupDirty = true;
      return;
    }
    this.commit(label, snapshot, true);
  }

  /** True while an undo group is open (pushes are folded into it; callers can skip cloning). */
  isGrouping(): boolean {
    return this.groupDepth > 0;
  }

  /** Inside a group: record that an undoable change happened without passing a snapshot. */
  markDirty(): void {
    if (this.groupDepth > 0) this.groupDirty = true;
  }

  /**
   * Open (or nest into) an undo group. Only the outermost call captures the
   * snapshot: pass a function to avoid cloning the arrangement for inner calls.
   */
  beginUndoGroup(label: string, snapshot: Arrangement | (() => Arrangement)): void {
    if (this.groupDepth++ > 0) return;
    this.groupLabel = label;
    this.groupSnapshot = typeof snapshot === 'function' ? snapshot() : snapshot;
    this.groupDirty = false;
  }

  /**
   * Close one level of the open group; the last close commits ONE entry with
   * the outermost snapshot and label. A group in which nothing was pushed is
   * still committed unless `isUnchanged(snapshot)` says the state is the same
   * as before (some edits, e.g. track volume, change state without pushing).
   * Unbalanced calls (no open group) are ignored.
   */
  endUndoGroup(isUnchanged?: (snapshot: Arrangement) => boolean): void {
    if (this.groupDepth === 0) return;
    if (--this.groupDepth > 0) return;
    const label = this.groupLabel;
    const snapshot = this.groupSnapshot;
    const dirty = this.groupDirty;
    this.resetGroup();
    if (label === null || snapshot === null) return;
    if (!dirty && isUnchanged?.(snapshot)) return;
    this.commit(label, snapshot, true);
  }

  undo(): Arrangement | null {
    const entry = this.undoStack.pop();
    if (!entry) return null;
    return entry.snapshot;
  }

  redo(): Arrangement | null {
    const entry = this.redoStack.pop();
    if (!entry) return null;
    return entry.snapshot;
  }

  /**
   * Undo/redo while a group is open (mid-gesture): the group's "before" state
   * is stale, restart it from the restored state (depth is kept, so the
   * gesture's own end still balances).
   */
  rebaseGroup(snapshot: Arrangement): void {
    if (this.groupDepth === 0) return;
    this.groupSnapshot = snapshot;
    this.groupDirty = false;
  }

  /** Push current state onto redo when performing an undo */
  pushRedo(label: string, snapshot: Arrangement): void {
    this.redoStack.push({ label, snapshot });
    this.emit();
  }

  /**
   * Push the current state onto the undo stack while performing a redo:
   * unlike pushUndo it keeps the redo stack (so several redos in a row work)
   * and is never folded into a group.
   */
  pushUndoFromRedo(label: string, snapshot: Arrangement): void {
    this.commit(label, snapshot, false);
  }

  canUndo(): boolean {
    return this.undoStack.length > 0;
  }

  canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  undoLabel(): string | undefined {
    return this.undoStack[this.undoStack.length - 1]?.label;
  }

  redoLabel(): string | undefined {
    return this.redoStack[this.redoStack.length - 1]?.label;
  }

  clear(): void {
    this.undoStack = [];
    this.redoStack = [];
    this.resetGroup();
    this.emit();
  }

  onStateChange(callback: StateCallback): () => void {
    this.callbacks.push(callback);
    return () => {
      this.callbacks = this.callbacks.filter((cb) => cb !== callback);
    };
  }

  private commit(label: string, snapshot: Arrangement, clearRedo: boolean): void {
    this.undoStack.push({ label, snapshot });
    if (this.undoStack.length > MAX_UNDO) {
      this.undoStack.shift();
    }
    if (clearRedo) this.redoStack = [];
    this.emit();
  }

  private resetGroup(): void {
    this.groupDepth = 0;
    this.groupLabel = null;
    this.groupSnapshot = null;
    this.groupDirty = false;
  }

  private emit(): void {
    for (const cb of this.callbacks) cb();
  }
}

export const undoManager = new UndoManager();
