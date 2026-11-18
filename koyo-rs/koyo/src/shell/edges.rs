//! The edges a person drags to resize the sidebar and the right panel. What's beside an edge
//! follows the pointer as the shell draws it anew, and the width it was dragged to is saved
//! once, as it's let go.

use gpui_kit::{
    App, Context, DispatchPhase, DragMoveEvent, IntoElement, MouseUpEvent, Styled as _, Window,
    canvas,
};

use super::Shell;
use crate::settings;
use crate::sidebar::SidebarResize;
use crate::theme;

impl Shell {
    /// The sidebar's width as set, or as its edge is being dragged.
    pub(crate) fn sidebar_width(&self, cx: &App) -> f32 {
        (self.sidebar_dragged).unwrap_or_else(|| settings::get(cx).sidebar_width())
    }

    pub(super) fn resize_sidebar(
        &mut self,
        event: &DragMoveEvent<SidebarResize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let width = (event.event.position.x.as_f32() - theme::GUTTER)
            .clamp(theme::SIDEBAR_MIN_WIDTH, theme::SIDEBAR_MAX_WIDTH);
        if self.sidebar_dragged != Some(width) {
            self.sidebar_dragged = Some(width);
            cx.notify();
        }
    }

    /// Hears an edge let go wherever the pointer is. The root's own listeners hear only what's
    /// over the root, and the pointer is over the edge's handle then, which takes the mouse for
    /// itself; the window hears everything.
    pub(super) fn hearing_let_go(cx: &Context<Self>) -> impl IntoElement {
        let shell = cx.weak_entity();
        canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble
                        && let Ok(true) = shell.read_with(cx, |shell, _| shell.holds_an_edge())
                    {
                        (shell.update(cx, Self::let_go)).ok();
                    }
                });
            },
        )
        .absolute()
        .size_0()
    }

    fn holds_an_edge(&self) -> bool {
        self.sidebar_dragged.is_some() || self.right_panel.is_dragged()
    }

    /// Saves the width an edge was dragged to, once it's let go. What's beside the edge is drawn
    /// at that width already, so only a change to the settings draws it again.
    fn let_go(&mut self, cx: &mut Context<Self>) {
        self.right_panel.let_go(cx);
        if let Some(width) = self.sidebar_dragged.take() {
            settings::update(cx, |settings| settings.sidebar_width = width);
        }
    }
}

#[cfg(test)]
mod tests;
