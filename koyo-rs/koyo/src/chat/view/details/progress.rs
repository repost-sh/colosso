//! The card's progress: the plan the agent keeps of the chat's task (`docs/chat.md` 5.5), how far
//! it has come in a bar, which opens on a click to its phases, each opening on a click to what it
//! covers and its sub-steps. A step the plan says runs is drawn still while the chat doesn't work,
//! as nothing runs it then, and the card says why.

use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, Context, Div, Hsla, InteractiveElement as _, ParentElement as _, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, div, px,
};
use koyo_agent::protocol::progress::{Plan, PlanPhase, PlanStep, StepStatus};
use koyo_agent::replica::Replica;
use koyo_agent::words::{percent, plan_count, plan_stale, plan_time, step_time};

use super::super::ChatView;
use super::{clickable, control_shell, gauge, meta};
use crate::chat::rows::chevron;
use crate::settings::{self, DetailsItem};
use crate::shell::icon;
use crate::theme::Palette;

/// What a line keeps from the card's edges, and between its mark and its title.
const PAD: f32 = 10.;
const SPACE: f32 = 8.;
/// How big a phase's mark is, and a sub-step's.
const MARK: f32 = 14.;
const STEP_MARK: f32 = 12.;
/// Where a phase's title starts, under which its detail and its sub-steps go.
const INDENT: f32 = PAD + MARK + SPACE;
/// The room for a line's chevron, kept on every line so their times line up.
const CHEVRON: f32 = 12.;
/// How tall a line of a title is, which its mark, count, time and chevron sit level with.
const LINE: f32 = 18.;

/// The plan the card shows: the chat's, while a person has the card show one.
pub(super) fn shown<'a>(replica: Option<&'a Replica>, cx: &App) -> Option<&'a Plan> {
    let settings = settings::get(cx);
    let on = settings.chat_progress && settings.chat_details.shows(DetailsItem::Progress);
    (replica?.thread.progress.as_ref()).filter(|_| on)
}

/// How far the plan has come, "40%", for the card's controls while the card is down and the
/// plan has work open.
pub(super) fn pill(replica: Option<&Replica>, p: Palette, cx: &App) -> Option<Stateful<Div>> {
    let plan = shown(replica, cx).filter(|plan| !plan.is_closed())?;
    let share = percent(plan.share()?);
    Some(
        control_shell("chat-details-share", p)
            .debug_selector(|| "chat-details-share".into())
            .px(px(6.))
            .text_xs()
            .child(share),
    )
}

/// The time the card is drawn at, and whether the chat works then.
#[derive(Clone, Copy)]
struct At {
    now: DateTime<Utc>,
    working: bool,
}

pub(super) fn section(
    view: &ChatView,
    replica: Option<&Replica>,
    p: Palette,
    cx: &Context<ChatView>,
) -> Option<Div> {
    let replica = replica?;
    let plan = shown(Some(replica), cx)?;
    let at = At {
        now: Utc::now(),
        working: replica.work().any(),
    };
    let open = settings::get(cx).chat_details.progress_open;
    Some(
        div()
            .flex()
            .flex_col()
            .child(head(plan, at, open, p))
            .when(open, |section| {
                let phases = plan.phases.iter();
                section
                    .children(phases.map(|phase| lines(view, Line::Phase(phase), at, p, cx)))
                    .children(foot(plan, replica, at, p))
            }),
    )
}

/// "Progress", how far the plan has come and for how long, and a bar of it, which fills green
/// once every phase is over; a click opens the section to the plan's phases, or closes it to this,
/// which settings keep.
fn head(plan: &Plan, at: At, open: bool, p: Palette) -> Stateful<Div> {
    let share = plan.share();
    let says = match plan_time(plan, at.now) {
        Some(time) => format!("{} · {time}", plan_count(plan, share)),
        None => plan_count(plan, share),
    };
    let says = div()
        .flex()
        .items_center()
        .gap(px(SPACE))
        .child(meta(says, p.muted))
        .child(folding(true, open, p));
    let fill = if plan.is_closed() { p.added } else { p.text };
    let head = gauge("Progress", says, share.unwrap_or(0.), fill, p)
        .rounded(px(8.))
        .debug_selector(|| "chat-details-progress".into());
    clickable(head, "chat-details-progress", p).on_click(|_, _, cx| {
        settings::update(cx, |settings| settings.chat_details.progress_open ^= true);
    })
}

/// A line of the plan: a phase's, which counts its sub-steps done and opens to them as well as to
/// what it covers, or a sub-step's under it.
#[derive(Clone, Copy)]
enum Line<'a> {
    Phase(&'a PlanPhase),
    SubStep(&'a PlanStep),
}

/// A line and, open, what's under it.
fn lines(view: &ChatView, line: Line<'_>, at: At, p: Palette, cx: &Context<ChatView>) -> Div {
    let (step, sub_steps, size, left, py) = match line {
        Line::Phase(phase) => (&phase.step, phase.steps.as_slice(), MARK, PAD, 5.),
        Line::SubStep(step) => (step, [].as_slice(), STEP_MARK, INDENT, 3.),
    };
    let count = match line {
        Line::Phase(phase) if !sub_steps.is_empty() => {
            let (done, counted) = phase.steps_done();
            Some(format!("{done}/{counted}"))
        }
        Line::Phase(_) | Line::SubStep(_) => None,
    };
    let folds = !sub_steps.is_empty() || step.detail.is_some();
    let open = folds && opens(view, &step.id, step.status);
    let took = step_time(step, at.now);
    let row = marked(step.status, &step.title, size, at, p)
        .py(px(py))
        .pl(px(left))
        .pr(px(PAD))
        .children(count.map(|count| aside(count, p)))
        .children(took.map(|took| aside(took, p)))
        .child(folding(folds, open, p));
    div()
        .flex()
        .flex_col()
        .child(toggles(row, &step.id, folds, open, p, cx))
        .when(open, |under| {
            let covers = step.detail.as_deref();
            under
                .children(covers.map(|says| detail(says, left + size + SPACE, p)))
                .children(sub_steps.iter().map(|step| lines(view, Line::SubStep(step), at, p, cx)))
        })
}

/// Whether a step shows what's under it: as a person last left it, else while it's under way.
fn opens(view: &ChatView, id: &str, status: StepStatus) -> bool {
    (view.details.opened_steps.get(id).copied()).unwrap_or(status.is_open())
}

/// A step's mark and its title, whole, wrapping onto the lines it takes: a title says what the
/// step does, cut it often wouldn't. What's under way reads as the text, the rest muted.
fn marked(status: StepStatus, title: &str, size: f32, at: At, p: Palette) -> Div {
    let (mark, color) = mark(status, at.working, p);
    let says = if status.is_open() { p.text } else { p.muted };
    div()
        .flex()
        .items_start()
        .gap(px(SPACE))
        .rounded(px(8.))
        .child(
            div()
                .flex_shrink_0()
                .pt(px((LINE - size) / 2.))
                .text_color(color)
                .child(icon(mark, size)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .line_height(px(LINE))
                .text_color(says)
                .child(title.to_owned()),
        )
}

/// What a line says beside its title, a count or a time, level with its first line.
fn aside(says: String, p: Palette) -> Div {
    meta(says, p.muted).line_height(px(LINE))
}

/// What marks a step of each status. A running one is still while the chat doesn't work, and
/// one waiting on the person is in the colour of what waits for them.
fn mark(status: StepStatus, working: bool, p: Palette) -> (IconName, Hsla) {
    match status {
        StepStatus::Pending => (IconName::Circle, p.muted),
        StepStatus::Running if working => (IconName::LoaderCircle, p.text),
        StepStatus::Running => (IconName::CirclePause, p.muted),
        StepStatus::Done => (IconName::CircleCheck, p.muted),
        StepStatus::Blocked => (IconName::CircleAlert, p.waiting),
        StepStatus::Skipped => (IconName::CircleMinus, p.muted),
    }
}

/// The chevron of a line that opens, or the room it would take.
fn folding(folds: bool, open: bool, p: Palette) -> Div {
    div()
        .flex_shrink_0()
        .w(px(CHEVRON))
        .h(px(LINE))
        .flex()
        .items_center()
        .text_color(p.muted)
        .when(folds, |slot| slot.child(chevron(open)))
}

/// A step's line, which opens and closes what's under it on a click if it has anything there.
fn toggles(
    line: Div,
    id: &str,
    folds: bool,
    open: bool,
    p: Palette,
    cx: &Context<ChatView>,
) -> Stateful<Div> {
    let name = format!("chat-details-progress-{id}");
    let line = line.debug_selector(|| name.clone());
    if !folds {
        return line.id(SharedString::from(name));
    }
    let id = id.to_owned();
    clickable(line, SharedString::from(name), p).on_click(cx.listener(
        move |view, _: &ClickEvent, _, cx| {
            view.details.opened_steps.insert(id.clone(), !open);
            cx.notify();
        },
    ))
}

/// What a step covers, under its title.
fn detail(says: &str, indent: f32, p: Palette) -> Div {
    div()
        .pl(px(indent))
        .pr(px(PAD))
        .pb(px(4.))
        .text_xs()
        .text_color(p.muted)
        .child(says.to_owned())
}

/// What the agent says it's doing while the chat works, and whether the plan has gone quiet; or,
/// while it doesn't and work is open, why nothing runs.
fn foot(plan: &Plan, replica: &Replica, at: At, p: Palette) -> Option<Div> {
    let note = |says: String, color: Hsla, name: &'static str| {
        div()
            .px(px(PAD))
            .pt(px(4.))
            .text_xs()
            .text_color(color)
            .debug_selector(|| name.into())
            .child(says)
    };
    let lines: Vec<Div> = if at.working {
        let now = (plan.now.as_ref())
            .map(|now| note(format!("Now: {now}"), p.muted, "chat-details-progress-now"));
        let stale = plan_stale(plan, at.now)
            .map(|stale| note(stale, p.working, "chat-details-progress-stale"));
        now.into_iter().chain(stale).collect()
    } else if plan.is_closed() {
        Vec::new()
    } else if replica.asking().is_some() {
        let says = "Waiting on you".to_owned();
        vec![note(says, p.waiting, "chat-details-progress-waiting")]
    } else {
        let says = "Paused until the next message".to_owned();
        vec![note(says, p.muted, "chat-details-progress-paused")]
    };
    (!lines.is_empty()).then(|| div().flex().flex_col().pb(px(4.)).children(lines))
}
