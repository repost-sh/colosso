//! What floats over a chat's timeline: the composer, over its end or in the middle of an empty
//! chat, a subagent's bar in its place, the pill back to the end once a person left it, and, as a
//! person scrolls up, the prompt of the section read at the top once its own place went up past it.

use gpui_kit::assets::IconName;
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::component::text::TextView;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, Context, ElementId, InteractiveElement as _, IntoElement,
    ListOffset, ParentElement as _, Pixels, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, deferred, div, px,
};
use koyo_agent::timeline::RowId;

use super::ChatView;
use crate::chat::composer::ChatComposer;
use crate::chat::message::longer_than;
use crate::chat::rows::{
    BUBBLE_RADIUS, PROMPT_ROOM, bubble, markdown, message_of, room_over, show_more,
};
use crate::native::liquid;
use crate::pane::tab_button;
use crate::settings;
use crate::shell::icon;
use crate::theme;
use crate::theme::fonts::at_chat_size;

/// How far the composer floats above the card's bottom edge.
const FLOAT: f32 = 12.;
/// How far the last row keeps from the composer.
const CLEARANCE: f32 = 12.;
/// How many lines of a floating prompt show until it's opened.
const FLOATING_LINES: usize = 3;
/// How tall a floating prompt's card is taken to be until one's laid out: three lines and its
/// footer, at the chat's usual size.
pub(super) const CARD_GUESS: f32 = 100.;

/// The prompt floating over the timeline: the row it heads the section from, which goes by `id`
/// as rows come and go above it, what it says and whether that's cut short, whether it's opened to
/// be read whole, and how far up it is: out of sight as far as a person scrolled down, or as the
/// next prompt pushed it.
#[derive(Clone, Debug)]
pub(in crate::chat) struct Floating {
    pub(in crate::chat) at: usize,
    id: RowId,
    said: SharedString,
    long: bool,
    pub(in crate::chat) open: bool,
    pub(in crate::chat) push: Pixels,
}

/// The prompt heading the section read with the list's top at `top`: where it is among `prompts`,
/// the rows heading a chat's sections, and where its bubble starts under the list's top if its row
/// is in view, the top row.
fn heading(prompts: &[usize], top: ListOffset) -> Option<(usize, Option<Pixels>)> {
    let at = prompts
        .partition_point(|row| *row <= top.item_ix)
        .checked_sub(1)?;
    let row = prompts[at];
    let bubble = (row == top.item_ix).then(|| px(room_over(row, prompts)) - top.offset_in_item);
    Some((at, bubble))
}

/// Which of `prompts` floats, where it is among them, with the list's top at `top` and the card's
/// top at `line` under it: the one heading the section read there, once its bubble has gone up
/// past that line.
pub(in crate::chat) fn floating(prompts: &[usize], top: ListOffset, line: Pixels) -> Option<usize> {
    let (at, bubble) = heading(prompts, top)?;
    bubble.is_none_or(|bubble| bubble < line).then_some(at)
}

/// How far the floating card is above where it floats once a person scrolled `by` down the chat
/// (up, less than nothing), from `hidden`, the card `card` tall: up as they scroll down, down as
/// they scroll up, from all out of sight to its place and no further. While it's out of sight
/// and the prompt it would show is in view, its bubble `bubble` under the list's top, it's held
/// at that bubble, which comes down into view itself rather than under a card.
pub(in crate::chat) fn hidden_after(
    hidden: Pixels,
    by: Pixels,
    card: Pixels,
    bubble: Option<Pixels>,
) -> Pixels {
    let out = card + px(PROMPT_ROOM);
    let at_bubble = bubble.map_or(Pixels::ZERO, |bubble| px(PROMPT_ROOM) - bubble);
    let most = out.max(at_bubble);
    let moved = (hidden.min(most) + by).clamp(Pixels::ZERO, most);
    if hidden >= out {
        moved.max(at_bubble)
    } else {
        moved
    }
}

/// How far the floating card goes up ahead of the next prompt, whose row is `distance` below the
/// list's top: as far as keeps the card, `height` tall, a prompt's room above the next bubble, so
/// the two never overlap. Once that one's at the top, the card's gone up past it.
pub(in crate::chat) fn pushed(distance: Pixels, height: Pixels) -> Pixels {
    (distance - px(PROMPT_ROOM) - height).min(Pixels::ZERO)
}

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
            .map(|held| settings::get(cx).chat_width.column_of(held))
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

    /// Takes up which prompt floats now, as the list's top says and as far as a person's scrolling
    /// brought its card down, none with the setting off, and how far the next one pushes it up,
    /// and draws the tab again if that changed; the timeline leaves the prompt's place empty while
    /// its card shows. Following the end, nothing floats until a person scrolls up; dragging the
    /// scrollbar moves the card as scrolling does. A prompt that still floats stays opened if it
    /// was, though rows came above it.
    pub(super) fn float(&mut self, cx: &mut Context<Self>) {
        let Some(shown) = self.shown.last() else {
            return;
        };
        let scroller = shown.scroller.read(cx);
        let timeline = shown.timeline.read(cx);
        let prompts = &timeline.prompts;
        let (top, offset, card) = (
            scroller.logical_scroll_top(),
            scroller.scroll_offset(),
            self.card_height.get(),
        );
        if scroller.is_following_tail() {
            self.hidden = Pixels::MAX;
        } else if scroller.is_scrollbar_dragging() {
            let bubble = heading(prompts, top).and_then(|(_, bubble)| bubble);
            self.hidden = hidden_after(self.hidden, offset - self.scrolled_to, card, bubble);
        }
        self.scrolled_to = offset;
        let hidden = self.hidden;
        let push = |at: usize| {
            let next = prompts
                .get(at + 1)
                .and_then(|row| scroller.bounds_for_item(*row));
            let pushed_up =
                next.map(|row| pushed(row.top() - scroller.viewport_bounds().top(), card));
            pushed_up.unwrap_or_default().min(-hidden)
        };
        let row = (floating(prompts, top, px(PROMPT_ROOM) - hidden))
            .filter(|_| settings::get(cx).floating_prompts)
            .map(|at| (prompts[at], push(at)))
            // All the way up, its card's out of sight, and its place stays drawn.
            .filter(|(_, push)| px(PROMPT_ROOM) + *push + card > Pixels::ZERO)
            .and_then(|(at, push)| Some((at, push, timeline.rows.get(at)?)));
        let lifted = row.map(|(at, ..)| at);
        let moved = match (&mut self.floating, row) {
            (Some(held), Some((at, push, row))) if held.id == row.id => {
                let moved = (held.at, held.push) != (at, push);
                (held.at, held.push) = (at, push);
                moved
            }
            (held, row) => {
                let replica = shown.thread.read(cx).replica();
                let next = row.and_then(|(at, push, row)| {
                    let (_, said) = message_of(&row.kind, replica?)?;
                    Some(Floating {
                        at,
                        id: row.id.clone(),
                        said: said.text.clone().into(),
                        long: longer_than(&said.text, FLOATING_LINES),
                        open: false,
                        push,
                    })
                });
                let moved = held.is_some() || next.is_some();
                *held = next;
                moved
            }
        };
        // Told only of a change: an entity updated at all is taken as changed, which a scroll's
        // frame mustn't be.
        if timeline.floating != lifted {
            let timeline = shown.timeline.clone();
            timeline.update(cx, |timeline, cx| timeline.set_floating(lifted, cx));
        }
        if moved {
            cx.notify();
        }
    }

    /// The list moved: if a person's wheel, trackpad or touch moved it, the card comes down as far
    /// as they scrolled up and goes up as far as they scrolled down; if something else did, a jump,
    /// the card's out of sight until they scroll up there.
    pub(super) fn reveal(&mut self, cx: &mut Context<Self>) {
        let Some(shown) = self.shown.last() else {
            return;
        };
        let scroller = shown.scroller.read(cx);
        let by = scroller.scroll_offset() - self.scrolled_to;
        let scrolls = scroller.hand_scrolls();
        if std::mem::replace(&mut self.hand_scrolls, scrolls) != scrolls {
            let prompts = &shown.timeline.read(cx).prompts;
            let top = scroller.logical_scroll_top();
            let bubble = heading(prompts, top).and_then(|(_, bubble)| bubble);
            self.hidden = hidden_after(self.hidden, by, self.card_height.get(), bubble);
        } else if by != Pixels::ZERO {
            self.hidden = Pixels::MAX;
        }
    }

    /// The prompt floating at the top of the timeline, where its bubble would be, on glass: cut to
    /// a few lines until it's opened, which it is in place, and a way to where it really is.
    pub(super) fn floating_prompt(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let floating = self.floating.as_ref()?;
        let p = theme::palette(cx);
        let words = TextView::markdown(("chat-floating", floating.at), floating.said.clone());
        let words = markdown(words, &self.cwd).when(floating.long && !floating.open, |words| {
            words.max_lines(FLOATING_LINES)
        });
        let more = floating.long.then(|| {
            show_more("chat-floating-more", floating.open, p)
                .debug_selector(|| "chat-floating-more".into())
                .on_click(cx.listener(|this, _, _, cx| this.open_floating(cx)))
        });
        let goes = tab_button("chat-floating-go", p)
            .debug_selector(|| "chat-floating-go".into())
            .cursor_pointer()
            .tooltip(|window, cx| Tooltip::new("Scroll to this message").build(window, cx))
            .child(icon(IconName::ArrowUp, 12.))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go_to_floating(cx)));
        let keyed = ElementId::NamedInteger("chat-prompt-glass".into(), cx.entity_id().as_u64());
        let on_glass = liquid::backdrop(keyed, px(BUBBLE_RADIUS), window, cx);
        let height = self.card_height.clone();
        let card = bubble()
            .id("chat-floating-prompt")
            .debug_selector(|| "chat-floating-prompt".into())
            .relative()
            .top(floating.push)
            .flex()
            .flex_col()
            .max_h_full()
            .occlude()
            .text_color(p.text);
        let card = liquid::floating(card, on_glass, p)
            .child(
                div()
                    .id("chat-floating-words")
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(words),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .pt_1()
                    .child(div().children(more))
                    .child(goes),
            );
        let column = (settings::get(cx).chat_width.column_of(div()))
            .w_full()
            .h_full()
            .pt(px(PROMPT_ROOM))
            .px_6()
            .flex()
            .flex_col()
            .items_end()
            .child(card)
            // Measured whole, its edge included, for how far the next prompt pushes it.
            .on_children_prepainted(move |cards, _, _| {
                height.set(cards.first().map_or(Pixels::ZERO, |card| card.size.height));
            });
        // Pushed up, it goes out of the top of the chat rather than over what's above it.
        let place = div()
            .absolute()
            .top_0()
            .bottom(self.inset())
            .left_0()
            .right_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .items_center()
            .child(at_chat_size(column, cx));
        Some(deferred(place).into_any_element())
    }

    /// The floating prompt opens to be read whole, or closes again.
    pub(in crate::chat) fn open_floating(&mut self, cx: &mut Context<Self>) {
        if let Some(floating) = &mut self.floating {
            floating.open = !floating.open;
            cx.notify();
        }
    }

    /// The chat goes to where the floating prompt is, which puts it in its place.
    pub(in crate::chat) fn go_to_floating(&mut self, cx: &mut Context<Self>) {
        let at = self.floating.as_ref().map(|floating| floating.at);
        let Some((shown, at)) = self.showing().zip(at) else {
            return;
        };
        (shown.scroller).update(cx, |scroller, cx| scroller.scroll_to_item(at, cx));
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
