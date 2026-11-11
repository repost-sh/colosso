//! Port of claude-swap's `autoswitch.py` (0fe22a8): the auto-switch engine, which watches the
//! active account's usage and switches to another account before it hits a limit.
//!
//! The engine has no UI: it composes the switcher (through [`EngineHost`]) and evaluates its
//! policy once per [`AutoSwitchEngine::tick`], which reports what it did, the typed events the
//! app shows in the panel and as a notice, and how long to wait before the next tick. The app
//! owns the loop.
//!
//! The policy (`docs/claude-accounts.md` 5): when any of the active account's windows (5h and
//! 7d) reaches the line (`threshold`), switch, proactively, so the old account still works
//! while a running Claude Code picks the new one up (which makes the macOS ~30 s Keychain cache
//! harmless). A target must sit below the line and beat the active account by `hysteresis_pct`
//! points of headroom, so two accounts at the line never ping-pong; among those, weekly-first
//! takes the one whose weekly window resets soonest, headroom breaking ties. A
//! `cooldown_seconds` floor bounds the switch rate, bypassed only when the active account is at
//! its limit, and the engine never undoes its own last move while the account it left hasn't
//! recovered (`release.rs`). When every account is over the line, it moves to the one with the
//! most quota left, by a margin. Before activation the target's token is freshened (refreshed
//! if it expires within ten minutes, twice Claude Code's own buffer, so a running Claude Code's
//! re-read under its lock sees a fresh token and drops its own refresh); a target whose refresh
//! token is dead is quarantined instead. When the active account's usage is unreadable for
//! `unhealthy_ticks` ticks the engine fails over; an expired token on an idle machine holds
//! instead. Cooldown, quarantine and the health counters persist in `autoswitch_state.json`
//! under its own lock.
//!
//! Left out (`docs/claude-accounts.md` 3): the `best`, `consume-first` and
//! `priority-consume-first` strategies as strategies (what weekly-first shares with `best` is
//! here: the hysteresis gate, the escapes' ranking, the every-account-above ranking by room);
//! `run_loop`, `stop`, `wake`, `dry_run` and the `--once` exit codes (the app schedules ticks);
//! the weekly line and the per-model windows (`weekly_threshold`, `model`: Koyo sets neither,
//! so every window's line is the threshold and no model window binds); per-account thresholds
//! and priority tiers (`ThresholdMap` overrides, `_priority_ready`,
//! `_rank_priority_candidates`); API-key accounts; session mode (cswap skipped a target a live
//! `cswap run` session owned; Koyo has none); JSON output and printing.

mod delay;
mod events;
mod host;
mod perform;
mod rank;
mod release;
mod schedule;
mod settings;
mod state;
mod tick;
mod windows;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use crate::Env;
use crate::oauth::OAUTH_EXPIRY_BUFFER_MS;
use crate::poll_policy::{self, EXHAUSTED_INTERVAL_S};

pub use events::pct_label;
pub use events::{
    AllExhaustedEvent, AutoSwitchEvent, ErrorEvent, NoSwitchEvent, NoSwitchReason, PollEvent,
    QuarantineEvent, QuarantineReason, ReleaseReason, SwitchEvent, TickOutcome, TickReport,
    Trigger, UnquarantineEvent,
};
pub use host::EngineHost;
pub use settings::{
    AutoSwitchSettings, DEFAULT_THRESHOLD, THRESHOLD_MAX, THRESHOLD_MIN, clamp_threshold,
};

/// The models whose per-model window binds: none (Koyo has no `model` setting).
const MODELS: &[&str] = &[];

/// Freshen a target whose access token expires within this: twice Claude Code's own 5-minute
/// refresh buffer, so its "abort the refresh if not expired" re-read after its lock holds with
/// margin after our swap.
const FRESHEN_BUFFER_MS: f64 = 2. * OAUTH_EXPIRY_BUFFER_MS;

/// The longest wait around a known quota reset: the exhausted-account poll cadence, since a
/// provider can grant quota before the reported reset and a long wait must not suppress the
/// fetch that finds it.
const MAX_SLEEP_S: f64 = EXHAUSTED_INTERVAL_S;
/// The wait when blocked with no reset known, and while idle-holding.
const NO_RESET_FALLBACK_S: f64 = 300.;

/// The idle-hold cap, in elapsed time (the hold itself slows the cadence to
/// [`NO_RESET_FALLBACK_S`]): an owned and expired token normally means Claude Code is idle and
/// heals on next use, but a dead refresh token with an active user would look the same forever,
/// so after this long the engine counts unhealthy ticks again.
const IDLE_HOLD_MAX_S: f64 = 30. * 60.;

/// The reset-axis margin of the no-return bar's release: the account left counts as recovered
/// only once its binding reset moved at least this much nearer. Longer than one poll cycle, so
/// jitter on a reset timestamp can't release the bar.
const RECOVERY_HYSTERESIS_S: f64 = 300.;

/// How near a peer's binding reset must be for the failover release to trust it while the
/// active's own reset reads unknown. A 5h window can be up to 5h from resetting; 4h keeps most of
/// a cycle inside.
const RECOVERY_HORIZON_S: f64 = 4. * 3600.;

/// The anti-flap margin when every account is over the line, as a ratio of the active's
/// headroom (strictly-more is no margin: one point moves the engine, the target burns it back,
/// and it ping-pongs), capped at [`SPENT_HEADROOM_PCT`] points (uncapped, an active holding 10
/// points refused a peer holding 19 at threshold 80). The no-return bar, not this ratio, keeps
/// the move one-way.
const HORIZON_HEADROOM_RATIO: f64 = 2.;

/// A headroom edge smaller than this is noise: a point is under ten minutes of work, less than
/// two poll intervals. It caps the margin above; the no-return release asks the account left to
/// have gained at least this much.
const SPENT_HEADROOM_PCT: f64 = 3.;

/// What a tick leaves for the wait after it, reset at the start of every tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Wait {
    /// The normal cadence.
    #[default]
    Interval,
    /// Every candidate exhausted: until this reset (with its slack), within the bounds.
    Until(f64),
    /// Blocked on what won't change soon (no candidates, or every one exhausted with no reset
    /// known), or an idle hold: crawl.
    Long,
}

/// The active row's next poll, for the wait after a tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum ActivePlan {
    /// No active row was read, or it carries no plan.
    #[default]
    Unknown,
    /// As the tick's collection left it.
    Kept(f64),
    /// A switch replanned the new active row: read it again.
    Replanned,
}

/// The threshold-policy auto-switcher over an [`EngineHost`].
pub struct AutoSwitchEngine {
    host: Arc<dyn EngineHost>,
    env: Env,
    settings: AutoSwitchSettings,
    /// The jitter's draw in `[0, 1)` (`poll_policy::random_unit`; a test fixes it).
    rng: fn() -> f64,
    unhealthy_ticks: i64,
    health_account: Option<String>,
    /// Idle-hold: the active token expired while Claude Code owns it (so is idle); the engine
    /// crawls instead of counting unhealthy ticks. Survives across ticks (the elapsed cap).
    idle_hold_since: Option<f64>,
    wait: Wait,
    active_plan: ActivePlan,
    /// The tick's events, handed over in its report.
    events: Vec<AutoSwitchEvent>,
}

impl AutoSwitchEngine {
    /// An engine on `env`'s store and clock (wall time: persisted cooldown stamps must survive
    /// processes), pinning the collector's poll inputs to `settings`.
    pub fn new(
        env: &Env,
        host: Arc<dyn EngineHost>,
        settings: AutoSwitchSettings,
    ) -> AutoSwitchEngine {
        let engine = AutoSwitchEngine {
            host,
            env: env.clone(),
            settings,
            rng: poll_policy::random_unit,
            unhealthy_ticks: 0,
            health_account: None,
            idle_hold_since: None,
            wait: Wait::default(),
            active_plan: ActivePlan::default(),
            events: Vec::new(),
        };
        engine.pin_poll_policy_inputs();
        engine
    }

    /// A threshold set mid-run, clamped as [`AutoSwitchSettings::new`] does: retargets the
    /// trigger from the next tick and the collector's cadence at once.
    pub fn set_threshold(&mut self, threshold: f64) {
        self.settings.threshold = clamp_threshold(threshold);
        self.pin_poll_policy_inputs();
    }

    /// The collector's urgent mode fires `threshold - 15` below the line, so the line decides
    /// when collection tightens.
    fn pin_poll_policy_inputs(&self) {
        self.host.set_poll_threshold(self.settings.threshold);
    }

    fn now(&self) -> f64 {
        self.env.now()
    }

    fn emit(&mut self, event: AutoSwitchEvent) {
        self.events.push(event);
    }
}

/// The engine going (auto-switch turned off) takes its pin with it, after its last tick: the
/// collector plans on the switcher's defaults again.
impl Drop for AutoSwitchEngine {
    fn drop(&mut self) {
        self.host.clear_poll_threshold();
    }
}
