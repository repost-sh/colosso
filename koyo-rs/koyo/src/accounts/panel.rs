//! The accounts' panel (`docs/claude-accounts.md` 6): on a menu's glass, anchored to the button
//! and opening upward from the sidebar's foot, as `chat/composer/context.rs`'s does.
//!
//! An account a block: its email, strong, with its plan as a badge, and its organisation when
//! it's a team's; then a line per window (the week, each model's): its name, a bar coloured by
//! how near auto-switch's line it is with a tick where even use would be by now, the percent,
//! and how long until it resets; last, quiet, when the week resets and how old the reading is,
//! or what keeps it from being read. A click on another account Koyo keeps the login of switches
//! to it. Under the accounts, auto-switch's state and what it last did, Add current login when
//! the Mac's login isn't one of Koyo's, and what went wrong. It keeps a copy of what it shows,
//! taken again as the accounts change, so it's drawn without reading them.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, Div, Entity, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, SharedString, Size, StatefulInteractiveElement as _,
    Styled as _, Subscription, Window, div, px, relative, size,
};
use koyo_accounts::autoswitch::DEFAULT_THRESHOLD;
use koyo_accounts::oauth::{format_countdown, login_expiry_clock};
use koyo_accounts::pace::WEEKLY_PERIOD_S;

use super::model::{Bar, Row, View};
use super::{Accounts, Job, words};
use crate::menu::{MenuBody, PADDING};
use crate::theme::{self, Palette};
use crate::wrapped::wrapped_lines;

const WIDTH: f32 = 320.;
/// Round an account's block.
const INSET_X: f32 = 10.;
const INSET_Y: f32 = 8.;
/// The email's line, and the type of it.
const HEADER: f32 = 20.;
const TITLE_SIZE: Pixels = px(13.);
/// The active account's dot, and the gap after it: what everything under the email is indented
/// by, so it lines up with the email.
const DOT: f32 = 6.;
const DOT_GAP: f32 = 8.;
const INDENT: f32 = DOT + DOT_GAP;
/// The small print: the organisation, the windows' names and numbers, the status, the notes.
const SMALL_SIZE: Pixels = px(11.);
const LINE: f32 = 15.;
/// A window's line, the gap above the first, and its columns.
const WINDOW: f32 = 18.;
const WINDOWS_GAP: f32 = 4.;
const NAME: f32 = 40.;
const PERCENT: f32 = 34.;
const COUNTDOWN: f32 = 46.;
const COLUMN_GAP: f32 = 8.;
const BAR_HEIGHT: f32 = 5.;
/// The pace tick: a little taller than the bar it marks.
const TICK_HEIGHT: f32 = 9.;
/// Auto-switch's line.
const FOOTER: f32 = 28.;
/// How far under auto-switch's line a window turns amber.
pub(super) const NEAR_LINE: f64 = 15.;

/// How many lines `text` takes in small print across `width`.
fn lines(text: &SharedString, width: f32, window: &Window) -> f32 {
    wrapped_lines(text, &window.text_style(), SMALL_SIZE, px(width), window) as f32
}

/// The width small print under an account's email has.
fn under_width() -> f32 {
    WIDTH - 2. * (PADDING + INSET_X) - INDENT
}

/// The width a note under the accounts has.
fn note_width() -> f32 {
    WIDTH - 2. * (PADDING + INSET_X) - INDENT
}

/// How tall `row` is drawn in `window`.
fn row_height(row: &Row, now: f64, window: &Window) -> f32 {
    let organisation = if row.organisation.is_some() { LINE } else { 0. };
    let windows = if row.bars.is_empty() {
        0.
    } else {
        WINDOWS_GAP + row.bars.len() as f32 * WINDOW
    };
    let status = LINE * lines(&status(row, now).0, under_width(), window);
    2. * INSET_Y + HEADER + organisation + windows + 2. + status
}

/// The notes under the accounts, top to bottom: each with whether it says what went wrong.
fn notes(view: &View) -> Vec<(SharedString, bool)> {
    let empty = (view.rows.is_empty() && view.unmanaged.is_none())
        .then(|| "The Mac isn't signed in to Claude Code.".to_owned());
    let overridden = view.overridden_by.map(|name| {
        format!("{name} is set in your shell, so Claude Code there uses it, not this login.")
    });
    let plain = [view.doing.clone(), empty, overridden];
    let failed = [view.said.clone(), view.unreadable.clone()];
    let plain = plain.into_iter().flatten().map(|note| (note.into(), false));
    plain
        .chain(failed.into_iter().flatten().map(|note| (note.into(), true)))
        .collect()
}

/// What auto-switch last did that matters, under its line.
fn auto_said(view: &View) -> Option<SharedString> {
    let said = view.auto_switch.as_ref()?.said.clone()?;
    Some(said.into())
}

/// The Mac's login's block, when Koyo doesn't keep it: its email, and what it is.
const UNKEPT: f32 = 2. * INSET_Y + HEADER + LINE;

/// How big the panel opens for `view` in `window`: every part as drawn, wrapped text measured.
pub fn extent(view: &View, window: &Window) -> Size<Pixels> {
    let now = super::now();
    let blocks = view.rows.len() + usize::from(view.unmanaged.is_some());
    let rows: f32 = (view.rows.iter())
        .map(|row| row_height(row, now, window))
        .sum();
    let unkept = if view.unmanaged.is_some() { UNKEPT } else { 0. };
    let rules = blocks.saturating_sub(1) as f32;
    let notes: f32 = (notes(view).iter())
        .map(|(text, _)| LINE * lines(text, note_width(), window) + 4.)
        .sum();
    let footer = if view.rows.is_empty() {
        0.
    } else {
        1. + FOOTER
            + auto_said(view).map_or(0., |said| LINE * lines(&said, note_width(), window) + 6.)
    };
    let height = rows + unkept + rules + notes + footer + 2. * PADDING;
    size(px(WIDTH), px(height))
}

pub struct AccountsPanel {
    accounts: Entity<Accounts>,
    view: View,
    _observing: Subscription,
}

impl AccountsPanel {
    pub fn new(accounts: &Entity<Accounts>, view: View, cx: &mut Context<Self>) -> Self {
        let observing = cx.observe(accounts, |panel, accounts, cx| {
            panel.view = accounts.read(cx).view.clone();
            cx.notify();
        });
        AccountsPanel {
            accounts: accounts.clone(),
            view,
            _observing: observing,
        }
    }
}

impl MenuBody for AccountsPanel {
    fn hears(&mut self, _: &str, _: &mut Context<Self>) -> bool {
        false
    }
}

impl Render for AccountsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let now = super::now();
        let view = &self.view;
        let line = (view.auto_switch.as_ref()).map_or(DEFAULT_THRESHOLD, |auto| auto.threshold);
        // One thing at a time: while something's being done, nothing more is offered.
        let idle = view.doing.is_none();
        let mut blocks = Vec::new();
        for (index, row) in view.rows.iter().enumerate() {
            if index > 0 {
                blocks.push(rule(p).into_any_element());
            }
            let switchable = idle && row.switchable && !row.active;
            let block = account(row, now, line, p).when(switchable, |block| {
                let (accounts, slot) = (self.accounts.clone(), row.slot());
                block
                    .cursor_pointer()
                    .hover(move |block| block.bg(p.hover))
                    .on_click(move |_, _, cx| {
                        let job = Job::Switch(slot.clone());
                        accounts.update(cx, |accounts, cx| accounts.ask(job, cx));
                    })
            });
            blocks.push(block.into_any_element());
        }
        if let Some(email) = view.unmanaged.clone() {
            if !blocks.is_empty() {
                blocks.push(rule(p).into_any_element());
            }
            let add = idle.then(|| {
                let accounts = self.accounts.clone();
                add_button(p).on_click(move |_, _, cx| {
                    accounts.update(cx, |accounts, cx| accounts.ask(Job::Add, cx));
                })
            });
            blocks.push(unkept(email, add, p).into_any_element());
        }
        let notes = notes(view).into_iter().map(|(text, failed)| {
            small(text, p)
                .pl(px(INSET_X + INDENT))
                .pr(px(INSET_X))
                .pb(px(4.))
                .when(failed, |note| note.text_color(p.danger))
        });
        div()
            .size_full()
            .p(px(PADDING))
            .flex()
            .flex_col()
            .children(blocks)
            .children(notes)
            .when(!view.rows.is_empty(), |panel| panel.child(footer(view, p)))
    }
}

/// The hairline between two accounts.
fn rule(p: Palette) -> Div {
    div().mx(px(INSET_X)).h(px(1.)).bg(p.text.opacity(0.1))
}

/// The Mac's login's block when Koyo doesn't keep it: its email with Add, and what it is.
fn unkept(email: SharedString, add: Option<gpui_kit::Stateful<Div>>, p: Palette) -> Div {
    let header = div()
        .h(px(HEADER))
        .pl(px(INDENT))
        .flex()
        .items_center()
        .gap(px(DOT_GAP))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(TITLE_SIZE)
                .font_weight(FontWeight::MEDIUM)
                .text_color(p.text)
                .child(email),
        )
        .children(add);
    div().px(px(INSET_X)).py(px(INSET_Y)).child(header).child(
        small("The Mac's login, not one of Koyo's accounts yet", p)
            .h(px(LINE))
            .pl(px(INDENT)),
    )
}

/// Add, beside the Mac's login: a small button in the accent.
fn add_button(p: Palette) -> gpui_kit::Stateful<Div> {
    div()
        .id("accounts-add")
        .flex_shrink_0()
        .h(px(20.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(5.))
        .bg(p.accent)
        .text_color(gpui_kit::white())
        .text_size(SMALL_SIZE)
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|button| button.opacity(0.85))
        .child("Add")
}

/// Small print: muted, wrapping.
fn small(text: impl Into<SharedString>, p: Palette) -> Div {
    div()
        .text_size(SMALL_SIZE)
        .line_height(px(LINE))
        .text_color(p.muted)
        .child(text.into())
}

/// The colour of a window used `percent`, against auto-switch's `line`: the accent well under
/// it, amber within [`NEAR_LINE`] of it, red at it; muted when unknown.
pub(super) fn level(percent: Option<f64>, line: f64, p: Palette) -> Hsla {
    match percent {
        None => p.muted,
        Some(used) if used >= line => p.danger,
        Some(used) if used >= line - NEAR_LINE => p.working,
        Some(_) => p.accent,
    }
}

/// The plan, as a badge beside the email.
fn badge(plan: SharedString, p: Palette) -> Div {
    div()
        .flex_shrink_0()
        .h(px(16.))
        .px(px(6.))
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(p.accent.opacity(0.2))
        .text_color(p.accent)
        .text_size(px(10.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(plan)
}

/// What an account's last line says: when the week resets and how old the reading is, or what
/// keeps it from being read, with its colour: amber for a wait that passes by itself, red for
/// what needs the person.
fn status(row: &Row, now: f64) -> (SharedString, Tone) {
    if let Some(problem) = &row.problem {
        let tone = if problem.needs_you() {
            Tone::Fault
        } else {
            Tone::Wait
        };
        return (problem.sentence(now).into(), tone);
    }
    let Some(measured) = row.measured_at else {
        return ("Not read yet".into(), Tone::Quiet);
    };
    let read = format!("read {}", words::age(measured, now));
    let resets = (row.weekly())
        .and_then(|week| week.as_of(now).resets_at)
        .map(|at| format!("Resets {} · ", login_expiry_clock(at, now)));
    let said = match resets {
        Some(resets) => format!("{resets}{read}"),
        None => capitalised(&read),
    };
    (said.into(), Tone::Quiet)
}

/// How an account's last line reads.
#[derive(Clone, Copy)]
enum Tone {
    Quiet,
    /// A wait that passes by itself.
    Wait,
    /// What needs the person.
    Fault,
}

impl Tone {
    fn colour(self, p: Palette) -> Hsla {
        match self {
            Tone::Quiet => p.muted,
            Tone::Wait => p.working,
            Tone::Fault => p.danger,
        }
    }
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// An account's block: its email and plan, its organisation, a line per window, its status.
fn account(row: &Row, now: f64, line: f64, p: Palette) -> gpui_kit::Stateful<Div> {
    let dot = div()
        .flex_shrink_0()
        .size(px(DOT))
        .rounded_full()
        .when(row.active, |dot| dot.bg(p.accent));
    let email = div()
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(TITLE_SIZE)
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.text)
        .child(row.email.clone());
    let header = div()
        .h(px(HEADER))
        .flex()
        .items_center()
        .gap(px(DOT_GAP))
        .child(dot)
        .child(email)
        .children(row.plan.clone().map(|plan| badge(plan, p)));
    let organisation = (row.organisation.clone())
        .map(|organisation| small(organisation, p).h(px(LINE)).pl(px(INDENT)).truncate());
    let windows = (!row.bars.is_empty()).then(|| {
        div().pt(px(WINDOWS_GAP)).children(
            row.bars
                .iter()
                .map(|bar| window(&bar.as_of(now), now, line, p)),
        )
    });
    let (said, tone) = status(row, now);
    let email = row.email.clone();
    div()
        .id(SharedString::from(format!("accounts-row-{}", row.number)))
        .debug_selector(move || format!("accounts-row-{email}"))
        .px(px(INSET_X))
        .py(px(INSET_Y))
        .rounded(px(8.))
        .child(header)
        .children(organisation)
        .children(windows)
        .child(
            small(said, p)
                .pt(px(2.))
                .pl(px(INDENT))
                .text_color(tone.colour(p)),
        )
}

/// A window's line: its name, its bar with the pace tick, the percent, the time to its reset.
fn window(bar: &Bar, now: f64, line: f64, p: Palette) -> Div {
    let colour = level(bar.percent, line, p);
    let full = bar.percent.unwrap_or(0.).clamp(0., 100.) as f32 / 100.;
    // Where even use of the week would have it by now.
    let pace = (bar.resets_at)
        .map(|at| (1. - (at - now) / WEEKLY_PERIOD_S).clamp(0., 1.) as f32)
        .filter(|pace| *pace > 0.);
    let track = div()
        .relative()
        .flex_1()
        .h(px(TICK_HEIGHT))
        .flex()
        .items_center()
        .child(
            div()
                .w_full()
                .h(px(BAR_HEIGHT))
                .rounded_full()
                .bg(p.text.opacity(0.1))
                .child(div().h_full().w(relative(full)).rounded_full().bg(colour)),
        )
        .children(pace.map(|pace| {
            div()
                .absolute()
                .top_0()
                .left(relative(pace))
                .w(px(1.5))
                .h(px(TICK_HEIGHT))
                .rounded_full()
                .bg(p.text.opacity(0.55))
        }));
    let percent = div()
        .w(px(PERCENT))
        .flex()
        .justify_end()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if bar.percent.is_some() {
            p.text
        } else {
            p.muted
        })
        .when(bar.percent.is_some_and(|used| used >= line), |percent| {
            percent.text_color(p.danger)
        })
        .child(words::percent(bar));
    let countdown = div()
        .w(px(COUNTDOWN))
        .flex()
        .justify_end()
        .text_color(p.muted)
        .children(bar.resets_at.map(|at| format_countdown(at - now)));
    div()
        .h(px(WINDOW))
        .pl(px(INDENT))
        .flex()
        .items_center()
        .gap(px(COLUMN_GAP))
        .text_size(SMALL_SIZE)
        .child(
            div()
                .w(px(NAME))
                .text_color(p.muted)
                .truncate()
                .child(bar.label.clone()),
        )
        .child(track)
        .child(percent)
        .child(countdown)
}

/// Auto-switch's line: on at its threshold, or off; under it, what it last did.
fn footer(view: &View, p: Palette) -> Div {
    let state = match &view.auto_switch {
        Some(auto) => format!("On at {}", words::threshold(auto.threshold)),
        None => "Off".to_owned(),
    };
    div()
        .mt(px(1.))
        .border_t_1()
        .border_color(p.hairline)
        .pl(px(INSET_X + INDENT))
        .pr(px(INSET_X))
        .child(
            div()
                .h(px(FOOTER))
                .flex()
                .items_center()
                .justify_between()
                .text_size(SMALL_SIZE)
                .child(div().text_color(p.text).child("Auto-switch"))
                .child(div().text_color(p.muted).child(state)),
        )
        .children(auto_said(view).map(|said| small(said, p).pb(px(6.))))
}
