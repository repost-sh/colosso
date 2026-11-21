//! What a chat's details card says of the plan its agent keeps (`docs/chat.md` 5.5): how far it
//! has come, how long its steps took and whether it's gone quiet, as Claude Code's progress pane
//! said it.

use chrono::{DateTime, Utc};

use super::{elapsed, parsed};
use crate::protocol::progress::{Plan, PlanStep, StepStatus};

/// How long a plan with work open goes unchanged before it's doubted.
const STALE_AFTER: i64 = 10 * 60;

/// A length of time as the card says it: "30s", "8m", "1h 05m".
pub fn span(seconds: u64) -> String {
    let minutes = seconds / 60;
    if minutes < 1 {
        format!("{seconds}s")
    } else if minutes < 60 {
        format!("{minutes}m")
    } else {
        format!("{}h {:02}m", minutes / 60, minutes % 60)
    }
}

/// A share as a percentage: "88%".
pub fn percent(share: f32) -> String {
    format!("{}%", (share.clamp(0., 1.) * 100.).round() as u8)
}

/// How far the plan has come: "2 of 5 · 40%", its phases done of those not skipped, and its
/// share, `Plan::share`.
pub fn plan_count(plan: &Plan, share: Option<f32>) -> String {
    let (done, counted) = plan.phases_done();
    match share {
        Some(share) => format!("{done} of {counted} · {}", percent(share)),
        None => format!("{done} of {counted}"),
    }
}

/// How long a step has run, or ran, to `now` while it goes on: nothing before it starts, nor for
/// one over in under a second.
pub fn step_time(step: &PlanStep, now: DateTime<Utc>) -> Option<String> {
    let ended = step.ended_at.as_ref();
    let started = (step.started_at.as_ref())
        .filter(|_| ended.is_some() || step.status != StepStatus::Pending)?;
    let seconds = elapsed(started, ended, now)?;
    (seconds >= 1).then(|| span(seconds.unsigned_abs()))
}

/// How long the task has gone on: from its first start to `now`, or to the last close once every
/// step is over.
pub fn plan_time(plan: &Plan, now: DateTime<Utc>) -> Option<String> {
    let started = parsed(plan.started_at.as_ref()?)?;
    let last = if plan.steps().all(|step| step.status.is_closed()) {
        (plan.steps())
            .filter_map(|step| step.ended_at.as_ref().and_then(parsed))
            .max()
    } else {
        None
    };
    let until = last.unwrap_or_else(|| now.fixed_offset());
    let seconds = (until - started).num_seconds().max(0);
    Some(span(seconds.unsigned_abs()))
}

/// "Plan last updated 12m ago", once it has gone unchanged long enough to doubt while work is open.
pub fn plan_stale(plan: &Plan, now: DateTime<Utc>) -> Option<String> {
    let age = (now.fixed_offset() - parsed(&plan.updated_at)?).num_seconds();
    (!plan.is_closed() && age >= STALE_AFTER)
        .then(|| format!("Plan last updated {} ago", span(age.unsigned_abs())))
}
