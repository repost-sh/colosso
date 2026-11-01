//! The activity panel's place beside its terminal (`docs/terminals.md` 7.4): shown per
//! terminal, off until asked for by the bar's "Activity" or ⌘⇧L, about 30% of the pane and
//! dragged wider or narrower by its left edge.

use gpui_kit::{
    AnyElement, App, AppContext as _, Context, DragMoveEvent, Empty, InteractiveElement as _,
    IntoElement, KeyBinding, ParentElement as _, Render, Styled as _, Window, actions, div, px,
    relative,
};

use super::panel::ActivityPanel;
use crate::shell::Shell;
use crate::sidebar::resize_handle;
use crate::theme::Palette;

actions!(terminal_panel, [ToggleActivity]);

/// The panel's share of the pane until the person drags it, and the least and most it takes.
const SHARE: f32 = 0.3;
const MIN_SHARE: f32 = 0.15;
const MAX_SHARE: f32 = 0.7;

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "cmd-shift-l",
        ToggleActivity,
        Some(colosso_term::CONTEXT),
    )]);
}

/// The drag payload while a terminal's panel edge is dragged: the terminal tab's id.
#[derive(Clone)]
pub struct PanelResize(u64);

impl Render for PanelResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl Shell {
    /// Shows terminal tab `id`'s activity panel, or puts it away: closed, it's dropped, so
    /// it reads nothing.
    pub fn toggle_activity(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.terminals.panels.remove(&id).is_none() {
            let Some(client) = self.terminals.client() else {
                return;
            };
            let Some(handle) = self.terminals.handle_of(id).cloned() else {
                return;
            };
            let panel = cx.new(|cx| ActivityPanel::new(client, handle, cx));
            self.terminals.panels.insert(id, panel);
        }
        cx.notify();
    }

    /// Terminal tab `id`'s view with its panel beside it when it's shown.
    pub(super) fn beside_panel(
        &self,
        id: u64,
        view: AnyElement,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(panel) = self.terminals.panels.get(&id) else {
            return view;
        };
        let share = self.terminals.panel_share.unwrap_or(SHARE);
        div()
            .size_full()
            .flex()
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<PanelResize>, _, cx| {
                    if event.drag(cx).0 != id {
                        return;
                    }
                    let bounds = event.bounds;
                    let share = (bounds.right() - event.event.position.x) / bounds.size.width;
                    this.terminals.panel_share = Some(share.clamp(MIN_SHARE, MAX_SHARE));
                    cx.notify();
                }),
            )
            .child(div().flex_1().min_w_0().h_full().child(view))
            .child(
                div()
                    .relative()
                    .h_full()
                    .w(relative(share))
                    .flex_shrink_0()
                    .border_l_1()
                    .border_color(p.hairline)
                    .bg(p.card)
                    .child(panel.clone())
                    .child(resize_handle("terminal-panel-edge", PanelResize(id)).left(px(-3.))),
            )
            .into_any_element()
    }
}
