//! The card at a chat's right that says where it works and how that's going (`docs/chat.md` 5.3):
//! its branch and folder, what's changed there, its subagents, how full its conversation is and
//! how far the plan of its task has come. It's up by itself beside the chat where the pane has
//! room for both, the chat keeping to what's left of it, and over the chat where a person asks for
//! it and there's none; down, its controls still say how far the plan has come.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, Entity, Focusable as _, Hsla,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Pixels, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, Window, actions, deferred, div, px,
    relative,
};
use koyo_agent::replica::Replica;
use koyo_agent::words::{NEARLY_FULL, context_line, context_percent};
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Display;
use std::path::PathBuf;
use std::rc::Rc;

mod commit_dialog;
mod git_actions;
mod open_in;
mod progress;
mod resize;
mod room;
mod scripts;
mod subagents;

#[cfg(test)]
pub(in crate::chat) use git_actions::{commit_files, went_old};
pub(super) use git_actions::{keep_fresh, window_active_again};
pub(in crate::chat) use open_in::OpenIn;
pub(super) use open_in::opened_in;
pub(super) use room::measure;

use super::ChatView;
use crate::checkout::Checkout;
use crate::menu::{self, MenuItem};
use crate::native::liquid;
use crate::right_panel::ToggleChanges;
use crate::settings::{self, ChatDetails, DetailsItem, FlipDetailsItem};
use crate::shell::{ellipsized, icon};
use crate::theme::{self, Palette};

actions!(chat, [ToggleDetails]);

/// How wide the card comes.
const WIDTH: f32 = 280.;
/// What it keeps from the pane's edges.
const MARGIN: f32 = 12.;
/// How tall its controls are, and the gap under them.
const CONTROLS: f32 = 28.;
const GAP: f32 = 8.;
/// What it leaves at the bottom for the composer, over the chat.
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
    /// Whether a person asked for it where it has no room beside the chat.
    over: bool,
    /// Whether the subagents that ended show.
    previous: bool,
    /// What a person opened or closed of the plan, by its steps' ids, phases' and sub-steps'
    /// alike: it holds over what a step's status opens.
    opened_steps: HashMap<String, bool>,
    git: git_actions::GitState,
    resize: resize::Resize,
    /// How tall the card was last drawn, border and all, which its bottom is dragged from while
    /// it's as tall as what it shows.
    drawn: Rc<Cell<f32>>,
}

impl Details {
    /// Follows where the chat works, which is new when it's on another branch.
    pub(super) fn set_place(&mut self, place: Workplace, cx: &mut Context<ChatView>) -> bool {
        let new = self.place.as_ref() != Some(&place);
        if new {
            self.git.moved(&place.checkout, cx);
            self.place = Some(place);
        }
        new
    }

    #[cfg(test)]
    pub(super) fn folder(&self) -> Option<PathBuf> {
        self.place.as_ref().map(|place| place.folder.clone())
    }
}

/// The card's controls and, if it's up, the card.
pub(super) fn floating(
    view: &ChatView,
    window: &mut Window,
    cx: &mut Context<ChatView>,
) -> AnyElement {
    let p = theme::palette(cx);
    let up = view.details.is_up(cx);
    let id = cx.entity_id().as_u64();
    let glass = ElementId::NamedInteger("chat-details-controls".into(), id);
    let on_glass = liquid::backdrop(glass, px(CONTROLS / 2.), window, cx);
    let controls = div()
        .relative()
        .flex()
        .items_center()
        .h(px(CONTROLS))
        .px_1()
        .gap_1()
        .rounded_full()
        .occlude();
    let replica = view
        .showing()
        .and_then(|shown| shown.thread.read(cx).replica());
    let share = (!up).then(|| progress::pill(replica, p, cx)).flatten();
    let controls = liquid::floating(controls, on_glass, p)
        .when(up, |controls| controls.child(choosing(view, p, cx)))
        .when_some(share, |controls, share| {
            controls.child(share.on_click(cx.listener(toggled)))
        })
        .child(
            control("chat-details-toggle", IconName::PanelRight, p).on_click(cx.listener(toggled)),
        );
    let card = up.then(|| card(view, window, cx));
    let drawn = view.details.drawn.clone();
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
            .children(card)
            .on_children_prepainted(move |bounds, _, _| {
                if let Some(card) = bounds.get(1) {
                    drawn.set(card.size.height.as_f32());
                }
            }),
    )
    .into_any_element()
}

fn toggled(view: &mut ChatView, _: &ClickEvent, _: &mut Window, cx: &mut Context<ChatView>) {
    view.details.toggle(cx);
    cx.notify();
}

fn control(id: &'static str, mark: IconName, p: Palette) -> Stateful<Div> {
    control_shell(id, p).w(px(CONTROLS - 6.)).child(icon(mark, 14.))
}

/// What a control is drawn in, round and muted until the pointer's on it, as wide as what it
/// holds.
fn control_shell(id: &'static str, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(CONTROLS - 6.))
        .rounded_full()
        .cursor_pointer()
        .text_color(p.muted)
        .hover(move |control| control.bg(p.hover).text_color(p.text))
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
    let replica = view
        .showing()
        .and_then(|shown| shown.thread.read(cx).replica());
    let height = view.details.height(cx);
    let sections: Vec<Div> = [
        choice
            .shows(DetailsItem::Worktree)
            .then(|| worktree(view, p, cx))
            .flatten(),
        choice
            .shows(DetailsItem::OpenIn)
            .then(|| open_in::line(view, p, cx))
            .flatten(),
        choice
            .shows(DetailsItem::Scripts)
            .then(|| scripts::section(view, p, cx))
            .flatten(),
        choice
            .shows(DetailsItem::Git)
            .then(|| git_actions::rows(view, p, cx))
            .flatten(),
        choice
            .shows(DetailsItem::Changes)
            .then(|| changes(view, p, cx))
            .flatten(),
        choice
            .shows(DetailsItem::Subagents)
            .then(|| subagents::section(view, replica, p, cx))
            .flatten(),
        choice
            .shows(DetailsItem::Context)
            .then(|| context(replica, p))
            .flatten(),
        progress::section(view, replica, p, cx),
    ]
    .into_iter()
    .flatten()
    .collect();
    let surface = div()
        .debug_selector(|| "chat-details-card".into())
        .relative()
        .w(px(view.details.width(cx)))
        .when_some(height, |surface, height| surface.h(px(height)))
        .rounded(px(RADIUS))
        .occlude()
        .text_sm();
    let body = div()
        .id("chat-details")
        .map(|body| {
            if height.is_some() {
                body.h_full()
            } else {
                body.max_h(px(view.details.tallest()))
            }
        })
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .p(px(6.))
        .children(sections.into_iter().enumerate().map(|(at, section)| {
            section.when(at > 0, |section| {
                section
                    .mt(px(6.))
                    .pt(px(6.))
                    .border_t_1()
                    .border_color(p.hairline)
            })
        }));
    crate::chat_backdrop::floating(surface, on_glass, p, cx)
        .child(body)
        .children(resize::handles(&view.details, p, cx))
        .into_any_element()
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
        .child(
            div()
                .flex_shrink_0()
                .text_color(p.muted)
                .child(icon(mark, ROW_ICON)),
        )
}

fn clickable(row: Div, id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    row.id(id)
        .cursor_pointer()
        .hover(move |row| row.bg(p.hover))
}

/// "1 file", or "3 files".
fn counted<Count: Copy + Display + PartialEq + From<u8>>(count: Count, thing: &str) -> String {
    if count == Count::from(1) {
        format!("1 {thing}")
    } else {
        format!("{count} {thing}s")
    }
}

fn meta(says: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .flex_shrink_0()
        .text_xs()
        .text_color(color)
        .child(says.into())
}

fn worktree(view: &ChatView, p: Palette, cx: &Context<ChatView>) -> Option<Div> {
    let place = view.details.place.as_ref()?;
    let folder = place.folder.clone();
    let called = if place.is_main {
        "Main checkout".to_owned()
    } else {
        folder
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    };
    let branch = row(IconName::GitBranch, p).child(ellipsized(place.branch.clone()));
    let reveal = clickable(
        row(IconName::Folder, p).child(ellipsized(called)),
        "chat-details-folder",
        p,
    )
    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.reveal_path(&folder)));
    Some(div().flex().flex_col().child(branch).child(reveal))
}

fn changes(view: &ChatView, p: Palette, cx: &Context<ChatView>) -> Option<Div> {
    let place = view.details.place.as_ref()?;
    let checkout = place.checkout.read(cx);
    let (added, removed) = checkout.totals();
    let files = checkout.files().len();
    let says = if files == 0 {
        "No changes".to_owned()
    } else {
        counted(files, "file")
    };
    let line = row(IconName::FileDiff, p).child(div().flex_1().min_w_0().child(says));
    let line = line
        .when(added > 0, |line| {
            line.child(meta(format!("+{added}"), p.added))
        })
        .when(removed > 0, |line| {
            line.child(meta(format!("−{removed}"), p.danger))
        });
    Some(div().child(
        clickable(line, "chat-details-changes", p)
            .debug_selector(|| "chat-details-changes".into())
            .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(ToggleChanges), cx);
            }),
    ))
}

fn context(replica: Option<&Replica>, p: Palette) -> Option<Div> {
    let (held, room) = replica?.context()?;
    let percent = context_percent(held, room);
    let color = if percent >= NEARLY_FULL {
        p.danger
    } else {
        p.muted
    };
    let share = f32::from(percent) / 100.;
    let says = meta(context_line(held, room), p.muted);
    Some(gauge("Context", says, share, color, p))
}

/// A section that says how far along something is: its title and what it says of it, over a bar
/// `share` full of `fill`.
fn gauge(title: &'static str, says: impl IntoElement, share: f32, fill: Hsla, p: Palette) -> Div {
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
                .child(div().text_color(p.text).child(title))
                .child(says),
        )
        .child(
            div()
                .h(px(3.))
                .w_full()
                .rounded_full()
                .bg(p.hairline)
                .child(
                    div()
                        .h_full()
                        .rounded_full()
                        .bg(fill)
                        .w(relative(share)),
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
