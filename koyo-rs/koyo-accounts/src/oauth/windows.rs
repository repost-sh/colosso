//! Part of the port of claude-swap's `oauth.py` (0fe22a8): the windows that gate an account
//! (`relevant_windows`) and its headroom (`account_headroom`), the one source decisions,
//! scheduling and reset maths read, so a window that binds a decision is never invisible to
//! the scheduler.
//!
//! cswap's `relevant_windows` first asks whether a header-probe reading (`source: headers`) is
//! complete (`_header_reading_complete`); the probe isn't ported, so no reading here has that
//! source and the check is left out. A probe refusal's `rejected` entry is still read, as the
//! store keeps its deadline (`usage_store`).

use serde_json::Value;

use super::value::py_number;

/// The 5-hour window's label, the only window that isn't weekly: everything that treats it
/// apart (`lines::window_line`) keys on this.
pub const FIVE_HOUR_LABEL: &str = "5h";
/// The window a refused probe adds: 100% until the refusal ends.
pub const REJECTED_LABEL: &str = "rejected";

/// One window that gates an account: its label (`5h`, `7d`, a model's name, `rejected`), its
/// utilization in percent, and its reset as fetched (ISO-8601), if any.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub label: String,
    pub pct: f64,
    pub resets_at: Option<String>,
}

impl Window {
    pub fn new(label: &str, pct: f64, resets_at: Option<&str>) -> Window {
        Window {
            label: label.to_owned(),
            pct,
            resets_at: resets_at.map(str::to_owned),
        }
    }
}

fn resets_at(window: &serde_json::Map<String, Value>) -> Option<String> {
    window
        .get("resets_at")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Every window that gates this account: the reported 5-hour ("5h") and 7-day ("7d") ones (an
/// Ultra account has no 5-hour one; a window without a figure is skipped), then, when `models`
/// names any, each named per-model `scoped` window (matched without case on the display name;
/// `all` matches every one). `spend` is another axis and never counts. A `rejected` entry adds
/// a 100% window resetting when the refusal ends.
pub fn relevant_windows(usage: Option<&Value>, models: &[&str]) -> Vec<Window> {
    let Some(usage) = usage.and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut windows = Vec::new();
    for (key, label) in [("five_hour", FIVE_HOUR_LABEL), ("seven_day", "7d")] {
        if let Some(window) = usage.get(key).and_then(Value::as_object)
            && let Some(pct) = window.get("pct").and_then(py_number)
        {
            windows.push(Window {
                label: label.to_owned(),
                pct,
                resets_at: resets_at(window),
            });
        }
    }
    if !models.is_empty() {
        let wanted: Vec<String> = models.iter().map(|model| model.to_lowercase()).collect();
        let match_all = wanted.iter().any(|model| model == "all");
        if let Some(Value::Array(scoped)) = usage.get("scoped") {
            for window in scoped.iter().filter_map(Value::as_object) {
                let (Some(pct), Some(name)) = (
                    window.get("pct").and_then(py_number),
                    window.get("name").and_then(Value::as_str),
                ) else {
                    continue;
                };
                if match_all || wanted.contains(&name.to_lowercase()) {
                    windows.push(Window {
                        label: name.to_owned(),
                        pct,
                        resets_at: resets_at(window),
                    });
                }
            }
        }
    }
    if let Some(rejected) = usage.get("rejected").and_then(Value::as_object) {
        windows.push(Window {
            label: REJECTED_LABEL.to_owned(),
            pct: 100.,
            resets_at: resets_at(rejected),
        });
    }
    windows
}

/// The percentage left before this account hits a window: `100 - max(pct)` over
/// [`relevant_windows`], so `<= 0` is at or over a limit. A model maxed at 100% blocks that
/// model's work even with 5h/7d headroom, so for someone pinned to it it binds as hard.
/// `None` when there is no reading or no window: unknown, never skipped for it.
pub fn account_headroom(usage: Option<&Value>, models: &[&str]) -> Option<f64> {
    headroom_of(&relevant_windows(usage, models))
}

/// [`account_headroom`] over windows already read: `100 - max(pct)`, `None` without a window.
pub(crate) fn headroom_of(windows: &[Window]) -> Option<f64> {
    windows
        .iter()
        .map(|window| window.pct)
        .reduce(f64::max)
        .map(|max| 100. - max)
}
