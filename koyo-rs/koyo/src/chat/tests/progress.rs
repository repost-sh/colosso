//! The details card's progress: the plan the agent keeps of a chat's task, its phases opening on a
//! click, and what the card says while nothing runs the plan.

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext, VisualTestContext, px};
use koyo_agent::protocol::ids::Time;
use koyo_agent::protocol::methods::ThreadSnapshot;
use koyo_agent::protocol::progress::{Plan, PlanPhase, PlanStep, StepStatus};
use koyo_agent::protocol::requests::RequestStatus;

use super::super::view::ChatView;
use super::AT;
use super::asks::request;
use super::drawing::{card_up, settled, shell_with_chat, shows};
use crate::settings::{self, DetailsItem};
use crate::test_support::{click, draw};

fn step(id: &str, status: StepStatus, detail: Option<&str>) -> PlanStep {
    PlanStep {
        id: id.into(),
        title: format!("Step {id} of the ledger fix"),
        detail: detail.map(Into::into),
        status,
        started_at: Some(Time(AT.into())),
        ended_at: None,
    }
}

fn phase(id: &str, status: StepStatus, steps: Vec<PlanStep>) -> PlanPhase {
    PlanPhase {
        step: PlanStep {
            title: format!("Phase {id} of the ledger fix"),
            detail: Some(format!("What phase {id} delivers.")),
            ..step(id, status, None)
        },
        steps,
    }
}

/// A plan one phase into its task: the second runs, the third waits its turn.
fn plan() -> Plan {
    use StepStatus::{Done, Pending, Running, Skipped};
    Plan {
        phases: vec![
            phase("1", Done, Vec::new()),
            phase(
                "2",
                Running,
                vec![
                    step("2.1", Done, None),
                    step("2.2", Running, Some("Two hundred runs pass.")),
                    step("2.3", Skipped, None),
                ],
            ),
            phase(
                "3",
                Pending,
                vec![step("3.1", Pending, None), step("3.2", Pending, None)],
            ),
        ],
        now: Some("running the suite".into()),
        started_at: Some(Time(AT.into())),
        updated_at: Time(AT.into()),
    }
}

/// A window showing a settled chat with `shown`'s plan, its card up and its progress open.
fn card_with(
    name: &str,
    change: impl FnOnce(&mut ThreadSnapshot),
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<ChatView>) {
    let (window, view) = closed_card_with(name, change, cx);
    cx.update(|cx| settings::update(cx, |settings| settings.chat_details.progress_open = true));
    draw(window, cx);
    (window, view)
}

/// A window showing a settled chat with `shown`'s plan, its card up as it comes.
fn closed_card_with(
    name: &str,
    change: impl FnOnce(&mut ThreadSnapshot),
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<ChatView>) {
    let mut shown = settled("0001");
    change(&mut shown);
    let (window, _, view, _) = shell_with_chat(name, shown, cx);
    card_up(window, &view, cx);
    draw(window, cx);
    (window, view)
}

fn has_plan(shown: &mut ThreadSnapshot) {
    shown.thread.progress = Some(plan());
}

#[gpui_kit::test]
fn the_card_shows_a_chats_plan_and_nothing_of_one_it_has_none_of(cx: &mut TestAppContext) {
    let (window, _) = card_with("chat-progress-none", |_| {}, cx);
    assert!(!shows("chat-details-progress", window, cx), "no plan");

    let (window, _) = card_with("chat-progress-plan", has_plan, cx);
    assert!(shows("chat-details-progress", window, cx));
    for phase in [
        "chat-details-progress-1",
        "chat-details-progress-2",
        "chat-details-progress-3",
    ] {
        assert!(shows(phase, window, cx), "{phase} has its line");
    }
    assert!(
        shows("chat-details-progress-2.1", window, cx),
        "the running phase is open"
    );
    assert!(
        !shows("chat-details-progress-3.1", window, cx),
        "the pending one isn't"
    );
}

/// Progress is the card's last part, which says how far the plan has come and opens on a click to
/// its phases, once a person wants them: a card full of a plan's lines pushes the rest out of sight.
#[gpui_kit::test]
fn progress_comes_last_and_closed_and_opens_on_a_click_which_is_kept(cx: &mut TestAppContext) {
    assert_eq!(DetailsItem::ALL.last(), Some(&DetailsItem::Progress), "and so in its menu");
    let (window, _) = closed_card_with("chat-progress-closed", has_plan, cx);
    let bounds = |what: &'static str, cx: &mut TestAppContext| {
        VisualTestContext::from_window(window, cx).debug_bounds(what)
    };
    let head = bounds("chat-details-progress", cx).expect("the plan's head shows");
    let changes = bounds("chat-details-changes", cx).expect("the changes show");
    let card = bounds("chat-details-card", cx).expect("the card is up");
    assert!(changes.bottom() < head.top(), "{changes:?} over {head:?}");
    assert!(card.bottom() - head.bottom() < px(10.), "nothing under it: {head:?} in {card:?}");
    for closed in [
        "chat-details-progress-1",
        "chat-details-progress-2.1",
        "chat-details-progress-paused",
    ] {
        assert!(!shows(closed, window, cx), "{closed} waits for a click");
    }

    click(window, "chat-details-progress", cx);
    assert!(shows("chat-details-progress-1", window, cx));
    assert!(shows("chat-details-progress-2.1", window, cx), "the running phase is open");
    assert!(shows("chat-details-progress-paused", window, cx));
    let kept = cx.update(|cx| settings::get(cx).chat_details.progress_open);
    assert!(kept, "settings keep it open");
    cx.simulate_keystrokes(window, "cmd-shift-b");
    draw(window, cx);
    cx.simulate_keystrokes(window, "cmd-shift-b");
    draw(window, cx);
    assert!(shows("chat-details-progress-1", window, cx), "open as the card comes up again");

    click(window, "chat-details-progress", cx);
    assert!(!shows("chat-details-progress-1", window, cx), "closed again");
    assert!(shows("chat-details-progress", window, cx), "to its head");
}

#[gpui_kit::test]
fn a_click_opens_a_phase_to_its_sub_steps_and_closes_one_again(cx: &mut TestAppContext) {
    let (window, _) = card_with("chat-progress-click", has_plan, cx);
    click(window, "chat-details-progress-3", cx);
    assert!(shows("chat-details-progress-3.1", window, cx));
    assert!(shows("chat-details-progress-3.2", window, cx));

    click(window, "chat-details-progress-2", cx);
    assert!(
        !shows("chat-details-progress-2.1", window, cx),
        "a person's click holds over the running phase's opening"
    );
    draw(window, cx);
    assert!(
        shows("chat-details-progress-3.1", window, cx),
        "what's opened stays so"
    );
}

#[gpui_kit::test]
fn progress_turned_off_takes_the_plan_off_the_card(cx: &mut TestAppContext) {
    let (window, _) = card_with("chat-progress-off", has_plan, cx);
    cx.update(|cx| settings::update(cx, |settings| settings.chat_progress = false));
    draw(window, cx);
    assert!(!shows("chat-details-progress", window, cx));

    cx.update(|cx| settings::update(cx, |settings| settings.chat_progress = true));
    draw(window, cx);
    assert!(shows("chat-details-progress", window, cx), "on again");
    cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.chat_details.flip(DetailsItem::Progress);
        });
    });
    draw(window, cx);
    assert!(
        !shows("chat-details-progress", window, cx),
        "taken off the card"
    );
}

/// A plan left with a step running when its turn ended reads as live work, which it isn't.
#[gpui_kit::test]
fn a_plan_nothing_runs_says_why(cx: &mut TestAppContext) {
    let (window, _) = card_with("chat-progress-paused", has_plan, cx);
    assert!(shows("chat-details-progress-paused", window, cx));
    assert!(!shows("chat-details-progress-now", window, cx));

    let (window, _) = card_with(
        "chat-progress-asking",
        |shown| {
            has_plan(shown);
            shown.requests = vec![request("q", RequestStatus::Pending, false, 9)];
        },
        cx,
    );
    assert!(shows("chat-details-progress-waiting", window, cx));
    assert!(!shows("chat-details-progress-paused", window, cx));

    let (window, _) = card_with(
        "chat-progress-working",
        |shown| {
            has_plan(shown);
            shown.beside.tasks = 1;
        },
        cx,
    );
    assert!(shows("chat-details-progress-now", window, cx));
    assert!(!shows("chat-details-progress-paused", window, cx));
}

#[gpui_kit::test]
fn the_cards_controls_say_how_far_the_plan_has_come_while_it_is_down(cx: &mut TestAppContext) {
    let (window, view) = card_with("chat-progress-pill", has_plan, cx);
    assert!(!shows("chat-details-share", window, cx), "the card says it");

    cx.simulate_keystrokes(window, "cmd-shift-b");
    draw(window, cx);
    assert!(!view.read_with(cx, ChatView::details_up));
    assert!(shows("chat-details-share", window, cx));

    click(window, "chat-details-share", cx);
    assert!(
        view.read_with(cx, ChatView::details_up),
        "it brings the card up"
    );
}
