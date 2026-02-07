import type { Arrangement } from '@/types/arrangement';

interface UndoEntry {
  label: string;
  snapshot: Arrangement;
}

type StateCallback = () => void;

const MAX_UNDO = 50;

class UndoManager {
  private undoStack: UndoEntry[] = [];
  private redoStack: UndoEntry[] = [];
  private callbacks: StateCallback[] = [];
  private groupLabel: string | null = null;
  private groupSnapshot: Arrangement | null = null;

  pushUndo(label: string, snapshot: Arrangement): void {
    // If inside a group, only capture the first snapshot
    if (this.groupLabel !== null) {
      if (this.groupSnapshot === null) {
        this.groupSnapshot = snapshot;
      }
      return;
    }

    this.undoStack.push({ label, snapshot });
    if (this.undoStack.length > MAX_UNDO) {
      this.undoStack.shift();
    }
    this.redoStack = [];
    this.emit();
  }

  /** Call before a slider/continuous operation begins */
  beginUndoGroup(label: string, snapshot: Arrangement): void {
    this.groupLabel = label;
    this.groupSnapshot = snapshot;
  }

  /** Call when the slider/continuous operation ends */
  endUndoGroup(): void {
    if (this.groupLabel !== null && this.groupSnapshot !== null) {
      this.undoStack.push({ label: this.groupLabel, snapshot: this.groupSnapshot });
      if (this.undoStack.length > MAX_UNDO) {
        this.undoStack.shift();
      }
      this.redoStack = [];
      this.emit();
    }
    this.groupLabel = null;
    this.groupSnapshot = null;
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

  /** Push current state onto redo when performing an undo */
  pushRedo(label: string, snapshot: Arrangement): void {
    this.redoStack.push({ label, snapshot });
    this.emit();
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
    this.groupLabel = null;
    this.groupSnapshot = null;
    this.emit();
  }

  onStateChange(callback: StateCallback): () => void {
    this.callbacks.push(callback);
    return () => {
      this.callbacks = this.callbacks.filter((cb) => cb !== callback);
    };
  }

  private emit(): void {
    for (const cb of this.callbacks) cb();
  }
}

export const undoManager = new UndoManager();
