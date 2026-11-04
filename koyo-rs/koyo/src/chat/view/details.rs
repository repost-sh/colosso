//! The card over a chat that says where it works and how that's going (`docs/chat.md` 5.3): its
//! branch and folder, what's changed there, its subagents and how full its conversation is. It's
//! up by itself where the pane has room for it beside the timeline, and over the timeline where a
//! person asks for it and there's none.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, Entity, Focusable as _, Hsla,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Pixels, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, actions, canvas, deferred, div, px,
};
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::model::{Item, ItemBody, ItemStatus, Subagent};
use koyo_agent::replica::Replica;
use koyo_agent::words::{
    NEARLY_FULL, context_line, context_percent, subagent_line, subagent_status,
};
use std::fmt::Display;
use std::path::PathBuf;

mod commit_dialog;
mod git_actions;
mod open_in;
mod scripts;

#[cfg(test)]
pub(in crate::chat) use git_actions::{commit_files, went_old};
pub(super) use git_actions::{keep_fresh, window_active_again};
pub(super) use open_in::{OpenIn, opened_in};

use super::{COLUMN, ChatView};
use crate::checkout::Checkout;
use crate::chat::rows::chevron;
use crate::chat::subagents::avatar;
use crate::menu::{self, MenuItem};
use crate::native::liquid;
use crate::right_panel::ToggleChanges;
use crate::settings::{self, ChatDetails, DetailsItem, FlipDetailsItem};
use crate::shell::{ellipsized, icon};
use crate::theme::{self, Palette};

actions!(chat, [ToggleDetails]);

/// How wide the card is.
const WIDTH: f32 = 280.;
/// What it keeps from the pane's edges.
const MARGIN: f32 = 12.;
/// How tall its controls are, and the gap under them.
const CONTROLS: f32 = 28.;
const GAP: f32 = 8.;
/// What it leaves at the bottom for the composer.
const COMPOSER: f32 = 180.;
const RADIUS: f32 = 16.;
const ROW_ICON: f32 = 15.;

pub(in super::super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-shift-b", ToggleDetails, None)]);
}

/// Where a chat works, as the shell knows it.
#[derive(Clone, PartialEq)]
pub(in super::super) struct Workplace {
    pub(in super::super) branch: SharedString,
    pub(in super::super) folder: PathBuf,
    /// The main checkout of the project it's in.
    pub(in super::super) project: PathBuf,
    pub(in super::super) is_main: bool,
    pub(in super::super) checkout: Entity<Checkout>,
}

#[derive(Default)]
pub(super) struct Details {
    place: Option<Workplace>,
    /// How big the pane was last drawn.
    pane: (Pixels, Pixels),
    /// Whether a person asked for it where it has no room beside the timeline.
    over: bool,
    /// Whether the subagents that ended show.
    previous: bool,
    git: git_actions::GitState,
}

impl Details {
    /// Whether the pane has room for it on the timeline's right.
    fn has_room(&self) -> bool {
        self.pane.0 >= px(COLUMN + 2. * (WIDTH + 2. * MARGIN))
    }

    pub(super) fn is_up(&self, cx: &App) -> bool {
        if self.has_room() { settings::get(cx).chat_details.open } else { self.over }
    }

    /// Follows where the chat works, which is new when it's on another branch.
    pub(super) fn set_place(&mut self, place: Workplace, cx: &mut Context<ChatView>) -> bool {
        let new = self.place.as_ref() != Some(&place);
        if new {
            self.git.moved(&place.checkout, cx);
            self.place = Some(place);
        }
        new
    }

    /// The card's up or it isn't, for this pane or wherever there's room, as it has any.
    pub(super) fn toggle(&mut self, cx: &mut App) {
        if self.has_room() {
            settings::update(cx, |settings| settings.chat_details.open ^= true);
        } else {
            self.over = !self.over;
        }
    }

    #[cfg(test)]
    pub(super) fn folder(&self) -> Option<PathBuf> {
        self.place.as_ref().map(|place| place.folder.clone())
    }

    pub(super) fn measured(&mut self, pane: (Pixels, Pixels)) -> bool {
        let new = self.pane != pane;
        self.pane = pane;
        new
    }
}

/// Keeps how big the pane is, which is drawn before the card is.
pub(super) fn measure(cx: &Context<ChatView>) -> impl IntoElement + use<> {
    let view = cx.weak_entity();
    canvas(
        move |bounds, _, cx| {
            let pane = (bounds.size.width, bounds.size.height);
            cx.defer(move |cx| {
                view.update(cx, |view, cx| {
                    if view.details.measured(pane) {
                        cx.notify();
                    }
                })
                .ok();
            });
        },
        |_, (), _, _| {},
    )
    .absolute()
    .size_full()
}

/// The card's controls and, if it's up, the card.
pub(super) fn floating(view: &ChatView, window: &mut Window, cx: &mut Context<ChatView>) -> AnyElement {
    let p = theme::palette(cx);
    let up = view.details.is_up(cx);
    let id = cx.entity_id().as_u64();
    let glass = ElementId::NamedInteger("chat-details-controls".into(), id);
    let on_glass = liquid::backdrop(glass, px(CONTROLS / 2.), window, cx);
    let controls = div().relative().flex().items_center().h(px(CONTROLS)).px_1().gap_1().rounded_full().occlude();
    let controls = liquid::floating(controls, on_glass, p)
        .when(up, |controls| controls.child(choosing(view, p, cx)))
        .child(
            control("chat-details-toggle", IconName::PanelRight, p)
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.details.toggle(cx);
                    cx.notify();
                })),
        );
    let card = up.then(|| card(view, window, cx));
    deferred(
        div()
            .absolute()
            .top(px(MARGIN))
            .right(px(MARGIN))
            .flex()
            .flex_col()
            .items_end()
            .gap(px(GAP))
            .child(controls)
            .children(card),
    )
    .into_any_element()
}

fn control(id: &'static str, mark: IconName, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(CONTROLS - 6.))
        .rounded_full()
        .cursor_pointer()
        .text_color(p.muted)
        .hover(move |control| control.bg(p.hover).text_color(p.text))
        .child(icon(mark, 14.))
}

/// What a person can have the card show.
fn choosing(view: &ChatView, p: Palette, cx: &App) -> impl IntoElement + use<> {
    let choice = settings::get(cx).chat_details.clone();
    menu::dropdown(
        "chat-details-items",
        control("chat-details-choose", IconName::Ellipsis, p),
        view.focus_handle(cx),
        move |_| items(&choice),
    )
}

fn card(view: &ChatView, window: &mut Window, cx: &mut Context<ChatView>) -> AnyElement {
    let p = theme::palette(cx);
    let id = cx.entity_id().as_u64();
    let glass = ElementId::NamedInteger("chat-details-glass".into(), id);
    let on_glass = liquid::backdrop(glass, px(RADIUS), window, cx);
    let choice = settings::get(cx).chat_details.clone();
    let replica = view.showing().and_then(|shown| shown.thread.read(cx).replica());
    let tallest = (view.details.pane.1 - px(MARGIN + CONTROLS + GAP + COMPOSER)).max(px(120.));
    let sections: Vec<Div> = [
        choice.shows(DetailsItem::Worktree).then(|| worktree(view, p, cx)).flatten(),
        choice.shows(DetailsItem::OpenIn).then(|| open_in::line(view, p, cx)).flatten(),
        choice.shows(DetailsItem::Scripts).then(|| scripts::section(view, p, cx)).flatten(),
        choice.shows(DetailsItem::Git).then(|| git_actions::rows(view, p, cx)).flatten(),
        choice.shows(DetailsItem::Changes).then(|| changes(view, p, cx)).flatten(),
        choice.shows(DetailsItem::Subagents).then(|| subagents(view, replica, p, cx)).flatten(),
        choice.shows(DetailsItem::Context).then(|| context(replica, p)).flatten(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let surface = div().relative().w(px(WIDTH)).rounded(px(RADIUS)).occlude().text_sm();
    let body = div()
        .id("chat-details")
        .max_h(tallest)
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .p(px(6.))
        .children(sections.into_iter().enumerate().map(|(at, section)| {
            section.when(at > 0, |section| section.mt(px(6.)).pt(px(6.)).border_t_1().border_color(p.hairline))
        }));
    liquid::floating(surface, on_glass, p).child(body).into_any_element()
}

/// A line of the card: what marks it, and what it says.
fn row(mark: IconName, p: Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .min_h(px(32.))
        .px(px(10.))
        .rounded(px(8.))
        .text_color(p.text)
        .child(div().flex_shrink_0().text_color(p.muted).child(icon(mark, ROW_ICON)))
}

fn clickable(row: Div, id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    row.id(id).cursor_pointer().hover(move |row| row.bg(p.hover))
}

/// "1 file", or "3 files".
fn counted<Count: Copy + Display + PartialEq + From<u8>>(count: Count, thing: &str) -> String {
    if count == Count::from(1) { format!("1 {thing}") } else { format!("{count} {thing}s") }
}

fn meta(says: impl Into<SharedString>, color: Hsla) -> Div {
    div().flex_shrink_0().text_xs().text_color(color).child(says.into())
}

fn worktree(view: &ChatView, p: Palette, cx: &Context<ChatView>) -> Option<Div> {
    let place = view.details.place.as_ref()?;
    let folder = place.folder.clone();
    let called = if place.is_main {
        "Main checkout".to_owned()
    } else {
        folder.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    };
    let branch = row(IconName::GitBranch, p).child(ellipsized(place.branch.clone()));
    let reveal = clickable(row(IconName::Folder, p).child(ellipsized(called)), "chat-details-folder", p)
        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.reveal_path(&folder)));
    Some(div().flex().flex_col().child(branch).child(reveal))
}

fn changes(view: &ChatView, p: Palette, cx: &Context<ChatView>) -> Option<Div> {
    let place = view.details.place.as_ref()?;
    let checkout = place.checkout.read(cx);
    let (added, removed) = checkout.totals();
    let files = checkout.files().len();
    let says = if files == 0 { "No changes".to_owned() } else { counted(files, "file") };
    let line = row(IconName::FileDiff, p).child(div().flex_1().min_w_0().child(says));
    let line = line
        .when(added > 0, |line| line.child(meta(format!("+{added}"), p.added)))
        .when(removed > 0, |line| line.child(meta(format!("−{removed}"), p.danger)));
    Some(div().child(clickable(line, "chat-details-changes", p).on_click(|_, window, cx| {
        window.dispatch_action(Box::new(ToggleChanges), cx);
    })))
}

/// The subagents working now, then those done with, behind a line.
fn subagents(
    view: &ChatView,
    replica: Option<&Replica>,
    p: Palette,
    cx: &Context<ChatView>,
) -> Option<Div> {
    let agents: Vec<(&Item, &Subagent)> = (replica?.items.iter())
        .filter_map(|item| match &item.body {
            ItemBody::Subagent(agent) => Some((item, &**agent)),
            _ => None,
        })
        .collect();
    if agents.is_empty() {
        return None;
    }
    let running = |item: &Item| matches!(item.status, ItemStatus::Running | ItemStatus::Pending);
    let (working, done): (Vec<_>, Vec<_>) = agents.into_iter().partition(|(item, _)| running(item));
    let open = view.details.previous;
    let line = row(IconName::Bot, p)
        .child(div().flex_1().min_w_0().child(format!("Previous agents · {}", done.len())))
        .child(div().text_color(p.muted).child(chevron(open)));
    let toggle = clickable(line, "chat-details-previous", p).on_click(cx.listener(
        |view, _: &ClickEvent, _, cx| {
            view.details.previous ^= true;
            cx.notify();
        },
    ));
    Some(
        div()
            .flex()
            .flex_col()
            .children(working.iter().map(|(item, agent)| subagent(item, agent, p, cx)))
            .when(!done.is_empty(), |list| list.child(toggle))
            .when(open, |list| {
                list.children(done.iter().map(|(item, agent)| subagent(item, agent, p, cx)))
            }),
    )
}

fn subagent(item: &Item, agent: &Subagent, p: Palette, cx: &Context<ChatView>) -> Stateful<Div> {
    let status = subagent_line(
        subagent_status(item.status),
        agent.started_at.as_ref(),
        agent.ended_at.as_ref(),
        chrono::Utc::now(),
    );
    let color = if item.status == ItemStatus::Failed { p.danger } else { p.muted };
    let line = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_h(px(36.))
        .px(px(8.))
        .rounded(px(8.))
        .child(avatar(item.status, p))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(ellipsized(agent.description.clone()).text_color(p.text))
                .child(ellipsized(status).text_xs().text_color(color)),
        );
    let id = SharedString::from(format!("chat-details-subagent-{}", item.id));
    let thread: Option<ThreadId> = agent.thread_id.clone();
    line.id(id).when_some(thread, |line, thread| {
        line.cursor_pointer()
            .hover(move |line| line.bg(p.hover))
            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.open_subagent(&thread, window, cx);
            }))
    })
}

fn context(replica: Option<&Replica>, p: Palette) -> Option<Div> {
    let (held, room) = replica?.context()?;
    let percent = context_percent(held, room);
    let color = if percent >= NEARLY_FULL { p.danger } else { p.muted };
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(10.))
            .py(px(6.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(div().text_color(p.text).child("Context"))
                    .child(meta(context_line(held, room), p.muted)),
            )
            .child(
                div().h(px(3.)).w_full().rounded_full().bg(p.hairline).child(
                    div().h_full().rounded_full().bg(color).w(gpui_kit::relative(f32::from(percent) / 100.)),
                ),
            ),
    )
}

/// What a person can turn on and off of the card.
fn items(choice: &ChatDetails) -> Vec<MenuItem> {
    DetailsItem::ALL
        .into_iter()
        .map(|item| {
            MenuItem::new(None, item.label(), Box::new(FlipDetailsItem(item)))
                .switch(choice.shows(item))
        })
        .collect()
}
