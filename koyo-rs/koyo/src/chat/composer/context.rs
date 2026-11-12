//! How full the conversation is (`docs/chat.md` 5.3): a ring in the footer that says so to the
//! pointer, and whose press opens that and the way to compact it, on a menu's surface.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, SharedString, Size,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div, px, size,
};
use koyo_agent::words::{NEARLY_FULL, context_line, context_percent};

use super::ChatComposer;
use crate::menu::{self, MenuBody, ROW_PADDING};
use crate::ring::ring;
use crate::theme::{self, Palette};

const PANEL_WIDTH: f32 = 210.;
const PANEL_HEIGHT: f32 = 2. * theme::ROW_HEIGHT;

/// What the menu holds: how full it was as the menu opened, and the row that compacts it.
struct ContextPanel {
    composer: WeakEntity<ChatComposer>,
    line: SharedString,
}

impl MenuBody for ContextPanel {
    fn hears(&mut self, _: &str, _: &mut Context<Self>) -> bool {
        false
    }

    fn extent(&self, _: &Window, _: &App) -> Size<Pixels> {
        size(px(PANEL_WIDTH), px(PANEL_HEIGHT))
    }
}

impl Render for ContextPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let composer = self.composer.clone();
        div()
            .size_full()
            .child(
                div()
                    .h(px(theme::ROW_HEIGHT))
                    .px(px(ROW_PADDING))
                    .flex()
                    .items_center()
                    .text_xs()
                    .text_color(p.muted)
                    .child(self.line.clone()),
            )
            .child(
                menu::row("chat-compact")
                    .hover(|row| row.bg(p.selected))
                    .child("Compact now")
                    .on_click(move |_, _, cx| {
                        // Once the menu's window is done with what got here.
                        let composer = composer.clone();
                        cx.defer(move |cx| {
                            composer
                                .update(cx, |composer, cx| {
                                    composer.context_menu.close(cx);
                                    composer.compact(cx);
                                })
                                .ok();
                        });
                    }),
            )
    }
}

impl ChatComposer {
    /// The ring, once a run has said how full the conversation is.
    pub(super) fn context_ring(&self, p: Palette, cx: &Context<Self>) -> Option<AnyElement> {
        let (held, room) = self.context?;
        let percent = context_percent(held, room);
        let color = if percent >= NEARLY_FULL {
            p.danger
        } else {
            p.muted
        };
        let line = SharedString::from(context_line(held, room));
        let shown = div()
            .id("chat-context-ring")
            .flex_shrink_0()
            .cursor_pointer()
            .tooltip(move |window, cx| Tooltip::new(line.clone()).build(window, cx))
            .child(ring(percent).text_color(color));
        let pressed = cx.listener(|this, _, window, cx| this.toggle_context(window, cx));
        Some(self.context_menu.wraps(shown, pressed).into_any_element())
    }

    pub(super) fn toggle_context(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((held, room)) = self.context else {
            return;
        };
        self.suggest = None;
        let composer = cx.weak_entity();
        let line = context_line(held, room).into();
        self.context_menu.toggle_panel(
            |cx| cx.new(|_| ContextPanel { composer, line }),
            window,
            cx,
        );
        window.focus(&self.focus_handle(cx), cx);
        cx.notify();
    }
}
