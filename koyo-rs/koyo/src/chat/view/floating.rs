//! What floats over a chat's timeline: the composer, over its end or in the middle of an empty
//! chat, a subagent's bar in its place, and the pill back to the end once a person left it.

use gpui_kit::assets::IconName;
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, Context, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, StatefulInteractiveElement as _, Styled as _, Window, deferred,
    div, px,
};

use super::ChatView;
use crate::chat::composer::ChatComposer;
use crate::native::liquid;
use crate::shell::icon;
use crate::theme;

/// How far the composer floats above the card's bottom edge.
const FLOAT: f32 = 12.;
/// How far the last row keeps from the composer.
const CLEARANCE: f32 = 12.;

impl ChatView {
    /// How far the timeline's last row has to stay from the bottom to clear the glass.
    pub(super) fn inset(&self) -> Pixels {
        self.composer_height + px(FLOAT + CLEARANCE)
    }

    /// The composer: over the end of the timeline, or in the middle of an empty chat.
    pub(super) fn composer_in_place(&self, cx: &Context<Self>) -> AnyElement {
        let place = div()
            .absolute()
            .left_0()
            .right_0()
            .flex()
            .flex_col()
            .items_center()
            .on_children_prepainted(self.measures_composer(cx));
        if let Some(bar) = self.bar() {
            return deferred(place.bottom(px(FLOAT)).child(bar.clone())).into_any_element();
        }
        let composer = self.composer.read(cx);
        let place = if composer.is_new() {
            place.top_0().bottom_0().justify_center().pb(px(48.))
        } else {
            place.bottom(px(FLOAT))
        };
        // The edge that shows the chat working is laid over the composer, as wide as it is, rather
        // than drawn in it, so its steps draw this view and not the composer
        // (`composer/activity.rs`).
        let held = div()
            .relative()
            .w_full()
            .map(|held| crate::settings::get(cx).chat_width.column_of(held))
            .child(self.composer.clone())
            .child(composer.activity.clone());
        deferred(place.child(held)).into_any_element()
    }

    /// The composer's height, once it's laid out, for the timeline to keep clear of it. Told a
    /// frame late, since the timeline drew first.
    fn measures_composer(
        &self,
        cx: &Context<Self>,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let view = cx.weak_entity();
        let held = self.composer_height;
        move |children, _, cx| {
            let height = children
                .first()
                .map_or(Pixels::ZERO, |child| child.size.height);
            if height != held {
                let view = view.clone();
                cx.defer(move |cx| {
                    view.update(cx, |view, cx| view.composer_measured(height, cx))
                        .ok();
                });
            }
        }
    }

    fn composer_measured(&mut self, height: Pixels, cx: &mut Context<Self>) {
        self.composer_height = height;
        let inset = self.inset();
        if let Some(shown) = self.showing() {
            shown
                .timeline
                .update(cx, |timeline, cx| timeline.set_inset(inset, cx));
        }
        cx.notify();
    }

    /// The pill back to the end of the timeline, once a person left it.
    pub(super) fn scroll_pill(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let composer = self.composer_on_show().map(|composer| composer.read(cx));
        if !self.pill || composer.is_some_and(ChatComposer::suggesting) {
            return None;
        }
        let queued = composer.map_or(0, ChatComposer::queued);
        let p = theme::palette(cx);
        let keyed = ElementId::NamedInteger("chat-pill-glass".into(), cx.entity_id().as_u64());
        let on_glass = liquid::backdrop(keyed, px(14.), window, cx);
        let pill = div()
            .id("chat-scroll-to-end")
            .relative()
            .flex()
            .items_center()
            .gap_1()
            .h(px(28.))
            .px_3()
            .rounded_full()
            .occlude()
            .cursor_pointer()
            .text_xs()
            .text_color(p.text);
        let pill = liquid::floating(pill, on_glass, p)
            .child(icon(IconName::ChevronDown, 13.))
            .child("Scroll to end")
            .when(queued > 0, |pill| pill.child(format!("· {queued} queued")))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                if let Some(shown) = this.showing() {
                    shown
                        .scroller
                        .update(cx, MessageScrollerState::scroll_to_end);
                }
            }));
        let bar = div()
            .absolute()
            .bottom(self.inset())
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(pill);
        Some(deferred(bar).into_any_element())
    }
}
