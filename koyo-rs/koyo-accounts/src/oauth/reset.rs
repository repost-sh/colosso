//! Part of the port of claude-swap's `oauth.py` (0fe22a8): reset times read and drawn,
//! `format_reset`, `format_countdown`, `reset_clock_string` and `fresh_reset_strings`, and the
//! ISO-8601 reader they and the store share, Python's `datetime.fromisoformat`.
//!
//! cswap reads the clock inside these (`datetime.now()`); here the caller passes `now` (epoch
//! seconds, `env.now()`), so a countdown is computed when it is drawn and tests set the time.
//! The clock string is in the Mac's time zone, as cswap's `astimezone()`.

use chrono::{
    DateTime, Datelike as _, FixedOffset, Local, NaiveDate, NaiveDateTime, NaiveTime,
    SecondsFormat, TimeZone as _, Utc,
};
use serde_json::{Map, Value};

/// What `datetime.fromisoformat` made of a string: a time with its offset, or a naive one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Iso {
    Aware(DateTime<FixedOffset>),
    Naive(NaiveDateTime),
}

/// Why a reset time couldn't be drawn, by the exception cswap raises: `ValueError` for a string
/// `fromisoformat` refuses, `TypeError` for a naive time or a value that isn't a string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetError {
    Value,
    Type,
}

impl ResetError {
    /// cswap's exception type name, its fallback error kind.
    pub fn kind(self) -> &'static str {
        match self {
            ResetError::Value => "ValueError",
            ResetError::Type => "TypeError",
        }
    }
}

/// `datetime.fromisoformat(text)`: `YYYY-MM-DD`, then any one character and `HH[:MM[:SS[.f]]]`
/// (or `HHMM[SS]`), then `Z` or `±HH[:MM[:SS[.f]]]`; `None` where Python raises.
pub fn parse_iso(text: &str) -> Option<Iso> {
    let date = NaiveDate::parse_from_str(text.get(..10)?, "%Y-%m-%d").ok()?;
    let mut rest = text.get(10..)?.chars();
    if rest.next().is_none() {
        return Some(Iso::Naive(date.and_time(NaiveTime::MIN)));
    }
    let rest = rest.as_str();
    let split = rest.find(['Z', 'z', '+', '-']).unwrap_or(rest.len());
    let (clock, zone) = rest.split_at(split);
    let at = date.and_time(time(clock)?);
    if zone.is_empty() {
        return Some(Iso::Naive(at));
    }
    let offset = offset(zone)?;
    offset.from_local_datetime(&at).single().map(Iso::Aware)
}

/// `HH[:MM[:SS[.ffffff]]]` or `HHMM[SS[.ffffff]]`.
fn time(clock: &str) -> Option<NaiveTime> {
    let (whole, fraction) = match clock.split_once(['.', ',']) {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (clock, None),
    };
    let parts = hms(whole)?;
    let micros = match fraction {
        Some(digits) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            let kept: String = digits.chars().chain("000000".chars()).take(6).collect();
            kept.parse().ok()?
        }
        Some(_) => return None,
        None => 0,
    };
    if fraction.is_some() && parts.len() != 3 {
        return None;
    }
    let field = |at: usize| parts.get(at).copied().unwrap_or(0);
    NaiveTime::from_hms_micro_opt(field(0), field(1), field(2), micros)
}

/// Two-digit fields, with or without colons: `HH`, `HH:MM`, `HHMM`, `HH:MM:SS`, `HHMMSS`.
fn hms(whole: &str) -> Option<Vec<u32>> {
    let fields: Vec<&str> = if whole.contains(':') {
        whole.split(':').collect()
    } else {
        (0..whole.len())
            .step_by(2)
            .map(|at| whole.get(at..at + 2))
            .collect::<Option<_>>()?
    };
    if fields.is_empty() || fields.len() > 3 {
        return None;
    }
    fields
        .iter()
        .map(|field| {
            (field.len() == 2 && field.bytes().all(|b| b.is_ascii_digit()))
                .then(|| field.parse().ok())
                .flatten()
        })
        .collect()
}

/// `Z`, or `±HH[:MM[:SS[.ffffff]]]` / `±HHMM[SS]`.
fn offset(zone: &str) -> Option<FixedOffset> {
    if zone.eq_ignore_ascii_case("z") {
        return FixedOffset::east_opt(0);
    }
    let (sign, body) = match zone.split_at_checked(1)? {
        ("+", body) => (1, body),
        ("-", body) => (-1, body),
        _ => return None,
    };
    let body = body.split_once('.').map_or(body, |(whole, _)| whole);
    let parts = hms(body)?;
    let field = |at: usize| i32::try_from(parts.get(at).copied().unwrap_or(0)).ok();
    let (hours, minutes, seconds) = (field(0)?, field(1)?, field(2)?);
    if hours > 23 || minutes > 59 || seconds > 59 {
        return None;
    }
    FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60 + seconds))
}

/// `datetime.fromisoformat(text).timestamp()`: a naive time is taken in the Mac's time zone,
/// as Python's is.
pub(crate) fn iso_timestamp(text: &str) -> Option<f64> {
    match parse_iso(text)? {
        Iso::Aware(at) => Some(epoch(&at)),
        Iso::Naive(at) => Local
            .from_local_datetime(&at)
            .earliest()
            .map(|at| epoch(&at)),
    }
}

fn epoch<Tz: chrono::TimeZone>(at: &DateTime<Tz>) -> f64 {
    at.timestamp() as f64 + f64::from(at.timestamp_subsec_micros()) / 1e6
}

/// `datetime.fromtimestamp(seconds, tz=timezone.utc)`, to the microsecond; `None` out of range.
pub(crate) fn utc_at(seconds: f64) -> Option<DateTime<Utc>> {
    let micros = (seconds * 1e6).round();
    if !micros.is_finite() {
        return None;
    }
    DateTime::from_timestamp_micros(micros as i64)
}

/// An epoch as ISO-8601 UTC to the second, `2024-01-01T00:00:00Z` (the fraction dropped, as
/// `strftime`'s `%S` does); `None` out of range.
pub(crate) fn iso_utc(seconds: f64) -> Option<String> {
    utc_at(seconds.floor()).map(|at| at.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// An epoch as Python's `datetime.fromtimestamp(ts, timezone.utc).isoformat()` with `Z`: to the
/// second, or the microsecond when there are any; `None` out of range.
pub(crate) fn iso_z(seconds: f64) -> Option<String> {
    utc_at(seconds).map(|at| {
        let precision = if at.timestamp_subsec_micros() == 0 {
            SecondsFormat::Secs
        } else {
            SecondsFormat::Micros
        };
        at.to_rfc3339_opts(precision, true)
    })
}

/// `(countdown, clock)` for a reset time in local time.
///
/// # Errors
/// [`ResetError::Value`] for a string `fromisoformat` refuses, [`ResetError::Type`] for a time
/// without an offset, which cswap can't subtract from an aware one.
pub fn format_reset(resets_at: &str, now: f64) -> Result<(String, String), ResetError> {
    let reset = match parse_iso(resets_at).ok_or(ResetError::Value)? {
        Iso::Aware(at) => at.with_timezone(&Utc),
        Iso::Naive(_) => return Err(ResetError::Type),
    };
    let now_utc = utc_at(now).ok_or(ResetError::Value)?;
    Ok((
        format_countdown(epoch(&reset) - now),
        reset_clock_string(reset, now_utc),
    ))
}

/// [`format_reset`] of a JSON value, which cswap passes as it came: a non-string is its
/// `TypeError`.
///
/// # Errors
/// As [`format_reset`].
pub(crate) fn format_reset_value(
    resets_at: &Value,
    now: f64,
) -> Result<(String, String), ResetError> {
    format_reset(resets_at.as_str().ok_or(ResetError::Type)?, now)
}

/// `"5d 12h"` / `"1h 59m"` / `"45m"`: the grammar of every countdown shown.
pub fn format_countdown(seconds: f64) -> String {
    let total = if seconds > 0. { seconds as u64 } else { 0 };
    let (days, remainder) = (total / 86_400, total % 86_400);
    let (hours, minutes) = (remainder / 3600, remainder % 3600 / 60);
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

/// The absolute reset time in local time: `"20:39"` the same day, else `"Jul 5 08:59"`.
pub fn reset_clock_string(reset: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let reset_local = reset.with_timezone(&Local);
    if reset_local.date_naive() == now.with_timezone(&Local).date_naive() {
        return reset_local.format("%H:%M").to_string();
    }
    format!(
        "{} {} {}",
        reset_local.format("%b"),
        reset_local.day(),
        reset_local.format("%H:%M")
    )
}

/// `(countdown, clock)` for one usage window, or `None` when unknown.
///
/// Recomputed from `resets_at` when drawn: strings cached at fetch time drift as the reading
/// ages (a countdown frozen 2 h ago overstates the wait by those 2 h, and a same-day "15:30"
/// starts meaning yesterday). A window stored without `resets_at`, or with one that can't be
/// read, falls back to its fetch-time strings: stale beats blank.
pub fn fresh_reset_strings(window: &Map<String, Value>, now: f64) -> Option<(String, String)> {
    if let Some(resets_at) = window.get("resets_at").filter(|v| super::value::truthy(v))
        && let Ok(strings) = format_reset_value(resets_at, now)
    {
        return Some(strings);
    }
    let clock = window.get("clock")?;
    let countdown = window
        .get("countdown")
        .map_or_else(|| "?".to_owned(), text_of);
    Some((countdown, text_of(clock)))
}

/// A stored string as itself, anything else as its JSON, as Python's f-string would show it.
fn text_of(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}
