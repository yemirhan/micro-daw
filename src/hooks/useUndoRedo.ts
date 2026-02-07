import { useState, useCallback, useEffect } from 'react';
import { undoManager } from '@/services/UndoManager';
import { arrangementEngine } from '@/services/ArrangementEngine';

export function useUndoRedo() {
  const [canUndo, setCanUndo] = useState(false);
  const [canRedo, setCanRedo] = useState(false);
  const [undoLabel, setUndoLabel] = useState<string | undefined>();
  const [redoLabel, setRedoLabel] = useState<string | undefined>();

  useEffect(() => {
    const sync = () => {
      setCanUndo(undoManager.canUndo());
      setCanRedo(undoManager.canRedo());
      setUndoLabel(undoManager.undoLabel());
      setRedoLabel(undoManager.redoLabel());
    };
    sync();
    return undoManager.onStateChange(sync);
  }, []);

  const undo = useCallback(() => {
    arrangementEngine.performUndo();
  }, []);

  const redo = useCallback(() => {
    arrangementEngine.performRedo();
  }, []);

  return { canUndo, canRedo, undoLabel, redoLabel, undo, redo };
}
