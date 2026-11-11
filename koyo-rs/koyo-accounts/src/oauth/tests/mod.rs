//! Tests translated from claude-swap's `tests/test_oauth.py` and
//! `tests/test_oauth_weekly_only.py` (0fe22a8), named after cswap's. `urlopen` patches become
//! answers queued on the sandbox's `FakeHttp`; `datetime.now` patches become the `now` passed in.

mod refresh;
mod tokens;
mod usage;
mod weekly_only;
mod windows;

use chrono::{TimeZone as _, Utc};
use serde_json::Value;

use crate::Env;
use crate::http::Response;
use crate::oauth::{build_usage_result, request_usage_data};
use crate::testing::Sandbox;

/// Epoch seconds of a UTC time.
pub(super) fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> f64 {
    Utc.with_ymd_and_hms(y, mo, d, h, mi, s)
        .unwrap()
        .timestamp() as f64
}

/// cswap's `fixed_now`: 2026-03-23 12:00 UTC.
pub(super) fn fixed_now() -> f64 {
    utc(2026, 3, 23, 12, 0, 0)
}

/// cswap's `fetch_usage`, which the crate leaves out: the answer normalised, `None` on any
/// failure.
pub(super) fn fetch_usage(env: &Env, access_token: &str) -> Option<Value> {
    request_usage_data(env, access_token)
        .and_then(|data| build_usage_result(&data, env.now()))
        .ok()
        .flatten()
}

/// An answer with `status`, `headers` and `body`.
pub(super) fn answer(status: u16, headers: &[(&str, &str)], body: &str) -> Response {
    Response {
        status,
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        body: body.as_bytes().to_vec(),
    }
}

/// A sandbox whose clock reads `now`.
pub(super) fn sandbox_at(now: f64) -> Sandbox {
    let sandbox = Sandbox::new();
    sandbox.clock.set(now);
    sandbox
}

/// The header `name` of a request, as sent.
pub(super) fn header<'a>(request: &'a crate::http::Request, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// The number at `path` of a JSON value, for asserting on usage readings.
pub(super) fn num(value: &Value, path: &[&str]) -> Option<f64> {
    path.iter()
        .try_fold(value, |at, key| at.get(*key))
        .and_then(Value::as_f64)
}
