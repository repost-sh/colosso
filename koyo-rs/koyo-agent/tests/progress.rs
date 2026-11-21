//! How far a chat's plan has come and how its card says so, with no drawing.

use koyo_agent::protocol::ids::Time;
use koyo_agent::protocol::progress::StepStatus::{Blocked, Done, Pending, Running, Skipped};
use koyo_agent::protocol::progress::{Plan, PlanPhase, PlanStep, StepStatus};
use koyo_agent::words::{percent, plan_count, plan_stale, plan_time, span, step_time};

const AT: &str = "2026-11-03T18:00:00.000Z";

fn at(time: &str) -> Time {
    Time(format!("2026-11-03T{time}.000Z"))
}

#[expect(
    clippy::unwrap_used,
    reason = "a fixture's time that doesn't parse fails the test at once"
)]
fn now(time: &str) -> chrono::DateTime<chrono::Utc> {
    at(time).0.parse().unwrap()
}

fn step(id: &str, status: StepStatus) -> PlanStep {
    PlanStep {
        id: id.into(),
        title: format!("Step {id}"),
        detail: None,
        status,
        started_at: None,
        ended_at: None,
    }
}

/// A step that started and ended when it says.
fn timed(status: StepStatus, started: Option<&str>, ended: Option<&str>) -> PlanStep {
    PlanStep {
        started_at: started.map(at),
        ended_at: ended.map(at),
        ..step("1", status)
    }
}

fn phase(id: &str, status: StepStatus, steps: &[StepStatus]) -> PlanPhase {
    PlanPhase {
        step: PlanStep {
            title: format!("Phase {id}"),
            ..step(id, status)
        },
        steps: (steps.iter().enumerate())
            .map(|(at, status)| step(&format!("{id}.{at}"), *status))
            .collect(),
    }
}

fn plan(phases: Vec<PlanPhase>) -> Plan {
    Plan {
        phases,
        now: None,
        started_at: None,
        updated_at: Time(AT.into()),
    }
}

#[test]
fn each_phase_counts_the_same_and_an_open_one_by_its_done_sub_steps() {
    let half = plan(vec![
        phase("1", Done, &[]),
        phase("2", Running, &[Done, Done, Done, Running]),
    ]);
    assert_eq!(half.share(), Some(0.875));
    assert_eq!(percent(half.share().unwrap()), "88%");
    assert_eq!(plan_count(&half, half.share()), "1 of 2 · 88%");

    let skips = plan(vec![
        phase("1", Done, &[]),
        phase("2", Running, &[Done, Skipped, Running]),
        phase("3", Skipped, &[]),
    ]);
    assert_eq!(
        skips.share(),
        Some(0.75),
        "a skipped phase leaves the total, a skipped sub-step its phase's"
    );
    assert_eq!(plan_count(&skips, skips.share()), "1 of 2 · 75%");

    let open = plan(vec![
        phase("1", Pending, &[]),
        phase("2", Blocked, &[]),
        phase("3", Running, &[Skipped, Skipped]),
    ]);
    assert_eq!(open.share(), Some(0.), "an open phase alone does nothing");
    assert!(!open.is_closed());

    let none = plan(vec![phase("1", Skipped, &[])]);
    assert_eq!(none.share(), None, "nothing counts");
    assert_eq!(plan_count(&none, none.share()), "0 of 0");
    assert!(none.is_closed());
}

#[test]
fn a_done_phase_is_whole_whatever_its_sub_steps_say() {
    let whole = plan(vec![phase("1", Done, &[Done, Skipped])]);
    assert_eq!(whole.share(), Some(1.));
    assert_eq!(whole.phases[0].steps_done(), (1, 1));
    assert_eq!(plan_count(&whole, whole.share()), "1 of 1 · 100%");
    assert!(whole.is_closed());
}

#[test]
fn times_read_as_the_progress_pane_says_them() {
    assert_eq!(span(30), "30s");
    assert_eq!(span(8 * 60 + 59), "8m");
    assert_eq!(span(3600 + 5 * 60), "1h 05m");

    let later = now("18:10:00");
    let (started, ended) = (Some("18:00:00"), Some("18:02:30"));
    assert_eq!(
        step_time(&timed(Done, started, ended), later).as_deref(),
        Some("2m")
    );
    assert_eq!(
        step_time(&timed(Running, started, None), later).as_deref(),
        Some("10m"),
        "a running step counts to now"
    );
    assert_eq!(step_time(&timed(Pending, None, None), later), None);
    assert_eq!(
        step_time(&timed(Pending, started, None), later),
        None,
        "not under way"
    );
    assert_eq!(
        step_time(&timed(Skipped, None, ended), later),
        None,
        "skipped without running"
    );
    assert_eq!(
        step_time(&timed(Done, started, started), later),
        None,
        "over at once"
    );
}

#[test]
fn the_run_counts_from_its_first_start_to_its_last_close() {
    let mut running = plan(vec![
        phase("1", Done, &[]),
        phase("2", Running, &[Done, Running]),
    ]);
    assert_eq!(plan_time(&running, now("18:30:00")), None, "not started");
    running.started_at = Some(at("18:00:00"));
    running.phases[0].step.ended_at = Some(at("18:05:00"));
    assert_eq!(
        plan_time(&running, now("19:12:00")).as_deref(),
        Some("1h 12m")
    );

    let mut over = running.clone();
    over.phases[1].step.status = Done;
    over.phases[1].step.ended_at = Some(at("18:20:00"));
    over.phases[1].steps[1].status = Done;
    over.phases[1].steps[1].ended_at = Some(at("18:20:00"));
    assert_eq!(
        plan_time(&over, now("19:12:00")).as_deref(),
        Some("20m"),
        "a finished task stops its clock"
    );
}

#[test]
fn a_plan_left_alone_with_work_open_is_doubted_after_ten_minutes() {
    let mut open = plan(vec![phase("1", Running, &[])]);
    open.updated_at = at("18:00:00");
    assert_eq!(plan_stale(&open, now("18:09:59")), None);
    assert_eq!(
        plan_stale(&open, now("18:12:00")).as_deref(),
        Some("Plan last updated 12m ago")
    );
    open.phases[0].step.status = Done;
    assert_eq!(
        plan_stale(&open, now("18:12:00")),
        None,
        "nothing's left to do"
    );
}
