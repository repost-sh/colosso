//! The accounts' panel (`docs/claude-accounts.md` 6): on a menu's glass, anchored to the button
//! and opening upward from the sidebar's foot, as `chat/composer/context.rs`'s does. A row per
//! account: its email, organisation and plan, the active mark, the 7 days' bar and each model's,
//! with the percent, the countdown to the reset and its date, "ahead of pace", and "(!)" at the
//! limit; then how old the reading is or what keeps it from being read. A click on another
//! account Koyo keeps the login of switches to it. Under the rows: what's being done, the Mac's
//! login when Koyo doesn't keep it with Add current login, and what went wrong. It keeps a copy
//! of what it shows, taken again as the accounts change, so it's drawn without reading them.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, Div, Entity, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render,
    SharedString, Size, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    px, relative, size,
};

use super::model::{Bar, Row, View};
use super::{Accounts, Job, words};
use crate::menu::{self, MenuBody, PADDING, ROW_PADDING};
use crate::theme::{self, Palette};
use crate::wrapped::wrapped_lines;

const WIDTH: f32 = 340.;
/// The parts of a row, top to bottom, and the room round it.
const TITLE: f32 = 20.;
const LINE: f32 = 16.;
/// The notes' text: small print.
const NOTE_SIZE: Pixels = px(12.);
const BAR: f32 = 18.;
const ROW_INSET: f32 = 6.;
/// The active mark, and the gap after it: what the lines under the title are indented by.
const MARK: f32 = 7.;
const MARK_GAP: f32 = 6.;
const INDENT: f32 = MARK + MARK_GAP;
/// A bar's label, and the gap after it: what its reset line is indented by, past `INDENT`.
const LABEL: f32 = 52.;
const LABEL_GAP: f32 = 8.;

/// How tall `row` is drawn.
fn row_height(row: &Row) -> f32 {
    ROW_INSET * 2. + TITLE + LINE + row.bars.len() as f32 * (BAR + LINE) + LINE
}

/// The notes under the rows, top to bottom, as they're shown for `view`.
fn notes(view: &View) -> Vec<(SharedString, bool)> {
    let unmanaged = view.unmanaged.as_deref().map(words::unmanaged);
    let empty = (view.rows.is_empty() && view.unmanaged.is_none())
        .then(|| "The Mac isn't signed in to Claude Code.".to_owned());
    let overridden = view.overridden_by.map(|name| {
        format!("{name} is set in your shell, so Claude Code there uses it, not this login.")
    });
    let auto = view.auto_switch.as_ref();
    let plain = [
        view.doing.clone(),
        unmanaged,
        empty,
        auto.map(words::auto_switch),
        auto.and_then(|auto| auto.said.clone()),
        overridden,
    ];
    let failed = [view.said.clone(), view.unreadable.clone()];
    let plain = plain.into_iter().flatten().map(|note| (note.into(), false));
    plain
        .chain(failed.into_iter().flatten().map(|note| (note.into(), true)))
        .collect()
}

/// How big the panel opens for `view` in `window`: its rows, Add current login, and its notes.
pub fn extent(view: &View, window: &Window) -> Size<Pixels> {
    let rows: f32 = view.rows.iter().map(row_height).sum();
    let add = if view.unmanaged.is_some() {
        theme::ROW_HEIGHT
    } else {
        0.
    };
    let (style, width) = (
        window.text_style(),
        px(WIDTH - 2. * (PADDING + ROW_PADDING)),
    );
    let lines: usize = (notes(view).iter())
        .map(|(text, _)| wrapped_lines(text, &style, NOTE_SIZE, width, window))
        .sum();
    let notes = lines as f32 * LINE;
    size(px(WIDTH), px(rows + add + notes + PADDING * 2.))
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
        // One thing at a time: while something's being done, nothing more is offered.
        let idle = view.doing.is_none();
        let rows = view.rows.iter().map(|row| {
            let switchable = idle && row.switchable && !row.active;
            row_shown(row, now, p).when(switchable, |shown| {
                let (accounts, slot) = (self.accounts.clone(), row.slot());
                shown
                    .cursor_pointer()
                    .hover(move |row| row.bg(p.hover))
                    .on_click(move |_, _, cx| {
                        let job = Job::Switch(slot.clone());
                        accounts.update(cx, |accounts, cx| accounts.ask(job, cx));
                    })
            })
        });
        let add = (idle && view.unmanaged.is_some()).then(|| {
            let accounts = self.accounts.clone();
            menu::row("accounts-add")
                .text_color(p.accent)
                .hover(|row| row.bg(p.selected))
                .child("Add current login")
                .on_click(move |_, _, cx| {
                    accounts.update(cx, |accounts, cx| accounts.ask(Job::Add, cx));
                })
        });
        let notes = notes(view)
            .into_iter()
            .map(|(text, failed)| note(text, p).when(failed, |note| note.text_color(p.danger)));
        div()
            .size_full()
            .p(px(PADDING))
            .flex()
            .flex_col()
            .children(rows)
            .children(add)
            .children(notes)
    }
}

/// A line of small print.
fn note(text: impl Into<SharedString>, p: Palette) -> Div {
    div()
        .px(px(ROW_PADDING))
        .text_size(NOTE_SIZE)
        .line_height(px(LINE))
        .text_color(p.muted)
        .child(text.into())
}

/// An account's row: its name and where it stands, its bars, and how old its reading is.
fn row_shown(row: &Row, now: f64, p: Palette) -> gpui_kit::Stateful<Div> {
    let mark = div()
        .size(px(MARK))
        .rounded_full()
        .when(row.active, |mark| mark.bg(p.accent));
    let title = div()
        .h(px(TITLE))
        .flex()
        .items_center()
        .gap(px(MARK_GAP))
        .child(mark)
        .child(div().flex_1().min_w_0().truncate().child(row.email.clone()));
    let status = match (&row.problem, row.measured_at) {
        (Some(problem), _) => problem.sentence(now),
        (None, Some(at)) => format!("Read {}", words::age(at, now)),
        (None, None) => "Not read yet.".to_owned(),
    };
    let email = row.email.clone();
    div()
        .id(SharedString::from(format!("accounts-row-{}", row.number)))
        .debug_selector(move || format!("accounts-row-{email}"))
        .px(px(ROW_PADDING))
        .py(px(ROW_INSET))
        .rounded(px(6.))
        .child(title)
        .child(small(row.under(), p))
        .children(
            row.bars
                .iter()
                .map(|bar| bar_lines(&bar.as_of(now), now, p)),
        )
        .child(small(status, p).when(row.problem.is_some(), |line| line.text_color(p.danger)))
}

/// A line under a row's title.
fn small(text: impl Into<SharedString>, p: Palette) -> Div {
    div()
        .h(px(LINE))
        .pl(px(INDENT))
        .text_xs()
        .text_color(p.muted)
        .truncate()
        .child(text.into())
}

/// A window's two lines: its label, bar and percent; then when it resets, and its pace.
fn bar_lines(bar: &Bar, now: f64, p: Palette) -> Div {
    let full = bar.percent.unwrap_or(0.).clamp(0., 100.) as f32 / 100.;
    let colour = if bar.percent.is_some_and(|percent| percent >= 100.) {
        p.danger
    } else {
        p.accent
    };
    let track = div()
        .flex_1()
        .h(px(4.))
        .rounded_full()
        .bg(p.hairline)
        .child(div().h_full().w(relative(full)).rounded_full().bg(colour));
    let first = div()
        .h(px(BAR))
        .pl(px(INDENT))
        .flex()
        .items_center()
        .gap(px(LABEL_GAP))
        .text_xs()
        .child(
            div()
                .w(px(LABEL))
                .text_color(p.muted)
                .child(bar.label.clone()),
        )
        .child(track)
        .child(
            div()
                .w(px(64.))
                .flex()
                .justify_end()
                .child(words::percent(bar)),
        );
    let mut said = words::resets(bar, now).unwrap_or_default();
    if bar.ahead_of_pace {
        said.push_str(" (ahead of pace)");
    }
    div()
        .child(first)
        .child(small(said, p).pl(px(INDENT + LABEL + LABEL_GAP)))
}
