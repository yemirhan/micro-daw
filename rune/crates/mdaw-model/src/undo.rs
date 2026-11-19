//! Snapshot undo/redo stacks with nestable undo groups.
//!
//! Ported from `src/services/UndoManager.ts`. The TypeScript manager is a singleton over
//! `Arrangement` snapshots; here it is generic over any cloneable snapshot type `S`.
//!
//! Groups: `begin_undo_group` / `end_undo_group` calls nest (depth counter). The OUTERMOST begin
//! captures the "before" snapshot and its label; inner begins only bump the depth, and the LAST
//! end commits one entry. Every `push_undo` while a group is open is folded into the group (its
//! snapshot is ignored), so independent callers (knob gestures, pad edits, paste/chop) can't
//! clobber each other's groups.
//!
//! Change listeners (`onStateChange`) are replaced by a version counter: every place the TS code
//! calls `emit()` bumps [`UndoManager::version`]. Note that, exactly as in TS, `undo()` / `redo()`
//! themselves do NOT emit (the caller's `push_redo` / `push_undo_from_redo` does).

/// Maximum number of undo entries kept (the oldest is dropped).
pub const MAX_UNDO: usize = 50;

#[derive(Debug, Clone, PartialEq)]
pub struct UndoEntry<S> {
    pub label: String,
    pub snapshot: S,
}

#[derive(Debug, Clone)]
pub struct UndoManager<S: Clone> {
    undo_stack: Vec<UndoEntry<S>>,
    redo_stack: Vec<UndoEntry<S>>,
    group_depth: usize,
    group_label: Option<String>,
    group_snapshot: Option<S>,
    /// Something was pushed (an undoable change happened) inside the open group.
    group_dirty: bool,
    version: u64,
}

impl<S: Clone> Default for UndoManager<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Clone> UndoManager<S> {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            group_depth: 0,
            group_label: None,
            group_snapshot: None,
            group_dirty: false,
            version: 0,
        }
    }

    /// Bumped on every state-change notification (the TS `onStateChange` callbacks).
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn push_undo(&mut self, label: &str, snapshot: S) {
        if self.group_depth > 0 {
            self.group_dirty = true;
            return;
        }
        self.commit(label.to_string(), snapshot, true);
    }

    /// True while an undo group is open (pushes are folded into it; callers can skip cloning).
    pub fn is_grouping(&self) -> bool {
        self.group_depth > 0
    }

    /// Inside a group: record that an undoable change happened without passing a snapshot.
    pub fn mark_dirty(&mut self) {
        if self.group_depth > 0 {
            self.group_dirty = true;
        }
    }

    /// Open (or nest into) an undo group. Only the outermost call captures the snapshot (the
    /// closure is not called for inner begins).
    pub fn begin_undo_group(&mut self, label: &str, snapshot: impl FnOnce() -> S) {
        let depth = self.group_depth;
        self.group_depth += 1;
        if depth > 0 {
            return;
        }
        self.group_label = Some(label.to_string());
        self.group_snapshot = Some(snapshot());
        self.group_dirty = false;
    }

    /// Close one level of the open group; the last close commits ONE entry with the outermost
    /// snapshot and label. A group in which nothing was pushed is still committed unless
    /// `is_unchanged(snapshot)` says the state is the same as before. Unbalanced calls (no open
    /// group) are ignored.
    pub fn end_undo_group(&mut self, is_unchanged: Option<&dyn Fn(&S) -> bool>) {
        if self.group_depth == 0 {
            return;
        }
        self.group_depth -= 1;
        if self.group_depth > 0 {
            return;
        }
        let label = self.group_label.take();
        let snapshot = self.group_snapshot.take();
        let dirty = self.group_dirty;
        self.reset_group();
        let (Some(label), Some(snapshot)) = (label, snapshot) else {
            return;
        };
        if !dirty && is_unchanged.is_some_and(|f| f(&snapshot)) {
            return;
        }
        self.commit(label, snapshot, true);
    }

    /// Commit what the open group changed so far as ONE entry (its label and "before" snapshot)
    /// WITHOUT closing it. The group stays open and restarts from `current()`. Nothing is
    /// committed when the group changed nothing (`is_unchanged`, as in `end_undo_group`).
    pub fn flush_group(
        &mut self,
        current: impl FnOnce() -> S,
        is_unchanged: Option<&dyn Fn(&S) -> bool>,
    ) {
        if self.group_depth == 0 {
            return;
        }
        let (Some(label), Some(snapshot)) =
            (self.group_label.clone(), self.group_snapshot.as_ref())
        else {
            return;
        };
        if !self.group_dirty && is_unchanged.is_some_and(|f| f(snapshot)) {
            return;
        }
        let snapshot = snapshot.clone();
        self.commit(label, snapshot, true);
        self.group_snapshot = Some(current());
        self.group_dirty = false;
    }

    pub fn undo(&mut self) -> Option<S> {
        self.undo_stack.pop().map(|e| e.snapshot)
    }

    pub fn redo(&mut self) -> Option<S> {
        self.redo_stack.pop().map(|e| e.snapshot)
    }

    /// Undo/redo while a group is open (mid-gesture): restart the group from the restored state
    /// (depth is kept, so the gesture's own end still balances).
    pub fn rebase_group(&mut self, snapshot: S) {
        if self.group_depth == 0 {
            return;
        }
        self.group_snapshot = Some(snapshot);
        self.group_dirty = false;
    }

    /// Push current state onto redo when performing an undo.
    pub fn push_redo(&mut self, label: &str, snapshot: S) {
        self.redo_stack.push(UndoEntry {
            label: label.to_string(),
            snapshot,
        });
        self.emit();
    }

    /// Push the current state onto the undo stack while performing a redo: unlike `push_undo` it
    /// keeps the redo stack and is never folded into a group.
    pub fn push_undo_from_redo(&mut self, label: &str, snapshot: S) {
        self.commit(label.to_string(), snapshot, false);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo_stack.last().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo_stack.last().map(|e| e.label.as_str())
    }

    /// Number of undo / redo entries (inspection helpers; not in the TS API).
    pub fn undo_len(&self) -> usize {
        self.undo_stack.len()
    }
    pub fn redo_len(&self) -> usize {
        self.redo_stack.len()
    }
    pub fn undo_entries(&self) -> &[UndoEntry<S>] {
        &self.undo_stack
    }
    pub fn redo_entries(&self) -> &[UndoEntry<S>] {
        &self.redo_stack
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.reset_group();
        self.emit();
    }

    fn commit(&mut self, label: String, snapshot: S, clear_redo: bool) {
        self.undo_stack.push(UndoEntry { label, snapshot });
        if self.undo_stack.len() > MAX_UNDO {
            self.undo_stack.remove(0);
        }
        if clear_redo {
            self.redo_stack.clear();
        }
        self.emit();
    }

    fn reset_group(&mut self) {
        self.group_depth = 0;
        self.group_label = None;
        self.group_snapshot = None;
        self.group_dirty = false;
    }

    fn emit(&mut self) {
        self.version += 1;
    }
}

#[cfg(test)]
#[path = "undo_tests.rs"]
mod tests;
