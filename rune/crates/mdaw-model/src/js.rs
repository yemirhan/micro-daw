//! JavaScript semantics the ported code depends on, shared by every module of this crate:
//!
//! - strings: `String.prototype.trim` / `\s`, UTF-16 `length` and `<` ordering;
//! - numbers: `Math.round`, `Math.min` / `Math.max` (NaN propagates, `+0 > -0`), `Math.log`
//!   (V8's fdlibm port), `Math.pow`, `Number(string)`, `String(number)`, `toFixed`;
//! - values: `ToNumber`, truthiness, `String(value)`, `SameValueZero`, `JSON.stringify` number
//!   form, object key order and object spread;
//! - `Date.parse` (ISO subset).

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

/// `Math.round`: halves round toward +∞, exact for every double (unlike `(x + 0.5).floor()`,
/// which rounds 0.49999999999999994 up and is off for odd integers above 2^52). NaN / ±∞ pass
/// through; results in [-0.5, 0] keep JS's -0.
pub fn round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let r = x.floor();
    let out = if x - r >= 0.5 { r + 1.0 } else { r };
    if out == 0.0 && x < 0.0 { -0.0 } else { out }
}

/// `Math.max(a, b)`: NaN if either side is NaN; `+0` wins over `-0`.
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        return if a.is_sign_positive() { a } else { b };
    }
    if a > b { a } else { b }
}

/// `Math.min(a, b)`: NaN if either side is NaN; `-0` wins over `+0`.
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == b {
        return if a.is_sign_negative() { a } else { b };
    }
    if a < b { a } else { b }
}

/// `Math.min(hi, Math.max(lo, v))` (NaN propagates; never panics when `lo > hi`).
pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    min(hi, max(lo, v))
}

/// JS `a >= b` (false when either is NaN). Named so ported `!(a >= b)` tests, whose NaN
/// behaviour must be kept, read as such.
pub fn ge(a: f64, b: f64) -> bool {
    a >= b
}

/// `String(Math.round(x))`.
pub fn round_string(x: f64) -> String {
    number_to_string(round(x))
}

/// `Math.pow`: current V8 calls the C library's `pow` (golden: the platform `pow` matched V8 on
/// all 6000 inputs, fdlibm's `libm::pow` did not), which is what `f64::powf` calls.
pub fn pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}

/// `Math.log`: V8's `base::ieee754::log`, a port of fdlibm `__ieee754_log` (FreeBSD e_log.c).
/// Ported literally because both the platform `ln` and the `libm` crate (musl's simplified
/// variant) differ from it by 1 ulp on some inputs (e.g. ln(185)).
///
/// The reference Node (V8 14.6, macOS arm64) is compiled by clang with `-ffp-contract=on`, so
/// each `a*b + c` inside one C expression is a fused multiply-add; `fused = true` reproduces that
/// (0 mismatches over 7000 golden inputs; the plain fdlibm order differs on 7 of them — that is
/// what an x86-64 Node build would return).
pub fn log(x: f64) -> f64 {
    log_impl(x, true)
}

#[allow(clippy::excessive_precision)] // fdlibm's literals, kept verbatim
fn log_impl(mut x: f64, fused: bool) -> f64 {
    // a*b + c as one rounding (clang -ffp-contract=on) or two.
    let fma = |a: f64, b: f64, c: f64| if fused { a.mul_add(b, c) } else { a * b + c };
    const LN2_HI: f64 = 6.93147180369123816490e-01;
    const LN2_LO: f64 = 1.90821492927058770002e-10;
    const TWO54: f64 = 1.80143985094819840000e+16;
    const LG1: f64 = 6.666666666666735130e-01;
    const LG2: f64 = 3.999999999940941908e-01;
    const LG3: f64 = 2.857142874366239149e-01;
    const LG4: f64 = 2.222219843214978396e-01;
    const LG5: f64 = 1.818357216161805012e-01;
    const LG6: f64 = 1.531383769920937332e-01;
    const LG7: f64 = 1.479819860511658591e-01;
    let hi = |x: f64| (x.to_bits() >> 32) as u32 as i32;
    let lo = |x: f64| x.to_bits() as u32;
    let mut hx = hi(x);
    let lx = lo(x);
    let mut k: i32 = 0;
    if hx < 0x0010_0000 {
        // x < 2**-1022
        if ((hx & 0x7fff_ffff) as u32 | lx) == 0 {
            return f64::NEG_INFINITY; // log(+-0) = -inf
        }
        if hx < 0 {
            return f64::NAN; // log(-#) = NaN
        }
        k -= 54;
        x *= TWO54; // subnormal number, scale up x
        hx = hi(x);
    }
    if hx >= 0x7ff0_0000 {
        return x + x;
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000f_ffff;
    let i = (hx + 0x95f64) & 0x10_0000;
    // normalize x or x/2
    x = f64::from_bits(((((hx | (i ^ 0x3ff0_0000)) as u32) as u64) << 32) | lo(x) as u64);
    k += i >> 20;
    let f = x - 1.0;
    if (0x000f_ffff & (2 + hx)) < 3 {
        // -2**-20 <= f < 2**-20
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            let dk = k as f64;
            return fma(dk, LN2_HI, dk * LN2_LO);
        }
        let r = f * f * fma(-0.33333333333333333, f, 0.5);
        if k == 0 {
            return f - r;
        }
        let dk = k as f64;
        return fma(dk, LN2_HI, -(fma(-dk, LN2_LO, r) - f));
    }
    let s = f / (2.0 + f);
    let dk = k as f64;
    let z = s * s;
    let mut i = hx - 0x6147a;
    let w = z * z;
    let j = 0x6b851 - hx;
    let t1 = w * fma(w, fma(w, LG6, LG4), LG2);
    let t2 = z * fma(w, fma(w, fma(w, LG7, LG5), LG3), LG1);
    i |= j;
    let r = t2 + t1;
    if i > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 {
            f - fma(-s, hfsq + r, hfsq)
        } else {
            fma(dk, LN2_HI, -((hfsq - fma(s, hfsq + r, dk * LN2_LO)) - f))
        }
    } else if k == 0 {
        fma(-s, f - r, f)
    } else {
        fma(dk, LN2_HI, -(fma(s, f - r, -(dk * LN2_LO)) - f))
    }
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

/// JS truthiness of a JSON value (`None` = `undefined`, falsy).
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// `ToNumber` of a JSON value (`None` = `undefined` → NaN; arrays go through `String(array)`).
pub fn to_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => string_to_number(s),
        Some(Value::Array(a)) => match a.as_slice() {
            [] | [Value::Null] => 0.0,
            [x @ (Value::Number(_) | Value::String(_) | Value::Array(_))] => to_number(Some(x)),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// `SameValueZero` (Map keys, `Array.prototype.includes`).
pub fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
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
        assert_eq!(round(-0.49999999999999994), 0.0);
        assert!(round(-0.4).is_sign_negative());
        assert!(round(-0.5).is_sign_negative());
        assert!(round(-0.0).is_sign_negative());
        assert!(round(0.4).is_sign_positive());
        assert_eq!(round(-0.6), -1.0);
        assert_eq!(round(1.5), 2.0);
        // 2^52 + 1 is odd and above 2^52: x + 0.5 is not representable there.
        let big = 4_503_599_627_370_497.0;
        assert_eq!(round(big), big);
        assert_eq!(round(-big), -big);
        // 2^53 + 1 is not a double (it rounds to 2^53); 2^53 + 2 is.
        let p53 = 2f64.powi(53);
        assert_eq!(round(p53 + 1.0), p53);
        assert_eq!(round(p53 + 2.0), p53 + 2.0);
        assert_eq!(round(-(p53 + 2.0)), -(p53 + 2.0));
        assert!(round(f64::NAN).is_nan());
        assert_eq!(round(f64::INFINITY), f64::INFINITY);
        assert_eq!(round(f64::NEG_INFINITY), f64::NEG_INFINITY);
        assert_eq!(round_string(-0.2), "0");
        assert_eq!(round_string(2.5), "3");
    }

    #[test]
    fn min_max_log() {
        assert!(max(f64::NAN, 1.0).is_nan());
        assert!(min(1.0, f64::NAN).is_nan());
        assert!(max(-0.0, 0.0).is_sign_positive());
        assert!(max(0.0, -0.0).is_sign_positive());
        assert!(min(0.0, -0.0).is_sign_negative());
        assert!(min(-0.0, 0.0).is_sign_negative());
        assert_eq!(clamp(5.0, 0.0, 1.0), 1.0);
        assert!(clamp(f64::NAN, 0.0, 1.0).is_nan());
        assert!(!ge(f64::NAN, 0.0));
        assert_eq!(log(1.0), 0.0);
        assert_eq!(log(0.0), f64::NEG_INFINITY);
        assert!(log(-1.0).is_nan());
        assert_eq!(log(std::f64::consts::E), 1.0);
        // The plain fdlibm order agrees with the fused one away from the rare 1-ulp cases.
        assert_eq!(log_impl(2.0, false), log(2.0));
    }

    #[test]
    fn value_coercion() {
        use serde_json::json;
        let n = |v: Value| to_number(Some(&v));
        assert_eq!(n(json!(" 12 ")), 12.0);
        assert_eq!(n(json!([["3"]])), 3.0);
        assert!(n(json!([true])).is_nan());
        assert!(n(json!("0x+1")).is_nan());
        assert!(truthy(Some(&json!("0"))));
        assert!(!truthy(Some(&json!(0))));
        assert!(same_value_zero(&json!(1), &json!(1.0)));
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
