//! The details card beside the chat, where the pane has room for both: the timeline's column and
//! the composer keep to what's left of it at every width, follow its left edge as it's dragged,
//! and the card goes down to the pane's bottom. Where there's no room, it comes up over the chat
//! and leaves the composer where it was.

use gpui_kit::{
    AnyWindowHandle, Bounds, Pixels, TestAppContext, VisualTestContext, point, px, size,
};

use super::super::view::ChatView;
use super::drawing::{card_up, shown_chat};
use crate::settings::{self, ChatWidth};
use crate::test_support::{center_of, drag, draw};

const CARD: &str = "chat-details-card";
/// What the card keeps from the pane's edges and from the chat beside it.
const MARGIN: f32 = 12.;
/// What the card leaves the composer under it, over the chat.
const COMPOSER: f32 = 180.;

fn bounds_of(
    window: AnyWindowHandle,
    what: &'static str,
    cx: &mut TestAppContext,
) -> Bounds<Pixels> {
    let bounds = VisualTestContext::from_window(window, cx).debug_bounds(what);
    bounds.unwrap_or_else(|| panic!("{what} is drawn"))
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() < px(0.5)
}

/// Draws `window` twice: the pane's measured as it's drawn, and the card placed by it in the next.
fn settle(window: AnyWindowHandle, cx: &mut TestAppContext) {
    draw(window, cx);
    draw(window, cx);
}

fn set_width(width: ChatWidth, window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update(|cx| settings::update(cx, |settings| settings.chat_width = width));
    settle(window, cx);
}

/// Drags the card's bottom edge as far down as it goes.
fn tallest(window: AnyWindowHandle, cx: &mut TestAppContext) -> Bounds<Pixels> {
    let from = center_of(window, "chat-details-resize-bottom", cx);
    drag(window, from, from + point(px(0.), px(2000.)), cx);
    settle(window, cx);
    bounds_of(window, CARD, cx)
}

/// The composer's right edge with the card down, up, and down again, at full width.
#[gpui_kit::test]
fn the_card_pushes_the_composer_aside_and_lets_it_back(cx: &mut TestAppContext) {
    let (window, view, _) = shown_chat("chat-beside-pushes", cx);
    set_width(ChatWidth::Full, window, cx);
    if view.read_with(cx, ChatView::details_up) {
        cx.simulate_keystrokes(window, "cmd-shift-b");
    }
    settle(window, cx);
    assert!(!view.read_with(cx, ChatView::details_up), "the card starts down");
    let pane = bounds_of(window, "chat", cx);
    let alone = bounds_of(window, "chat-composer", cx);

    card_up(window, &view, cx);
    settle(window, cx);
    let card = bounds_of(window, CARD, cx);
    let pushed = bounds_of(window, "chat-composer", cx);

    cx.simulate_keystrokes(window, "cmd-shift-b");
    settle(window, cx);
    let back = bounds_of(window, "chat-composer", cx);
    println!(
        "pane right {:?}; card down: composer right {:?}; card up: card left {:?}, composer \
         right {:?}; card down again: composer right {:?}",
        pane.right(),
        alone.right(),
        card.left(),
        pushed.right(),
        back.right(),
    );
    assert!(alone.right() > card.left(), "alone, it runs where the card goes: {alone:?}");
    assert!(pushed.right() < card.left(), "pushed beside the card: {pushed:?} {card:?}");
    assert!(
        near(alone.right() - pushed.right(), pane.right() - card.left() + px(MARGIN)),
        "by the card's width and margin: {alone:?} to {pushed:?}"
    );
    assert_eq!(back, alone, "and back where it was once the card's down");
}

#[gpui_kit::test]
fn beside_the_card_the_chat_keeps_to_its_left_at_every_width(cx: &mut TestAppContext) {
    let (window, view, _) = shown_chat("chat-beside-widths", cx);
    set_width(ChatWidth::Full, window, cx);
    card_up(window, &view, cx);
    settle(window, cx);
    let pane = bounds_of(window, "chat", cx);
    let card = bounds_of(window, CARD, cx);
    let column = bounds_of(window, "chat-row-0", cx);
    let composer = bounds_of(window, "chat-composer", cx);
    assert!(near(column.left(), pane.left()), "{column:?} in {pane:?}");
    assert!(
        near(column.right(), card.left() - px(MARGIN)),
        "a full-width column runs to the card's margin: {column:?} beside {card:?}"
    );
    assert!(
        composer.right() < card.left(),
        "{composer:?} beside {card:?}"
    );

    let from = center_of(window, "chat-details-resize-left", cx);
    drag(window, from, from - point(px(60.), px(0.)), cx);
    settle(window, cx);
    let widened = bounds_of(window, CARD, cx);
    assert!(near(widened.left(), card.left() - px(60.)), "{widened:?}");
    let column = bounds_of(window, "chat-row-0", cx);
    let followed = bounds_of(window, "chat-composer", cx);
    assert!(
        near(column.right(), widened.left() - px(MARGIN)),
        "the column follows the card's edge: {column:?} beside {widened:?}"
    );
    assert!(
        near(followed.right(), composer.right() - px(60.)),
        "and the composer: {followed:?}, from {composer:?}"
    );

    set_width(ChatWidth::Narrow, window, cx);
    let left = widened.left() - px(MARGIN);
    for what in ["chat-row-0", "chat-composer"] {
        let bounds = bounds_of(window, what, cx);
        assert!(
            bounds.right() < widened.left(),
            "{what}: {bounds:?} beside {widened:?}"
        );
        assert!(
            near(bounds.left() - pane.left(), left - bounds.right()),
            "{what} is centred in what the card leaves: {bounds:?} in {pane:?} to {left:?}"
        );
    }
    assert!(near(
        bounds_of(window, "chat-row-0", cx).size.width,
        px(760.)
    ));
}

#[gpui_kit::test]
fn beside_the_chat_the_card_goes_down_to_the_panes_bottom(cx: &mut TestAppContext) {
    let (window, view, _) = shown_chat("chat-beside-tallest", cx);
    card_up(window, &view, cx);
    settle(window, cx);
    let pane = bounds_of(window, "chat", cx);
    let card = tallest(window, cx);
    assert!(
        near(card.bottom(), pane.bottom() - px(MARGIN)),
        "{card:?} in {pane:?}"
    );
    let composer = bounds_of(window, "chat-composer", cx);
    assert!(
        composer.right() < card.left(),
        "and beside the composer: {composer:?}"
    );
}

/// Where the pane has no room for the chat beside the card, the card is as it always was: up
/// over the chat only when asked for, under it the composer as it was, and room kept for it.
#[gpui_kit::test]
fn too_narrow_the_card_comes_up_over_the_chat_and_leaves_it_be(cx: &mut TestAppContext) {
    let (window, view, _) = shown_chat("chat-beside-narrow", cx);
    VisualTestContext::from_window(window, cx).simulate_resize(size(px(900.), px(900.)));
    settle(window, cx);
    let up = |cx: &TestAppContext| view.read_with(cx, ChatView::details_up);
    assert!(!up(cx), "it's down until asked for");
    let composer = bounds_of(window, "chat-composer", cx);
    let column = bounds_of(window, "chat-row-0", cx);

    cx.simulate_keystrokes(window, "cmd-shift-b");
    settle(window, cx);
    assert!(up(cx));
    let card = bounds_of(window, CARD, cx);
    assert!(
        card.left() < composer.right(),
        "it's over the chat: {card:?}"
    );
    assert_eq!(
        bounds_of(window, "chat-composer", cx),
        composer,
        "the composer stays put"
    );
    assert_eq!(
        bounds_of(window, "chat-row-0", cx),
        column,
        "and the timeline"
    );

    let pane = bounds_of(window, "chat", cx);
    let card = tallest(window, cx);
    assert!(
        card.bottom() <= pane.bottom() - px(COMPOSER),
        "it leaves the composer its room: {card:?} in {pane:?}"
    );
}
