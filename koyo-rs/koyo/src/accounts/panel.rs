//! The accounts' panel (`docs/claude-accounts.md` 6): on a menu's glass, anchored to the button
//! and opening away from it, as `chat/composer/context.rs`'s does.
//!
//! An account a block: its email, strong, with its plan as a badge, and its organisation when
//! it's a team's; then a line per window (the week, each model's): its name, a bar coloured by
//! how near auto-switch's line it is with a tick where even use would have it (the crate's pace),
//! the percent, and how long until it resets; last, quiet, when the week resets and how old the
//! reading is, or what keeps it from being read. A click on another account Koyo keeps the login
//! of switches to it. Then the Mac's login when Koyo doesn't keep it, with a button to Settings →
//! Accounts, where it's added and the rest is set; what's being done or went wrong; and
//! auto-switch's state with what it last did.
//!
//! Each part is made with how tall it's drawn, so the window it opens in, and fits as it changes,
//! is sized by the code that draws it. It keeps a copy of what it shows, taken again as the
//! accounts change, so it's drawn without reading them.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, SharedString, Size, Stateful,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, px, relative, size,
};
use koyo_accounts::oauth::format_countdown;

use super::model::{Bar, Row, View};
use super::{Accounts, Job, words};
use crate::menu::MenuBody;
use crate::settings_view::SettingsPage;
use crate::theme::{self, Palette};
use crate::wrapped::wrapped_lines;

/// What the panel holds, inside the menu's padding (`MenuBody`).
const WIDTH: f32 = 310.;
/// Round an account's block.
const INSET_X: f32 = 10.;
const INSET_Y: f32 = 8.;
/// The email's line, and the type of it.
const HEADER: f32 = 18.;
const TITLE_SIZE: Pixels = px(13.);
/// The active account's dot, and the gap after it: what everything under the email is indented
/// by, so it lines up with the email.
const DOT: f32 = 6.;
const DOT_GAP: f32 = 8.;
const INDENT: f32 = DOT + DOT_GAP;
/// The small print, and the width it wraps at under an email or as a note.
const SMALL_SIZE: Pixels = px(11.);
const LINE: f32 = 15.;
const SMALL_WIDTH: f32 = WIDTH - 2. * INSET_X - INDENT;
/// A window's line, and the gap above the first.
const WINDOW: f32 = 18.;
const WINDOWS_GAP: f32 = 4.;
/// Over an account's status, and under a note.
const STATUS_GAP: f32 = 2.;
const NOTE_GAP: f32 = 4.;
const RULE: f32 = 1.;
/// How far under auto-switch's line a window turns amber.
const NEAR_LINE: f64 = 15.;

/// A part of the panel and how tall it's drawn.
type Part = (AnyElement, f32);

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

    /// The panel's parts, top to bottom.
    fn parts(&self, window: &Window, cx: &App) -> Vec<Part> {
        let p = theme::palette(cx);
        let now = super::now();
        let view = &self.view;
        // One thing at a time: while something's being done, nothing more is offered.
        let idle = view.doing.is_none();
        let mut blocks: Vec<Part> = Vec::new();
        for row in &view.rows {
            let (block, height) = account(row, now, view.line(), window, p);
            let switchable = idle && row.switchable && !row.active;
            let block = block.when(switchable, |block| {
                let (accounts, slot) = (self.accounts.clone(), row.slot());
                block
                    .cursor_pointer()
                    .hover(move |block| block.bg(p.hover))
                    .on_click(move |_, _, cx| {
                        let job = Job::Switch(slot.clone());
                        accounts.update(cx, |accounts, cx| accounts.ask(job, cx));
                    })
            });
            blocks.push((block.into_any_element(), height));
        }
        if let Some(email) = view.unmanaged.clone() {
            blocks.push(unkept(email, p));
        }
        let rule = || {
            let rule = div().mx(px(INSET_X)).h(px(RULE)).bg(p.hairline);
            (rule.into_any_element(), RULE)
        };
        let mut parts = Vec::new();
        for (index, block) in blocks.into_iter().enumerate() {
            if index > 0 {
                parts.push(rule());
            }
            parts.push(block);
        }
        for (text, failed) in notes(view) {
            let (note, height) = small(text, window, p);
            let note = (note
                .pl(px(INSET_X + INDENT))
                .pr(px(INSET_X))
                .pb(px(NOTE_GAP)))
            .when(failed, |note| note.text_color(p.danger));
            parts.push((note.into_any_element(), height + NOTE_GAP));
        }
        if !view.rows.is_empty() {
            parts.push(rule());
            parts.push(footer(view, window, p));
        }
        parts
    }
}

impl MenuBody for AccountsPanel {
    fn hears(&mut self, _: &str, _: &mut Context<Self>) -> bool {
        false
    }

    fn extent(&self, window: &Window, cx: &App) -> Size<Pixels> {
        let height: f32 = self
            .parts(window, cx)
            .iter()
            .map(|(_, height)| height)
            .sum();
        size(px(WIDTH), px(height))
    }
}

impl Render for AccountsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let parts = self.parts(window, cx).into_iter().map(|(part, _)| part);
        div().size_full().flex().flex_col().children(parts)
    }
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

/// Small print of `text`, muted and wrapping, with how tall it's drawn in `window`.
fn small(text: impl Into<SharedString>, window: &Window, p: Palette) -> (Div, f32) {
    let text = text.into();
    let lines = wrapped_lines(
        &text,
        &window.text_style(),
        SMALL_SIZE,
        px(SMALL_WIDTH),
        window,
    );
    let shown = div()
        .text_size(SMALL_SIZE)
        .line_height(px(LINE))
        .text_color(p.muted)
        .child(text);
    (shown, LINE * lines as f32)
}

/// The colour of a window used `percent`, against auto-switch's `line`: `calm` well under it,
/// amber within [`NEAR_LINE`] of it, red at it; muted when unknown.
pub(super) fn level(percent: Option<f64>, line: f64, calm: Hsla, p: Palette) -> Hsla {
    match percent {
        None => p.muted,
        Some(used) if used >= line => p.danger,
        Some(used) if used >= line - NEAR_LINE => p.working,
        Some(_) => calm,
    }
}

/// An email's line: the dot, lit for the Mac's login, the email, and what's at its end.
fn header(email: SharedString, lit: bool, end: Option<AnyElement>, p: Palette) -> Div {
    let dot = div()
        .flex_shrink_0()
        .size(px(DOT))
        .rounded_full()
        .when(lit, |dot| dot.bg(p.accent));
    let email = div()
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(TITLE_SIZE)
        .font_weight(FontWeight::MEDIUM)
        .text_color(p.text)
        .child(email);
    div()
        .h(px(HEADER))
        .flex()
        .items_center()
        .gap(px(DOT_GAP))
        .child(dot)
        .child(email)
        .children(end)
}

/// The plan, as a badge beside the email.
fn badge(plan: SharedString, p: Palette) -> AnyElement {
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
        .into_any_element()
}

/// Beside the Mac's login, a small button in the accent to Settings → Accounts, where it's added:
/// the panel goes, the page changing or not.
fn to_settings(p: Palette) -> AnyElement {
    div()
        .id("accounts-to-settings")
        .debug_selector(|| "accounts-to-settings".to_owned())
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
        .child("Add in Settings")
        .on_click(|_, _, cx| {
            crate::popup::close(crate::popup::Kind::Menu, cx);
            if let Some((_, shell)) = crate::control::main_shell(cx) {
                let page = Some(SettingsPage::Accounts);
                shell.update(cx, |shell, cx| shell.open_settings_page(page, cx));
            }
        })
        .into_any_element()
}

/// The Mac's login's block when Koyo doesn't keep it: its email with the button to Settings,
/// and what it is.
fn unkept(email: SharedString, p: Palette) -> Part {
    let what = div()
        .h(px(LINE))
        .pl(px(INDENT))
        .text_size(SMALL_SIZE)
        .text_color(p.muted)
        .child(words::NOT_KEPT);
    let block = (div().px(px(INSET_X)).py(px(INSET_Y)))
        .child(header(email, false, Some(to_settings(p)), p))
        .child(what);
    (block.into_any_element(), 2. * INSET_Y + HEADER + LINE)
}

/// An account's block: its email and plan, its organisation, a line per window, its status.
fn account(row: &Row, now: f64, line: f64, window: &Window, p: Palette) -> (Stateful<Div>, f32) {
    let badge = row.plan.clone().map(|plan| badge(plan, p));
    let organisation = (row.organisation.clone()).map(|organisation| {
        let (shown, _) = small(organisation, window, p);
        shown.h(px(LINE)).pl(px(INDENT)).truncate()
    });
    let windows = (!row.bars.is_empty()).then(|| {
        div().pt(px(WINDOWS_GAP)).children(
            row.bars
                .iter()
                .map(|bar| bar_line(&bar.as_of(now), now, line, p)),
        )
    });
    let (status, status_height) = small(words::status(row, now), window, p);
    let tone = match &row.problem {
        Some(problem) if problem.needs_you() => p.danger,
        Some(_) => p.working,
        None => p.muted,
    };
    let email = row.email.clone();
    let block = div()
        .id(SharedString::from(format!("accounts-row-{}", row.number)))
        .debug_selector(move || format!("accounts-row-{email}"))
        .px(px(INSET_X))
        .py(px(INSET_Y))
        .rounded(px(8.))
        .child(header(row.email.clone(), row.active, badge, p))
        .children(organisation)
        .children(windows)
        .child((status.pt(px(STATUS_GAP)).pl(px(INDENT))).text_color(tone));
    let organisation = if row.organisation.is_some() { LINE } else { 0. };
    let windows = if row.bars.is_empty() {
        0.
    } else {
        WINDOWS_GAP + row.bars.len() as f32 * WINDOW
    };
    let height = 2. * INSET_Y + HEADER + organisation + windows + STATUS_GAP + status_height;
    (block, height)
}

/// A window's line: its name, its bar with the pace tick, the percent, the time to its reset.
fn bar_line(bar: &Bar, now: f64, line: f64, p: Palette) -> Div {
    let colour = level(bar.percent, line, p.accent, p);
    let full = bar.percent.unwrap_or(0.).clamp(0., 100.) as f32 / 100.;
    // The bar, and a tick a little taller where even use of the week would have it.
    let track = div()
        .relative()
        .flex_1()
        .h(px(9.))
        .flex()
        .items_center()
        .child(
            div()
                .w_full()
                .h(px(5.))
                .rounded_full()
                .bg(p.text.opacity(0.1))
                .child(div().h_full().w(relative(full)).rounded_full().bg(colour)),
        )
        .children(bar.pace.map(|pace| {
            div()
                .absolute()
                .top_0()
                .left(relative((pace / 100.).clamp(0., 1.) as f32))
                .w(px(1.5))
                .h(px(9.))
                .rounded_full()
                .bg(p.text.opacity(0.55))
        }));
    let percent_colour = match bar.percent {
        None => p.muted,
        Some(used) if used >= line => p.danger,
        Some(_) => p.text,
    };
    div()
        .h(px(WINDOW))
        .pl(px(INDENT))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(SMALL_SIZE)
        .child(
            div()
                .w(px(40.))
                .text_color(p.muted)
                .truncate()
                .child(bar.label.clone()),
        )
        .child(track)
        .child(
            div()
                .w(px(34.))
                .flex()
                .justify_end()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(percent_colour)
                .child(words::percent(bar)),
        )
        .child(
            div()
                .w(px(46.))
                .flex()
                .justify_end()
                .text_color(p.muted)
                .children(bar.resets_at.map(|at| format_countdown(at - now))),
        )
}

/// Auto-switch's line: on at its threshold, or off; under it, what it last did.
fn footer(view: &View, window: &Window, p: Palette) -> Part {
    let state = match &view.auto_switch {
        Some(auto) => format!("On at {}", words::threshold(auto.threshold)),
        None => "Off".to_owned(),
    };
    let said = (view.auto_switch.as_ref())
        .and_then(|auto| auto.said.clone())
        .map(|said| small(said, window, p));
    let said_height = said.as_ref().map_or(0., |(_, height)| STATUS_GAP + height);
    let footer = div()
        .pl(px(INSET_X + INDENT))
        .pr(px(INSET_X))
        .py(px(INSET_Y))
        .child(
            div()
                .h(px(LINE))
                .flex()
                .items_center()
                .justify_between()
                .text_size(SMALL_SIZE)
                .child(div().text_color(p.text).child("Auto-switch"))
                .child(div().text_color(p.muted).child(state)),
        )
        .children(said.map(|(said, _)| said.pt(px(STATUS_GAP))));
    let height = 2. * INSET_Y + LINE + said_height;
    (footer.into_any_element(), height)
}
