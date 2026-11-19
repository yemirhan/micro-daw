//! Shared test helpers (review L4: one JSON comparator for every golden test).

use serde_json::Value;

/// One difference found by [`json_diffs`].
#[derive(Debug, Clone, PartialEq)]
pub struct JsonDiff {
    pub path: String,
    /// None = key absent on that side.
    pub got: Option<Value>,
    pub want: Option<Value>,
}

impl std::fmt::Display for JsonDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let show = |v: &Option<Value>| v.as_ref().map_or("<absent>".to_owned(), Value::to_string);
        write!(
            f,
            "{}: {} != {}",
            self.path,
            show(&self.got),
            show(&self.want)
        )
    }
}

/// Numbers compare like JS numbers (`120` == `120.0`), within 1e-9.
fn num_eq(a: &serde_json::Number, b: &serde_json::Number) -> bool {
    let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
    (a - b).abs() <= 1e-9
}

/// Every structural difference (numbers within 1e-9, object key order ignored).
pub fn json_diffs(got: &Value, want: &Value) -> Vec<JsonDiff> {
    let mut out = Vec::new();
    collect(got, want, "$", &mut out);
    out
}

fn collect(got: &Value, want: &Value, path: &str, out: &mut Vec<JsonDiff>) {
    match (got, want) {
        (Value::Number(a), Value::Number(b)) => {
            if !num_eq(a, b) {
                out.push(JsonDiff {
                    path: path.into(),
                    got: Some(got.clone()),
                    want: Some(want.clone()),
                });
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                collect(x, y, &format!("{path}[{i}]"), out);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
            keys.sort();
            keys.dedup();
            for k in keys {
                let p = format!("{path}.{k}");
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => collect(x, y, &p, out),
                    (x, y) => out.push(JsonDiff {
                        path: p,
                        got: x.cloned(),
                        want: y.cloned(),
                    }),
                }
            }
        }
        _ => {
            if got != want {
                out.push(JsonDiff {
                    path: path.into(),
                    got: Some(got.clone()),
                    want: Some(want.clone()),
                });
            }
        }
    }
}

/// The first difference as text (None when equal). `path` prefixes the reported path.
pub fn json_diff(got: &Value, want: &Value, path: &str) -> Option<String> {
    json_diffs(got, want).into_iter().next().map(|d| {
        let mut d = d;
        d.path = format!("{path}{}", &d.path[1..]);
        d.to_string()
    })
}

#[track_caller]
pub fn assert_json_eq(got: &Value, want: &Value, ctx: &str) {
    if let Some(d) = json_diff(got, want, "$") {
        panic!("{ctx}: {d}\n got: {got}\nwant: {want}");
    }
}

/// The arrangement patches of `rune/parity/fixtures/doc.json` (doc.golden.ts `diff`).
/// RFC 6901 pointer tokens.
fn tokens(ptr: &str) -> Vec<String> {
    ptr.split('/')
        .skip(1)
        .map(|t| t.replace("~1", "/").replace("~0", "~"))
        .collect()
}

/// Apply one `arr` patch (see doc.golden.ts): `[ptr, v]` set, `[ptr, v, 1]` array insert,
/// `[ptr]` delete / array splice.
pub fn apply_patch(root: &mut Value, patch: &[Value]) {
    let ptr = patch[0].as_str().unwrap();
    if ptr.is_empty() {
        *root = patch.get(1).cloned().unwrap_or(Value::Null);
        return;
    }
    let toks = tokens(ptr);
    let (last, parents) = toks.split_last().unwrap();
    let mut cur = &mut *root;
    for t in parents {
        cur = match cur {
            Value::Object(m) => m.get_mut(t).unwrap(),
            Value::Array(a) => &mut a[t.parse::<usize>().unwrap()],
            _ => panic!("bad patch {ptr}"),
        };
    }
    match (cur, patch.get(1), patch.get(2)) {
        (Value::Object(m), Some(v), _) => {
            m.insert(last.clone(), v.clone());
        }
        (Value::Object(m), None, _) => {
            m.shift_remove(last);
        }
        (Value::Array(a), Some(v), Some(_)) => a.insert(last.parse().unwrap(), v.clone()),
        (Value::Array(a), Some(v), None) => a[last.parse::<usize>().unwrap()] = v.clone(),
        (Value::Array(a), None, _) => {
            a.remove(last.parse::<usize>().unwrap());
        }
        _ => panic!("bad patch {ptr}"),
    }
}

/// vitest `toBeCloseTo(b, digits)`: `|a - b| < 10^-digits / 2`.
pub fn close(a: f64, b: f64, digits: i32) -> bool {
    (a - b).abs() < 10f64.powi(-digits) / 2.0
}

/// The deterministic id generator the golden scripts install (`uuid-1`, `uuid-2`, ...).
pub fn counter_ids() -> impl FnMut() -> String {
    let mut n = 0;
    move || {
        n += 1;
        format!("uuid-{n}")
    }
}

/// `ALL_TEMPLATES` (src/data/templates/index.ts order) as raw JSON, with their ids.
pub fn templates() -> Vec<(&'static str, Value)> {
    crate::templates_data::TEMPLATE_FILES
        .into_iter()
        .map(|(id, s)| (id, serde_json::from_str(s).expect("template JSON")))
        .collect()
}

#[test]
fn comparator_is_not_vacuous() {
    use serde_json::json;
    assert!(json_diff(&json!({ "a": [1, 2.0] }), &json!({ "a": [1.0, 2] }), "$").is_none());
    assert!(json_diff(&json!({ "a": 1 }), &json!({ "a": 1.000001 }), "$").is_some());
    assert!(json_diff(&json!({ "a": 1 }), &json!({ "a": 1, "b": null }), "$").is_some());
    assert!(json_diff(&json!(["x"]), &json!(["x", "y"]), "$").is_some());
    assert!(json_diff(&json!("100"), &json!(100), "$").is_some());
    assert_eq!(
        json_diffs(
            &json!({ "a": 1, "b": [1, 2] }),
            &json!({ "a": 2, "b": [1, 3] })
        )
        .len(),
        2
    );
    assert_eq!(
        json_diff(&json!({ "a": 1 }), &json!({ "a": 2 }), "ret").as_deref(),
        Some("ret.a: 1 != 2")
    );
}
