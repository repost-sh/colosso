//! What a terminal shows of its state (`docs/terminals.md` 7.4): a dot on its tab, and over
//! its pane a bar with its status, who controls it with "Take control" or "Give back", who
//! watches, the notice of a takeover by typing, and Stop (Close once the program ended). The
//! buttons ask the daemon straight away; what changes comes back as its events.

use std::sync::Arc;

use gpui_kit::component::button::Button;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Div, Hsla, InteractiveElement as _, ParentElement as _,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, div, px,
};
use koyo_term::{TermClient, ToggleResponsive};
use koyo_term_control::{RequestBody, Status};

use super::responsive::ResponsiveTerminal;
use super::states::{State, most_urgent};
use crate::model::Tab;
use crate::pane;
use crate::responsive::bar::toggle_button;
use crate::shell::{Shell, ellipsized};
use crate::theme::Palette;

/// The bar's height.
pub const BAR: f32 = 26.;

impl Shell {
    /// A tab's dot where its row shows it: after a single pane's icon (the first), or ahead
    /// of a split's chips (the second). A terminal's dot is there whatever it's doing and a
    /// chat's mark only when it has something to say, which goes first unless a terminal waits
    /// for the person.
    pub fn tab_dots(&self, tab: &Tab, p: Palette, cx: &mut App) -> (Option<Div>, Option<Div>) {
        let status = most_urgent(self.tab_terminal_statuses(tab));
        let mark = (status != Some(Status::NeedsInput))
            .then(|| self.chat_tab_mark(tab, p, cx))
            .flatten();
        let dot = mark.or_else(|| status.map(|status| dot(status, p)));
        match tab {
            Tab::Single(_) => (dot, None),
            Tab::Split(_) => (None, dot),
        }
    }

    /// Whether a terminal of `tab` waits for the person.
    pub fn tab_terminal_waits(&self, tab: &Tab) -> bool {
        self.tab_terminal_statuses(tab)
            .any(|status| status == Status::NeedsInput)
    }

    /// What `tab`'s terminals are doing, those the daemon has told of.
    fn tab_terminal_statuses(&self, tab: &Tab) -> impl Iterator<Item = Status> {
        (tab.panes().into_iter())
            .filter_map(|pane| pane.terminal)
            .filter_map(|id| self.terminal_state(id))
            .map(|state| state.status)
    }

    /// How many terminals the daemon keeps for the checkout at `worktree`.
    pub fn terminal_count(&self, worktree: &std::path::Path) -> usize {
        self.terminals.states.count_in(worktree)
    }

    /// How many terminals of the checkout at `worktree` wait for the person.
    pub fn terminals_needing_input(&self, worktree: &std::path::Path) -> usize {
        self.terminals.states.needing_input(worktree)
    }

    /// Whether the daemon keeps terminals for the checkout at `worktree`.
    pub fn has_terminals(&self, worktree: &std::path::Path) -> bool {
        self.terminal_count(worktree) > 0
    }

    fn terminal_state(&self, id: u64) -> Option<&State> {
        self.terminals.states.get(self.terminals.handle_of(id)?)
    }

    /// The bar over terminal pane `id`, once its terminal is known.
    pub(super) fn terminal_bar(
        &self,
        id: u64,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let handle = self.terminals.handle_of(id)?;
        let state = self.terminal_state(id)?;
        let client = self.terminals.client.clone()?;
        let key = usize::try_from(id).unwrap_or(usize::MAX);
        let takeover = self
            .terminals
            .states
            .takeover(handle)
            .map(|t| t.from.clone());
        let observers = state.control.observers.len();
        let (holder, holder_color) = if state.yours() {
            ("yours".to_owned(), p.accent)
        } else if let Some(agent) = state.agent() {
            (agent.label.clone(), p.modified)
        } else {
            ("no one controls it".to_owned(), p.muted)
        };
        let (label, request) = if state.yours() {
            (
                "Give back",
                RequestBody::Release {
                    handle: handle.clone(),
                },
            )
        } else {
            let claim = RequestBody::Claim {
                handle: handle.clone(),
                takeover: false,
                reason: None,
            };
            ("Take control", claim)
        };
        Some(
            div()
                .id(("terminal-bar", key))
                .h(px(BAR))
                .flex_shrink_0()
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .text_xs()
                .text_color(p.muted)
                .border_b_1()
                .border_color(p.hairline)
                // A narrow pane shortens what the bar says, never its buttons.
                .overflow_hidden()
                .child(dot(state.status, p))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(p.text)
                        .child(words(state.status)),
                )
                .when(!state.reason.is_empty(), |bar| {
                    bar.child(ellipsized(state.reason.clone()))
                })
                .child(div().flex_1())
                // The chip's own button gives it back.
                .when_some(takeover, |bar, from| {
                    bar.child(
                        ellipsized(format!("You took over from {from}")).text_color(p.modified),
                    )
                })
                .when(observers > 0, |bar| {
                    bar.child(ellipsized(format!("watching: {observers}")))
                })
                .child(
                    ellipsized(holder)
                        .px_1p5()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(holder_color.opacity(0.5))
                        .bg(holder_color.opacity(0.12))
                        .text_color(holder_color),
                )
                .child(button(
                    ("terminal-control", key),
                    label,
                    &client,
                    request,
                    p,
                ))
                .children(self.responsive_toggle(id, key, cx))
                .child(
                    div()
                        .id(("terminal-activity", key))
                        .flex_shrink_0()
                        .px_1p5()
                        .rounded(px(4.))
                        .text_color(p.text)
                        .cursor_pointer()
                        .when(self.terminals.panels.contains_key(&id), |toggle| {
                            toggle.bg(p.selected)
                        })
                        .hover(|style| style.bg(p.hover))
                        .tooltip(|window, cx| Tooltip::new("Activity  ⌘⇧L").build(window, cx))
                        .child("Activity")
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_activity(id, cx))),
                )
                .child(stop_button(id, key, state.status == Status::Exited, p, cx)),
        )
    }
}

impl Shell {
    /// The toggle of responsive mode (`docs/responsive.md` 3.3), the browser toolbar's: pins
    /// the terminal at what the pane holds, or lets it fill the pane again; selected while
    /// it's pinned.
    fn responsive_toggle(&self, id: u64, key: usize, cx: &App) -> Option<Button> {
        let made = self.terminals.views.get(&id)?;
        let responsive = made.responsive.clone();
        let pinned = made.view.read(cx).pinned().is_some();
        Some(
            toggle_button(
                ("terminal-responsive", key),
                pinned,
                &ToggleResponsive,
                koyo_term::CONTEXT,
                cx,
            )
            .on_click(move |_, _, cx: &mut App| {
                responsive.update(cx, ResponsiveTerminal::toggle);
            }),
        )
    }
}

/// Stop, which ends the program and closes the terminal; Close once the program ended.
fn stop_button(
    id: u64,
    key: usize,
    exited: bool,
    p: Palette,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    div()
        .flex_shrink_0()
        .id(("terminal-stop", key))
        .px_1p5()
        .rounded(px(4.))
        .text_color(p.text)
        .cursor_pointer()
        .hover(|style| style.bg(p.hover))
        .active(|style| style.bg(p.selected))
        .tooltip(|window, cx| {
            Tooltip::new("End its program and close the terminal").build(window, cx)
        })
        .child(if exited { "Close" } else { "Stop" })
        .on_click(cx.listener(move |this, _, window, cx| {
            this.stop_terminal(id, window, cx);
        }))
}

/// A small text button that sends `request` to the daemon.
fn button(
    id: (&'static str, usize),
    label: &'static str,
    client: &Arc<TermClient>,
    request: RequestBody,
    p: Palette,
) -> Stateful<Div> {
    let client = Arc::clone(client);
    div()
        .flex_shrink_0()
        .id(id)
        .px_1p5()
        .rounded(px(4.))
        .text_color(p.text)
        .cursor_pointer()
        .hover(|style| style.bg(p.hover))
        .active(|style| style.bg(p.selected))
        .child(label)
        .on_click(move |_, _, cx: &mut App| send(&client, request.clone(), cx))
}

fn send(client: &Arc<TermClient>, request: RequestBody, cx: &mut App) {
    let client = Arc::clone(client);
    cx.background_spawn(async move {
        if let Err(error) = client.request(request) {
            log::warn!("the daemon refused: {error}");
        }
    })
    .detach();
}

/// The dot a status shows as.
pub fn dot(status: Status, p: Palette) -> Div {
    pane::dot(color(status, p))
}

fn color(status: Status, p: Palette) -> Hsla {
    match status {
        Status::Running => p.added,
        Status::NeedsInput => p.modified,
        Status::Exited => p.danger,
        Status::Idle | Status::Unknown => p.muted.opacity(0.6),
    }
}

pub(super) fn words(status: Status) -> SharedString {
    status.words().into()
}
