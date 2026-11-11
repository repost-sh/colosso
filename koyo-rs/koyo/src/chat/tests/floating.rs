//! A chat's prompts floating over its timeline (`docs/chat.md` 5.3): none until a person scrolls
//! up, the card coming down as far as they scroll up and going as they scroll down, shown once,
//! pushed up ahead of the next prompt, opened in place and gone to, and none with the setting off.

use gpui_kit::{
    AnyWindowHandle, Entity, ListOffset, Modifiers, Pixels, ScrollDelta, ScrollEvent,
    TestAppContext, VisualTestContext, point, px,
};
use koyo_agent::protocol::ids::{ItemId, RunId, ThreadId, Time};
use koyo_agent::protocol::methods::ThreadSnapshot;
use koyo_agent::protocol::model::{
    Delivered, Item, ItemBody, ItemStatus, RunStatus, Text, UserMessage,
};

use super::super::view::ChatView;
use super::super::view::floating::{floating, hidden_after, pushed};
use super::drawing::shell_with_chat;
use super::{AT, answer, run, snapshot};
use crate::settings;
use crate::test_support::draw;

#[test]
fn the_prompt_of_the_section_at_the_top_floats_once_its_place_has_gone_past_the_card() {
    let prompts = [0, 5, 10];
    let at = |item_ix, offset: f32, line: f32| {
        let top = ListOffset {
            item_ix,
            offset_in_item: px(offset),
        };
        floating(&prompts, top, px(line)).map(|at| prompts[at])
    };
    // The card in place, 8 pt down.
    assert_eq!(at(0, 0., 8.), None, "at the top the first is in place");
    assert_eq!(
        at(0, 8., 8.),
        None,
        "the room over the first row goes by first"
    );
    assert_eq!(at(0, 9., 8.), Some(0));
    assert_eq!(at(3, 40., 8.), Some(0), "inside its section");
    assert_eq!(
        at(5, 0., 8.),
        None,
        "the next has come to the top: it settles"
    );
    assert_eq!(
        at(5, 1., 8.),
        Some(5),
        "and lifts out as its place goes past"
    );
    assert_eq!(at(12, 0., 8.), Some(10));
    // The card partly out of sight, its top 50 pt up: a bubble under that is where it is.
    assert_eq!(
        at(5, 30., -50.),
        None,
        "30 pt past the top, still above the card"
    );
    assert_eq!(at(5, 70., -50.), Some(5));
    assert_eq!(at(7, 0., -50.), Some(5), "its row all gone by");
    let above = ListOffset {
        item_ix: 1,
        offset_in_item: px(30.),
    };
    assert_eq!(
        floating(&[2], above, px(8.)),
        None,
        "above the first prompt, nothing"
    );
    assert_eq!(floating(&[], above, px(8.)), None);
}

#[test]
fn the_next_prompt_pushes_the_floating_one_up_ahead_of_it() {
    let card = px(60.);
    assert_eq!(
        pushed(px(400.), card),
        px(0.),
        "far below, it stays where it floats"
    );
    assert_eq!(
        pushed(px(68.), card),
        px(0.),
        "its bubble a prompt's room under the card"
    );
    assert_eq!(
        pushed(px(40.), card),
        px(-28.),
        "closer, the card goes up as far"
    );
    assert_eq!(
        pushed(px(0.), card),
        px(-68.),
        "at the top, the card's above it, out of sight"
    );
}

#[test]
fn the_card_comes_down_as_far_as_a_person_scrolls_up_and_goes_as_they_scroll_down() {
    let card = px(100.);
    // Out of sight at first; 30 pt up brings 30 pt of it down.
    let hidden = hidden_after(Pixels::MAX, px(-30.), card, None);
    assert_eq!(hidden, px(78.));
    assert_eq!(
        hidden_after(hidden, px(-200.), card, None),
        px(0.),
        "no further than its place"
    );
    assert_eq!(
        hidden_after(px(0.), px(40.), card, None),
        px(40.),
        "40 pt down, 40 pt up"
    );
    assert_eq!(
        hidden_after(px(40.), px(500.), card, None),
        px(108.),
        "all out of sight and no further, so it comes back at the first turn up"
    );
    // Out of sight, with the prompt itself still in view, its bubble 150 pt up: the card's held at
    // that bubble, which comes down by itself.
    assert_eq!(
        hidden_after(Pixels::MAX, px(-20.), card, Some(px(-150.))),
        px(158.)
    );
    assert_eq!(
        hidden_after(px(108.), px(-20.), card, Some(px(-60.))),
        px(88.)
    );
}

/// The person's message `id`, saying `text`, in run `run`.
fn said(id: &str, run: &str, ordinal: u64, text: &str, delivered: Delivered) -> Item {
    Item {
        run_id: RunId(run.into()),
        status: ItemStatus::Completed,
        body: ItemBody::UserMessage(UserMessage {
            message_id: koyo_agent::protocol::ids::MessageId(format!("m-{id}")),
            text: text.into(),
            attachments: Vec::new(),
            delivered,
        }),
        ..answer(id, ordinal, "", ordinal)
    }
}

/// Three runs, each a prompt and an answer of many paragraphs, the second prompt long and the
/// third with a steer in its run.
fn three_runs(id: &str) -> ThreadSnapshot {
    let long = "- A point the prompt makes.\n".repeat(6);
    let reply = "A paragraph of the answer, which goes on for a line.\n\n".repeat(40);
    let mut items = Vec::new();
    let mut runs = Vec::new();
    for (at, prompt) in ["First", long.as_str(), "Third"].into_iter().enumerate() {
        let run_id = format!("r{at}");
        let ordinal = at as u64 * 3;
        items.push(said(
            &format!("u{at}"),
            &run_id,
            ordinal,
            prompt,
            Delivered::Turn,
        ));
        if at == 2 {
            items.push(said(
                "s",
                &run_id,
                ordinal + 1,
                "Not that one.",
                Delivered::Steer,
            ));
        }
        items.push(Item {
            run_id: RunId(run_id.clone()),
            status: ItemStatus::Completed,
            body: ItemBody::AssistantText(Text {
                text: reply.clone(),
            }),
            ..answer(&format!("a{at}"), ordinal + 2, "", ordinal + 2)
        });
        runs.push(koyo_agent::protocol::model::Run {
            thread_id: ThreadId(id.into()),
            started_at: Some(Time(AT.into())),
            ended_at: Some(Time(AT.into())),
            ..run(&run_id, RunStatus::Completed, prompt, 3)
        });
    }
    ThreadSnapshot {
        runs,
        ..snapshot(id, 10, items)
    }
}

/// A window showing the three runs, at their end, following it.
fn shown(name: &str, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<ChatView>) {
    let (window, _, view, _) = shell_with_chat(name, three_runs("0001"), cx);
    settled(window, cx);
    (window, view)
}

/// Draws until what floats has been taken up and drawn.
fn settled(window: AnyWindowHandle, cx: &mut TestAppContext) {
    draw(window, cx);
    draw(window, cx);
}

/// The rows of the prompts, which are the first, the user messages of each run but the steer.
fn prompts(view: &Entity<ChatView>, cx: &TestAppContext) -> Vec<usize> {
    view.read_with(cx, |view, cx| {
        (view.showing().unwrap().timeline.read(cx).prompts).to_vec()
    })
}

fn scroll_to(
    view: &Entity<ChatView>,
    row: usize,
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
) {
    let scroller = view.read_with(cx, |view, _| view.showing().unwrap().scroller.clone());
    scroller.update(cx, |scroller, cx| scroller.scroll_to_item(row, cx));
    settled(window, cx);
}

fn top(view: &Entity<ChatView>, cx: &TestAppContext) -> ListOffset {
    view.read_with(cx, |view, cx| {
        view.showing()
            .unwrap()
            .scroller
            .read(cx)
            .logical_scroll_top()
    })
}

/// The row of the prompt that floats, if one's drawn floating; its place in the list is empty
/// meanwhile, so it shows once.
fn floats(
    view: &Entity<ChatView>,
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
) -> Option<usize> {
    let card = VisualTestContext::from_window(window, cx).debug_bounds("chat-floating-prompt");
    let (at, lifted) = view.read_with(cx, |view, cx| {
        let floating = view.floating.as_ref().map(|floating| floating.at);
        (floating, view.showing().unwrap().timeline.read(cx).floating)
    });
    assert_eq!(
        at, lifted,
        "the row a prompt floats from is left empty, and only that one"
    );
    card.and(at)
}

/// A turn of the wheel over the timeline, `by` down the chat (up, less than nothing).
/// The chat gone to `row`, then scrolled 600 pt down into what's there and 300 back up, which
/// brings a card all the way down.
fn scrolled_back_into(
    view: &Entity<ChatView>,
    row: usize,
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
) {
    scroll_to(view, row, window, cx);
    wheel(600., window, cx);
    wheel(-300., window, cx);
}

fn wheel(by: f32, window: AnyWindowHandle, cx: &mut TestAppContext) {
    VisualTestContext::from_window(window, cx).simulate_event(ScrollEvent {
        // Over the timeline, in the middle of the window.
        position: point(px(700.), px(400.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-by))),
        ..ScrollEvent::default()
    });
    settled(window, cx);
    // How far the next prompt pushes the card follows its height as it was last laid out.
    settled(window, cx);
}

/// How far up the floating card is, if one floats.
fn push(view: &Entity<ChatView>, cx: &TestAppContext) -> Option<Pixels> {
    view.read_with(cx, |view, _| view.floating.as_ref().map(|f| f.push))
}

fn click(selector: &'static str, window: AnyWindowHandle, cx: &mut TestAppContext) {
    let mut drawn = VisualTestContext::from_window(window, cx);
    let bounds = drawn.debug_bounds(selector).unwrap();
    drawn.simulate_click(bounds.center(), Modifiers::none());
    settled(window, cx);
}

#[gpui_kit::test]
fn nothing_floats_until_a_person_scrolls_up_and_then_it_comes_down_with_the_scroll(
    cx: &mut TestAppContext,
) {
    let (window, view) = shown("chat-floating-reveal", cx);
    let rows = prompts(&view, cx);
    assert_eq!(rows.len(), 3, "the steer heads no section: {rows:?}");
    let third = rows[2];
    assert_eq!(
        floats(&view, window, cx),
        None,
        "at the end, nothing floats"
    );

    wheel(-40., window, cx);
    assert_eq!(
        floats(&view, window, cx),
        Some(third),
        "40 pt up, the latest comes down"
    );
    let partly = push(&view, cx).unwrap();
    assert!(partly < px(-40.), "still mostly out of sight: {partly:?}");

    wheel(-30., window, cx);
    assert_eq!(
        push(&view, cx),
        Some(partly + px(30.)),
        "as far down as the scroll went up"
    );

    wheel(-300., window, cx);
    assert_eq!(push(&view, cx), Some(px(0.)), "all of it, in its place");

    wheel(50., window, cx);
    assert_eq!(
        push(&view, cx),
        Some(px(-50.)),
        "scrolling down takes it up as far"
    );

    wheel(400., window, cx);
    assert_eq!(
        floats(&view, window, cx),
        None,
        "and out of sight, its place drawn again"
    );
}

#[gpui_kit::test]
fn a_jump_shows_no_prompt_and_the_one_of_the_section_read_comes_down_on_a_scroll_up(
    cx: &mut TestAppContext,
) {
    let (window, view) = shown("chat-floating-sections", cx);
    let [first, second] = [prompts(&view, cx)[0], prompts(&view, cx)[1]];

    scroll_to(&view, second + 1, window, cx);
    assert_eq!(floats(&view, window, cx), None, "gone to, nothing floats");
    // Down into its answer, then back up a little: its prompt stays far above.
    wheel(600., window, cx);
    assert_eq!(
        floats(&view, window, cx),
        None,
        "scrolling down, nothing either"
    );
    wheel(-300., window, cx);
    assert_eq!(
        floats(&view, window, cx),
        Some(second),
        "its section's, scrolled up to"
    );

    scroll_to(&view, second, window, cx);
    assert_eq!(
        floats(&view, window, cx),
        None,
        "its place is at the top: it's there"
    );
    wheel(20., window, cx);
    assert_eq!(
        floats(&view, window, cx),
        None,
        "scrolling down, it goes with its place"
    );

    scrolled_back_into(&view, second - 1, window, cx);
    assert_eq!(
        floats(&view, window, cx),
        Some(first),
        "the section before has its own"
    );

    scroll_to(&view, first, window, cx);
    assert_eq!(
        floats(&view, window, cx),
        None,
        "above the first prompt, nothing"
    );
}

#[gpui_kit::test]
fn a_prompt_still_in_view_comes_down_itself_and_no_card_shows_it(cx: &mut TestAppContext) {
    let (window, view) = shown("chat-floating-in-view", cx);
    let second = prompts(&view, cx)[1];
    scroll_to(&view, second, window, cx);
    wheel(60., window, cx);
    assert_eq!(
        top(&view, cx).item_ix,
        second,
        "its bubble's still in view, 60 pt up"
    );
    assert_eq!(floats(&view, window, cx), None);

    wheel(-30., window, cx);
    assert_eq!(
        floats(&view, window, cx),
        None,
        "scrolling up, the bubble comes down"
    );
}

#[gpui_kit::test]
fn the_next_prompt_coming_up_pushes_the_floating_one_out_rather_than_under_it(
    cx: &mut TestAppContext,
) {
    let (window, view) = shown("chat-floating-push", cx);
    let [second, third] = [prompts(&view, cx)[1], prompts(&view, cx)[2]];
    scroll_to(&view, third, window, cx);
    assert_eq!(
        push(&view, cx),
        None,
        "the third is at the top, in its place"
    );

    // A turn up: the third's row 30 pt under the top, the second coming down over it.
    wheel(-30., window, cx);
    assert_eq!(floats(&view, window, cx), Some(second));
    // The third's bubble starts a prompt's room into its row; the card ends that much above it.
    let mut drawn = VisualTestContext::from_window(window, cx);
    let card = drawn.debug_bounds("chat-floating-prompt").unwrap();
    let row = drawn
        .debug_bounds(format!("chat-row-{third}").leak())
        .unwrap();
    assert!(
        card.bottom() <= row.top() + px(0.5),
        "{card:?} over {row:?}"
    );
}

#[gpui_kit::test]
fn a_floating_prompt_opens_in_place_and_goes_to_where_it_is(cx: &mut TestAppContext) {
    let (window, view) = shown("chat-floating-open", cx);
    let second = prompts(&view, cx)[1];
    scrolled_back_into(&view, second + 1, window, cx);
    let card = |cx: &mut TestAppContext| {
        (VisualTestContext::from_window(window, cx).debug_bounds("chat-floating-prompt")).unwrap()
    };
    let (closed, at) = (card(cx), top(&view, cx));

    click("chat-floating-more", window, cx);
    let open = |cx: &TestAppContext| {
        view.read_with(cx, |view, _| view.floating.as_ref().is_some_and(|f| f.open))
    };
    assert!(open(cx));
    assert!(card(cx).size.height > closed.size.height, "all of it shows");
    assert_eq!(top(&view, cx), at, "where the chat is stays");

    click("chat-floating-more", window, cx);
    assert!(!open(cx));
    assert_eq!(card(cx).size.height, closed.size.height);

    click("chat-floating-go", window, cx);
    assert_eq!(top(&view, cx).item_ix, second, "the chat goes to it");
    assert_eq!(floats(&view, window, cx), None, "and it's in its place");
}

#[gpui_kit::test]
fn with_the_setting_off_no_prompt_floats_in_a_chat_already_open(cx: &mut TestAppContext) {
    let (window, view) = shown("chat-floating-off", cx);
    let third = prompts(&view, cx)[2];
    wheel(-300., window, cx);
    assert_eq!(floats(&view, window, cx), Some(third));

    cx.update(|cx| settings::update(cx, |settings| settings.floating_prompts = false));
    settled(window, cx);
    assert_eq!(floats(&view, window, cx), None);
    wheel(-100., window, cx);
    assert_eq!(floats(&view, window, cx), None);

    cx.update(|cx| settings::update(cx, |settings| settings.floating_prompts = true));
    settled(window, cx);
    assert_eq!(floats(&view, window, cx), Some(third));
}

/// The rows of a steer and of a prompt: only the prompt's has room over its bubble.
#[gpui_kit::test]
fn only_a_prompts_row_has_room_over_its_bubble(cx: &mut TestAppContext) {
    let (_, view) = shown("chat-floating-room", cx);
    let (rows, prompts) = view.read_with(cx, |view, cx| {
        let timeline = view.showing().unwrap().timeline.read(cx);
        (timeline.rows.clone(), timeline.prompts.clone())
    });
    let steer = (rows.iter())
        .position(|row| row.id == koyo_agent::timeline::RowId::Item(ItemId("s".into())))
        .unwrap();
    assert!(!prompts.contains(&steer));
    let room = |row| super::super::rows::room_over(row, &prompts).to_bits();
    assert_eq!(room(steer), 0_f32.to_bits());
    assert_eq!(room(prompts[1]), 8_f32.to_bits());
}
