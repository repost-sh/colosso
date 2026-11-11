//! Part of the port of claude-swap's `settings.py` (0fe22a8): the auto-switch engine's knobs
//! (`AutoSwitchSettings`) and the threshold's clamp from `SETTING_SPECS`.
//!
//! cswap reads these from `settings.json`; Koyo sets only the threshold (Settings → Accounts,
//! `docs/claude-accounts.md` 5) and keeps cswap's defaults for the rest, so there's no file and
//! no loader here. Left out with the settings Koyo never sets: the strategies but weekly-first
//! (the strategy isn't a field), `weekly_threshold` (every window's line is the threshold),
//! `model` (no per-model window binds, so cswap's model-name check goes too) and
//! `include_api_key_accounts` (Koyo manages no API-key account).

/// The threshold's range (`SETTING_SPECS["autoswitch.threshold"]`).
pub const THRESHOLD_MIN: f64 = 50.;
pub const THRESHOLD_MAX: f64 = 99.9;

/// cswap's defaults; the threshold's is the switcher's, which poll planning keys on until the
/// engine pins its own.
pub use crate::switcher::DEFAULT_THRESHOLD;
pub(super) const INTERVAL_SECONDS: f64 = 60.;
pub(super) const COOLDOWN_SECONDS: f64 = 300.;
pub(super) const HYSTERESIS_PCT: f64 = 10.;
pub(super) const UNHEALTHY_TICKS: i64 = 3;

/// Policy knobs for the auto-switch engine.
///
/// `threshold` is the switch line of every usage window (5h and 7d): the engine looks for a
/// better account once any of them is at or over it. A proactive candidate must itself sit
/// below the line (never land somewhere that re-triggers next tick) and beat the active
/// account by at least `hysteresis_pct` points of headroom, so two accounts hovering at the
/// line never ping-pong while a strictly better account is always taken. `cooldown_seconds`
/// is the least time between proactive switches; `unhealthy_ticks` the consecutive ticks with
/// the active account's usage unreadable before failing over.
#[derive(Clone, Debug, PartialEq)]
pub struct AutoSwitchSettings {
    pub(super) threshold: f64,
    pub(super) interval_seconds: f64,
    pub(super) cooldown_seconds: f64,
    pub(super) hysteresis_pct: f64,
    pub(super) unhealthy_ticks: i64,
}

impl AutoSwitchSettings {
    /// cswap's defaults with `threshold` clamped into its range, as cswap's `_clamped` reads a
    /// stored one: what the app builds from its setting.
    pub fn new(threshold: f64) -> AutoSwitchSettings {
        AutoSwitchSettings {
            threshold: clamp_threshold(threshold),
            interval_seconds: INTERVAL_SECONDS,
            cooldown_seconds: COOLDOWN_SECONDS,
            hysteresis_pct: HYSTERESIS_PCT,
            unhealthy_ticks: UNHEALTHY_TICKS,
        }
    }
}

/// cswap's `_clamped` for the threshold: a non-finite value is garbage and reads as the default
/// (a NaN line would silently never be crossed); anything else is held to 50-99.9.
pub fn clamp_threshold(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(THRESHOLD_MIN, THRESHOLD_MAX)
    } else {
        DEFAULT_THRESHOLD
    }
}
