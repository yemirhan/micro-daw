//! JavaScript semantics the ported code depends on: `String.prototype.trim` / `\s`, UTF-16
//! `length` and `<` ordering, `Number(string)`, `String(value)`, `Number.prototype.toString`,
//! `toFixed`, `Math.round`, `Date.parse` (ISO subset), object key order and object spread.
//! Shared by the pack, fs-entry and settings ports of this crate.

use std::cmp::Ordering;

use serde_json::{Map, Value};

/// JS `WhiteSpace` + `LineTerminator` (what `trim()` strips and `\s` matches).
pub fn is_js_ws(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `String.prototype.trim`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_ws)
}

/// `string.length` (UTF-16 code units).
pub fn len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// JS `a < b` / `a > b` on strings: UTF-16 code unit order.
pub fn utf16_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Lower-case ASCII letter test used for `[a-z]` with the `i` flag (non-unicode regexps
/// canonicalize case only within ASCII for these ranges).
pub fn is_ascii_alpha(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// `Math.round`: halves round toward +∞; results in (-0.5, 0] are -0.
pub fn round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let r = x.floor();
    let out = if x - r >= 0.5 { r + 1.0 } else { r };
    if out == 0.0 && x < 0.0 { -0.0 } else { out }
}

/// `Number.isInteger(v)` for a JSON value: the number when it is an integral finite number.
pub fn integer_of(v: Option<&Value>) -> Option<f64> {
    let n = v?.as_f64()?;
    (n.is_finite() && n.fract() == 0.0).then_some(n)
}

/// A JSON number the way `JSON.stringify` writes it: integral values without a fraction.
pub fn number_value(x: f64) -> Value {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.007_199_254_740_992e15 {
        Value::from(x as i64)
    } else {
        serde_json::Number::from_f64(x)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }
}

/// Re-encodes integral floats (`1.0`) as integers so values compare like JS numbers.
pub fn normalize_number(v: &Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => number_value(n.as_f64().unwrap_or(f64::NAN)),
        other => other.clone(),
    }
}

/// `Number.prototype.toString()` (radix 10), ECMAScript `Number::toString`.
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x.is_infinite() {
        return if x > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    let sign = if x < 0.0 { "-" } else { "" };
    // Rust's `{:e}` prints the shortest round-tripping digits, like JS.
    let e = format!("{:e}", x.abs());
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i64;
    let n = exp.parse::<i64>().unwrap_or(0) + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let es = if e >= 0 {
            format!("+{e}")
        } else {
            format!("{e}")
        };
        if k == 1 {
            format!("{digits}e{es}")
        } else {
            format!("{}.{}e{es}", &digits[..1], &digits[1..])
        }
    };
    format!("{sign}{body}")
}

/// `Number(string)` (ECMAScript `StringToNumber`).
pub fn string_to_number(s: &str) -> f64 {
    let t = trim(s);
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    let b = t.as_bytes();
    if b.len() > 2 && b[0] == b'0' {
        let radix = match b[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 0,
        };
        if radix != 0 {
            let mut v = 0.0f64;
            for c in t[2..].chars() {
                match c.to_digit(radix) {
                    Some(d) => v = v * radix as f64 + d as f64,
                    None => return f64::NAN,
                }
            }
            return v;
        }
    }
    // StrDecimalLiteral: [+-] (digits [. digits*] | . digits) [(e|E) [+-] digits]
    let mut i = 0;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    let int_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - int_start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let f = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - f;
    }
    if digits == 0 {
        return f64::NAN;
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let e = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == e {
            return f64::NAN;
        }
    }
    if i != b.len() {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `String(value)` for a JSON value (`None` = `undefined`).
pub fn string_of(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|e| {
                if e.is_null() {
                    String::new()
                } else {
                    string_of(Some(e))
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// `x.toFixed(digits)` for finite `|x| < 1e21` (exact decimal expansion, ties away from zero
/// on the exact value as the spec's "larger n" rule).
pub fn to_fixed(x: f64, digits: usize) -> String {
    if !x.is_finite() || x.abs() >= 1e21 {
        return number_to_string(x);
    }
    let neg = x < 0.0;
    let exact = format!("{:.1100}", x.abs());
    let (int_part, frac) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut ds: Vec<u8> = int_part
        .bytes()
        .chain(frac.bytes().take(digits))
        .map(|c| c - b'0')
        .collect();
    let round_up = frac.as_bytes().get(digits).is_some_and(|d| *d >= b'5');
    if round_up {
        let mut i = ds.len();
        loop {
            if i == 0 {
                ds.insert(0, 1);
                break;
            }
            i -= 1;
            if ds[i] == 9 {
                ds[i] = 0;
            } else {
                ds[i] += 1;
                break;
            }
        }
    }
    let int_len = ds.len() - digits;
    let mut s: String = ds[..int_len].iter().map(|d| (b'0' + d) as char).collect();
    if digits > 0 {
        s.push('.');
        s.extend(ds[int_len..].iter().map(|d| (b'0' + d) as char));
    }
    // Spec: x < 0 → "-" + toFixed(-x) (so (-0.04).toFixed(1) is "-0.0").
    if neg { format!("-{s}") } else { s }
}

/// True for an array-index property key ("0", "17"; not "01", "-1", "4294967295").
pub fn is_array_index(key: &str) -> bool {
    if key.is_empty() || key.len() > 10 || !key.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if key.len() > 1 && key.starts_with('0') {
        return false;
    }
    key.parse::<u64>().is_ok_and(|n| n < 4_294_967_295)
}

/// JS own-property order: array-index keys ascending first, then the others in insertion order.
pub fn order_keys(map: Map<String, Value>) -> Map<String, Value> {
    if !map.keys().any(|k| is_array_index(k)) {
        return map;
    }
    let mut idx: Vec<(u64, String, Value)> = Vec::new();
    let mut rest: Vec<(String, Value)> = Vec::new();
    for (k, v) in map {
        if is_array_index(&k) {
            idx.push((k.parse().unwrap_or(0), k, v));
        } else {
            rest.push((k, v));
        }
    }
    idx.sort_by_key(|(n, _, _)| *n);
    idx.into_iter()
        .map(|(_, k, v)| (k, v))
        .chain(rest)
        .collect()
}

/// `{ ...target, ...v }`: copies `v`'s own enumerable properties into `target` (strings and
/// arrays spread their indices; other primitives and null add nothing). An existing key keeps
/// its position. Limit: a string's astral characters are copied whole at their first UTF-16
/// index (JS would copy two lone surrogates).
pub fn spread_into(target: &mut Map<String, Value>, v: &Value) {
    match v {
        Value::Object(o) => {
            for (k, val) in order_keys(o.clone()) {
                target.insert(k, val);
            }
        }
        Value::Array(a) => {
            for (i, val) in a.iter().enumerate() {
                target.insert(i.to_string(), val.clone());
            }
        }
        Value::String(s) => {
            let mut i = 0usize;
            for c in s.chars() {
                target.insert(i.to_string(), Value::String(c.to_string()));
                i += c.len_utf16();
            }
        }
        _ => {}
    }
    let ordered = order_keys(std::mem::take(target));
    *target = ordered;
}

/// `Date.parse` for the ECMAScript date-time string format (and the extended-year form):
/// `YYYY[-MM[-DD]][THH:mm[:ss[.sss…]][Z|±HH:mm|±HHmm]]`, `T`/`Z` in either case. Milliseconds
/// since the epoch, or NaN.
///
/// Limits (documented deviations from V8): date-times without an offset are read as UTC (JS
/// uses the local zone); V8's legacy fallback formats ("Nov 17 2026", "2026/11/17", "2026-1-17",
/// surrounding spaces) give NaN here.
pub fn date_parse(s: &str) -> f64 {
    parse_iso(s).unwrap_or(f64::NAN)
}

fn parse_iso(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = 0;
    let num = |i: &mut usize, n: usize| -> Option<i64> {
        let part = b.get(*i..*i + n)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        *i += n;
        std::str::from_utf8(part).ok()?.parse().ok()
    };
    let year = match b.first()? {
        b'+' | b'-' => {
            let neg = b[0] == b'-';
            i = 1;
            let y = num(&mut i, 6)?;
            if neg && y == 0 {
                return None;
            }
            if neg { -y } else { y }
        }
        _ => num(&mut i, 4)?,
    };
    let (mut month, mut day) = (1, 1);
    if b.get(i) == Some(&b'-') {
        i += 1;
        month = num(&mut i, 2)?;
        if b.get(i) == Some(&b'-') {
            i += 1;
            day = num(&mut i, 2)?;
        }
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut ms_of_day = 0i64;
    let mut offset_min = 0i64;
    if i < b.len() {
        if b[i] != b'T' && b[i] != b't' {
            return None;
        }
        i += 1;
        let h = num(&mut i, 2)?;
        if b.get(i) != Some(&b':') {
            return None;
        }
        i += 1;
        let m = num(&mut i, 2)?;
        let mut sec = 0;
        let mut ms = 0;
        if b.get(i) == Some(&b':') {
            i += 1;
            sec = num(&mut i, 2)?;
            if b.get(i) == Some(&b'.') {
                i += 1;
                let start = i;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                if i == start {
                    return None;
                }
                let frac = &s[start..i];
                let first3: String = frac.chars().chain("00".chars()).take(3).collect();
                ms = first3.parse().ok()?;
            }
        }
        if h > 24 || m > 59 || sec > 59 || (h == 24 && (m > 0 || sec > 0 || ms > 0)) {
            return None;
        }
        ms_of_day = ((h * 60 + m) * 60 + sec) * 1000 + ms;
        match b.get(i) {
            None => {}
            Some(b'Z' | b'z') => i += 1,
            Some(c @ (b'+' | b'-')) => {
                let neg = *c == b'-';
                i += 1;
                let oh = num(&mut i, 2)?;
                if b.get(i) == Some(&b':') {
                    i += 1;
                }
                let om = num(&mut i, 2)?;
                if oh > 23 || om > 59 {
                    return None;
                }
                offset_min = if neg { -(oh * 60 + om) } else { oh * 60 + om };
            }
            _ => return None,
        }
        if i != b.len() {
            return None;
        }
    }
    let days = days_from_civil(year, month, 1) + (day - 1);
    let t = days * 86_400_000 + ms_of_day - offset_min * 60_000;
    if t.abs() > 8_640_000_000_000_000 {
        return None;
    }
    Some(t as f64)
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_strings() {
        for (x, s) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-12.0, "-12"),
            (0.1, "0.1"),
            (1.5, "1.5"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (123456789012345680000.0, "123456789012345680000"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
        ] {
            assert_eq!(number_to_string(x), s, "{x}");
        }
    }

    #[test]
    fn string_numbers() {
        let cases: [(&str, f64); 13] = [
            ("", 0.0),
            ("  ", 0.0),
            (" 12 ", 12.0),
            ("0x1A", 26.0),
            ("1e3", 1000.0),
            ("-1.5", -1.5),
            (".5", 0.5),
            ("5.", 5.0),
            ("Infinity", f64::INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
            ("0b101", 5.0),
            ("0o17", 15.0),
            ("+3", 3.0),
        ];
        for (s, x) in cases {
            assert_eq!(string_to_number(s), x, "{s:?}");
        }
        for s in [
            "abc", "1e", "inf", "nan", "infinity", "1_000", "-0x1", "12px", ".",
        ] {
            assert!(string_to_number(s).is_nan(), "{s:?}");
        }
    }

    #[test]
    fn fixed_and_round() {
        assert_eq!(to_fixed(1.25, 1), "1.3");
        assert_eq!(to_fixed(1.5, 1), "1.5");
        assert_eq!(to_fixed(9.96, 1), "10.0");
        assert_eq!(to_fixed(1.45, 1), "1.4"); // 1.45 is 1.4499999… exactly
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.49999999999999994), 0.0);
        assert!(round(-0.4).is_sign_negative());
    }

    #[test]
    fn dates() {
        assert_eq!(date_parse("2026-11-17"), 1_794_873_600_000.0);
        assert_eq!(date_parse("2026"), 1_767_225_600_000.0);
        assert_eq!(date_parse("2026-11"), 1_793_491_200_000.0);
        assert_eq!(date_parse("2026-02-30"), 1_772_409_600_000.0);
        assert_eq!(date_parse("+002026-11-17"), 1_794_873_600_000.0);
        assert_eq!(date_parse("-000001-01-01"), -62_198_755_200_000.0);
        assert_eq!(date_parse("2026-11-17T24:00:00Z"), 1_794_960_000_000.0);
        assert_eq!(date_parse("2026-11-17T10:00:00.5Z"), 1_794_909_600_500.0);
        assert_eq!(
            date_parse("2026-11-17T10:00:00.123456Z"),
            1_794_909_600_123.0
        );
        assert_eq!(date_parse("2026-11-17t10:00z"), 1_794_909_600_000.0);
        assert_eq!(date_parse("2026-11-17T10:00:00+01:00"), 1_794_906_000_000.0);
        assert_eq!(date_parse("2026-11-17T10:00:00+0100"), 1_794_906_000_000.0);
        for s in [
            "x",
            "2026-02-32",
            "2026-13-01",
            "2026-11-17T24:00:01Z",
            "2026-11-17T10",
            "20261117",
            "2026-00-10",
            "2026-11-00",
            "2026-11-17T10:60Z",
            "not a date",
        ] {
            assert!(date_parse(s).is_nan(), "{s}");
        }
    }

    #[test]
    fn key_order_and_spread() {
        let mut m = Map::new();
        m.insert("b".into(), Value::from(1));
        m.insert("10".into(), Value::from(2));
        m.insert("2".into(), Value::from(3));
        m.insert("01".into(), Value::from(4));
        let keys: Vec<String> = order_keys(m).keys().cloned().collect();
        assert_eq!(keys, ["2", "10", "b", "01"]);
        let mut t = Map::new();
        t.insert("x".into(), Value::from(1));
        spread_into(&mut t, &Value::from("ab"));
        let keys: Vec<String> = t.keys().cloned().collect();
        assert_eq!(keys, ["0", "1", "x"]);
        assert_eq!(
            string_of(Some(&serde_json::json!([1, null, [2, "a"]]))),
            "1,,2,a"
        );
        assert_eq!(string_of(Some(&serde_json::json!({}))), "[object Object]");
        assert_eq!(string_of(None), "undefined");
    }
}
