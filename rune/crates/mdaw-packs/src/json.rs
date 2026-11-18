//! `JSON.stringify(value, null, 2)` for the files the installer writes (`pack.json`), so a pack
//! installed by the Rust app is byte-identical to one the Electron app installed (apart from
//! `dir` and `installedAt`). Numbers use JS formatting (`1` not `1.0`, `1e+21`).

use mdaw_model::js;
use serde_json::Value;

/// `JSON.stringify(value, null, 2)`.
pub fn stringify_pretty(v: &Value) -> String {
    let mut out = String::new();
    write(&mut out, v, 0);
    out
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn write(out: &mut String, v: &Value, level: usize) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else {
                let f = n.as_f64().unwrap_or(f64::NAN);
                if f.is_finite() {
                    out.push_str(&js::number_to_string(f));
                } else {
                    out.push_str("null");
                }
            }
        }
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap_or_default()),
        Value::Array(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                indent(out, level + 1);
                write(out, x, level + 1);
                if i + 1 < a.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, level);
            out.push(']');
        }
        Value::Object(o) => {
            if o.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            let n = o.len();
            for (i, (k, x)) in o.iter().enumerate() {
                indent(out, level + 1);
                out.push_str(&serde_json::to_string(k).unwrap_or_default());
                out.push_str(": ");
                write(out, x, level + 1);
                if i + 1 < n {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, level);
            out.push('}');
        }
    }
}

/// `new Date().toISOString()` (UTC, milliseconds).
pub fn iso_now() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    iso_from_millis(d.as_millis() as i64)
}

/// `new Date(ms).toISOString()` for years 0..=9999.
pub fn iso_from_millis(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    // Civil from days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y,
        m,
        d,
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates_match_to_iso_string() {
        assert_eq!(iso_from_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            iso_from_millis(1_763_478_123_456),
            "2025-11-18T15:02:03.456Z"
        );
        assert_eq!(iso_from_millis(951_782_400_000), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn pretty_json_matches_json_stringify() {
        let v: Value = serde_json::from_str(
            r#"{"a":1.0,"b":[],"c":{},"d":[1,"x\u0001\n\"é"],"e":0.5,"f":1e21,"g":null,"h":{"i":true}}"#,
        )
        .unwrap();
        assert_eq!(
            stringify_pretty(&v),
            "{\n  \"a\": 1,\n  \"b\": [],\n  \"c\": {},\n  \"d\": [\n    1,\n    \"x\\u0001\\n\\\"é\"\n  ],\n  \"e\": 0.5,\n  \"f\": 1e+21,\n  \"g\": null,\n  \"h\": {\n    \"i\": true\n  }\n}"
        );
    }
}
