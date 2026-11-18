use std::cell::Cell;

use serde_json::{Value, json};

use super::*;

// --- behavioural tests of the manager itself (the TS suite only covers it through the engine) ---

#[test]
fn push_undo_redo_labels() {
    let mut um = UndoManager::<i32>::new();
    assert!(!um.can_undo());
    um.push_undo("A", 1);
    um.push_undo("B", 2);
    assert_eq!(um.undo_label(), Some("B"));
    assert_eq!(um.undo(), Some(2));
    um.push_redo("B", 3);
    assert_eq!(um.redo_label(), Some("B"));
    assert!(um.can_redo());
    // pushUndoFromRedo keeps the redo stack.
    um.push_undo_from_redo("X", 9);
    assert!(um.can_redo());
    // pushUndo clears it.
    um.push_undo("C", 4);
    assert!(!um.can_redo());
    assert_eq!(um.undo(), Some(4));
    assert_eq!(um.undo(), Some(9));
    assert_eq!(um.undo(), Some(1));
    assert_eq!(um.undo(), None);
}

#[test]
fn caps_at_max_undo() {
    let mut um = UndoManager::<usize>::new();
    for i in 0..60 {
        um.push_undo("p", i);
    }
    assert_eq!(um.undo_len(), MAX_UNDO);
    assert_eq!(um.undo_entries()[0].snapshot, 10);
}

#[test]
fn nested_groups_commit_once_with_the_outermost_snapshot() {
    let mut um = UndoManager::<i32>::new();
    let calls = Cell::new(0);
    um.begin_undo_group("Outer", || {
        calls.set(calls.get() + 1);
        1
    });
    um.begin_undo_group("Inner", || {
        calls.set(calls.get() + 1);
        2
    });
    assert_eq!(calls.get(), 1);
    um.push_undo("ignored", 5);
    um.end_undo_group(None);
    assert!(!um.can_undo());
    um.end_undo_group(None);
    assert_eq!(um.undo_label(), Some("Outer"));
    assert_eq!(um.undo(), Some(1));
    // Unbalanced end is ignored.
    um.end_undo_group(None);
    assert!(!um.can_undo());
}

#[test]
fn unchanged_group_is_dropped_unless_dirty() {
    let mut um = UndoManager::<i32>::new();
    let yes = |_: &i32| true;
    um.begin_undo_group("G", || 1);
    um.end_undo_group(Some(&yes));
    assert!(!um.can_undo());
    um.begin_undo_group("G", || 1);
    um.mark_dirty();
    um.end_undo_group(Some(&yes));
    assert!(um.can_undo());
}

#[test]
fn flush_and_rebase_keep_the_group_open() {
    let mut um = UndoManager::<i32>::new();
    um.begin_undo_group("Knob", || 1);
    um.push_undo("x", 0);
    um.flush_group(|| 2, None);
    assert_eq!(um.undo_label(), Some("Knob"));
    assert!(um.is_grouping());
    assert_eq!(um.undo(), Some(1));
    um.rebase_group(7);
    um.end_undo_group(None);
    assert_eq!(um.undo(), Some(7));
    let v = um.version();
    um.clear();
    assert_eq!(um.version(), v + 1);
}

// --- golden session replay ---------------------------------------------------------------------

fn entries(es: &[UndoEntry<i64>]) -> Value {
    Value::Array(es.iter().map(|e| json!([e.label, e.snapshot])).collect())
}

#[test]
fn golden_sessions_match_typescript() {
    let data: Value = serde_json::from_str(include_str!("../../../parity/fixtures/undo.json")).unwrap();
    let mut compared = 0;
    for (si, session) in data["sessions"].as_array().unwrap().iter().enumerate() {
        let mut um = UndoManager::<i64>::new();
        um.clear();
        let base = um.version();
        for (i, step) in session["steps"].as_array().unwrap().iter().enumerate() {
            let op = step[0].as_array().unwrap();
            let name = op[0].as_str().unwrap();
            let ret: Value = match name {
                "push" => {
                    um.push_undo(op[1].as_str().unwrap(), op[2].as_i64().unwrap());
                    Value::Null
                }
                "begin" => {
                    let v = op[2].as_i64().unwrap();
                    let called = Cell::new(0);
                    // lazy or eager: the TS eager form also has no observable call
                    let lazy = op[3].as_bool().unwrap();
                    um.begin_undo_group(op[1].as_str().unwrap(), || {
                        if lazy {
                            called.set(called.get() + 1);
                        }
                        v
                    });
                    json!(called.get())
                }
                "end" => {
                    let calls = Cell::new(0);
                    let mode = op[1].clone();
                    let f = |before: &i64| {
                        calls.set(calls.get() + 1);
                        match &mode {
                            Value::String(_) => before % 2 == 0,
                            Value::Bool(b) => *b,
                            _ => unreachable!(),
                        }
                    };
                    if mode.is_null() {
                        um.end_undo_group(None);
                    } else {
                        um.end_undo_group(Some(&f));
                    }
                    json!(calls.get())
                }
                "flush" => {
                    let v = op[1].as_i64().unwrap();
                    let calls = Cell::new(0);
                    let current_calls = Cell::new(0);
                    let mode = op[2].as_bool();
                    let f = |_: &i64| {
                        calls.set(calls.get() + 1);
                        mode.unwrap()
                    };
                    um.flush_group(
                        || {
                            current_calls.set(current_calls.get() + 1);
                            v
                        },
                        if mode.is_some() { Some(&f) } else { None },
                    );
                    json!([calls.get(), current_calls.get()])
                }
                "rebase" => {
                    um.rebase_group(op[1].as_i64().unwrap());
                    Value::Null
                }
                "markDirty" => {
                    um.mark_dirty();
                    Value::Null
                }
                "undo" => {
                    let lbl = um.undo_label().map(str::to_string);
                    let got = um.undo();
                    if let Some(v) = op.get(1) {
                        um.push_redo(lbl.as_deref().unwrap_or("x"), v.as_i64().unwrap());
                    }
                    json!(got)
                }
                "redo" => {
                    let lbl = um.redo_label().map(str::to_string);
                    let got = um.redo();
                    if let Some(v) = op.get(1) {
                        um.push_undo_from_redo(lbl.as_deref().unwrap_or("x"), v.as_i64().unwrap());
                    }
                    json!(got)
                }
                "clear" => {
                    um.clear();
                    Value::Null
                }
                "pushRedo" => {
                    um.push_redo(op[1].as_str().unwrap(), op[2].as_i64().unwrap());
                    Value::Null
                }
                other => panic!("unknown op {other}"),
            };
            let ctx = format!("session {si} step {i} op {op:?}");
            assert_eq!(ret, step[1], "{ctx}: return");
            let exp = &step[2];
            let undo = um.undo_entries();
            let redo = um.redo_entries();
            let got = json!({
                "canUndo": um.can_undo(),
                "canRedo": um.can_redo(),
                "undoLabel": um.undo_label(),
                "redoLabel": um.redo_label(),
                "grouping": um.is_grouping(),
                "undoLen": undo.len(),
                "redoLen": redo.len(),
                "undoBottom": entries(&undo[..undo.len().min(1)]),
                "undoTop": entries(&undo[undo.len().saturating_sub(3)..]),
                "redoTop": entries(&redo[redo.len().saturating_sub(3)..]),
                "emits": um.version() - base,
            });
            assert_eq!(&got, exp, "{ctx}");
            compared += 1;
        }
        assert_eq!(
            entries(um.undo_entries()),
            session["finalUndo"],
            "session {si} final undo"
        );
        assert_eq!(
            entries(um.redo_entries()),
            session["finalRedo"],
            "session {si} final redo"
        );
    }
    assert!(compared > 3000);
}
