//! Python's view of a JSON value, for the rules claude-swap (0fe22a8) writes over dicts: which
//! values `isinstance(v, (int, float))` takes for a number (a bool too, as Python's bool is an
//! int), which are truthy, what `int(v or 0)` and `float(v)` give. The usage, store and plan
//! modules read cswap's files and answers through these, so a value cswap would take or refuse
//! is taken or refused here.

use serde_json::Value;

/// `isinstance(v, (int, float))`, as `float(v)`: a number, or a bool as 1 or 0.
pub(crate) fn py_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::Bool(flag) => Some(f64::from(u8::from(*flag))),
        _ => None,
    }
}

/// A number and not a bool, where cswap rules a bool out (`_is_number`, `_window_entry`).
pub(crate) fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        _ => None,
    }
}

/// [`py_number`] of an optional value: cswap's `_num_or_none(row.get(key))`.
pub(crate) fn num_or_none(value: Option<&Value>) -> Option<f64> {
    value.and_then(py_number)
}

/// Python's truthiness.
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// [`truthy`] of an optional value: `bool(d.get(key))`.
pub(crate) fn truthy_at(value: Option<&Value>) -> bool {
    value.is_some_and(truthy)
}

/// `int(v or 0)` for a stored counter: a number truncated, a bool as 1 or 0, anything else 0.
pub(crate) fn py_int(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|n| n.trunc() as i64))
            .unwrap_or(0),
        Some(Value::Bool(flag)) => i64::from(*flag),
        _ => 0,
    }
}

/// `float(v)`: a number, a bool, or a string Python's `float` reads; `None` where it raises.
pub(crate) fn py_float(value: &Value) -> Option<f64> {
    match value {
        Value::String(text) => text.trim().parse().ok(),
        other => py_number(other),
    }
}

/// The string at `key`, when it is one.
pub(crate) fn str_at<'a>(map: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    map.get(key).and_then(Value::as_str)
}
