//! What the pointer does on a chat's rows: a drag over a step copies its words or what it printed,
//! a click opens it, a click on a link to a file opens the file, and one on a link to a page opens
//! the page in a browser tab.

use gpui_kit::base::TextSelection;
use gpui_kit::{
    AppContext as _, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px,
};
use koyo_agent::protocol::model::{ItemBody, Tool, ToolFamily};

use super::drawing::{settled, settled_with, shell_with_chat, shows_of};
use super::tabs;
use crate::model::PaneKind;
use crate::settings::Shows;
use crate::test_support::draw;

/// A step's words and what it printed can be dragged over and copied, as in a terminal; a click on
/// its words still opens it, and the drag doesn't.
#[gpui_kit::test]
fn a_steps_words_and_output_can_be_selected_and_a_click_still_opens_it(cx: &mut TestAppContext) {
    let ran = ItemBody::Tool(Tool {
        name: "Bash".into(),
        family: ToolFamily::Command,
        label: "cargo test".into(),
        input: serde_json::Value::Null,
        output: Some("one\ntwo".into()),
        exit_code: Some(0),
        patch: None,
        files: Vec::new(),
        server: None,
    });
    let (window, _, view, _) =
        shell_with_chat("chat-select", settled_with("0001", vec![("c", ran)]), cx);
    let timeline = view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone());
    let shows = |cx: &mut TestAppContext| shows_of(&timeline, "c", cx);
    let selected = |cx: &mut TestAppContext| {
        (cx.update_window(window, |_, window, cx| {
            TextSelection::selected_text(window, cx)
        }))
        .unwrap()
    };
    let row = VisualTestContext::from_window(window, cx)
        .debug_bounds("chat-row-2")
        .unwrap();
    let (left, middle) = (row.left() + px(48.), row.top() + px(10.));
    let mut visual = VisualTestContext::from_window(window, cx);
    let none = Modifiers::default();
    visual.simulate_mouse_move(point(left, middle), None, none);
    visual.simulate_mouse_down(point(left, middle), MouseButton::Left, none);
    visual.simulate_mouse_move(
        point(left + px(200.), middle),
        Some(MouseButton::Left),
        none,
    );
    visual.simulate_mouse_up(point(left + px(200.), middle), MouseButton::Left, none);
    draw(window, cx);
    assert!(selected(cx).contains("cargo test"), "{:?}", selected(cx));
    assert_eq!(shows(cx), Shows::Preview, "the drag opened nothing");

    let mut visual = VisualTestContext::from_window(window, cx);
    visual.simulate_mouse_move(point(left, middle), None, none);
    visual.simulate_mouse_down(point(left, middle), MouseButton::Left, none);
    visual.simulate_mouse_up(point(left, middle), MouseButton::Left, none);
    draw(window, cx);
    assert_eq!(selected(cx), "");
    assert_eq!(shows(cx), Shows::All, "the click opened all of it");

    let output = row.top() + px(44.);
    let mut visual = VisualTestContext::from_window(window, cx);
    visual.simulate_mouse_down(point(row.left() + px(40.), output), MouseButton::Left, none);
    let past = point(row.left() + px(300.), output + px(18.));
    visual.simulate_mouse_move(past, Some(MouseButton::Left), none);
    visual.simulate_mouse_up(past, MouseButton::Left, none);
    draw(window, cx);
    assert_eq!(selected(cx), "one\ntwo");
}

/// A link in an answer to a file in the chat's checkout reads as the file tree names the file, and
/// a click on it opens the file in a tab; a link to a page stays a link.
#[gpui_kit::test]
fn a_link_to_a_file_shows_as_the_tree_names_it_and_opens_it(cx: &mut TestAppContext) {
    let mut shown = settled("0001");
    let said = "See [README.md](README.md) and [the site](https://example.com).";
    if let Some(ItemBody::AssistantText(text)) = shown.items.last_mut().map(|item| &mut item.body) {
        text.text = said.into();
    }
    let (window, shell, _, _) = shell_with_chat("chat-file-link", shown, cx);
    draw(window, cx);
    let label = VisualTestContext::from_window(window, cx).debug_bounds("file-link-README.md");
    let label = label.expect("the link is drawn as the file's label");
    assert!(
        VisualTestContext::from_window(window, cx)
            .debug_bounds("file-link-the site")
            .is_none(),
        "a page's link stays a link"
    );
    let before = tabs(&shell, cx);
    let mut visual = VisualTestContext::from_window(window, cx);
    let none = Modifiers::default();
    visual.simulate_mouse_move(label.center(), None, none);
    visual.simulate_mouse_down(label.center(), MouseButton::Left, none);
    visual.simulate_mouse_up(label.center(), MouseButton::Left, none);
    draw(window, cx);
    let after = tabs(&shell, cx);
    assert_eq!(after.len(), before.len() + 1, "{before:?} to {after:?}");
    assert!(
        after.iter().any(|tab| tab.contains("README.md")),
        "{after:?}"
    );
}

/// A click on a link to a page in an answer opens the page in a Koyo browser tab right after the
/// chat's, shown, as a link opened beside its page does, and never in the system's browser.
#[gpui_kit::test]
fn a_link_to_a_page_opens_it_in_a_browser_tab_beside_the_chat(cx: &mut TestAppContext) {
    let url = "https://example.com/guide";
    let mut shown = settled("0001");
    let said = format!("[Read the whole guide on the site]({url})");
    if let Some(ItemBody::AssistantText(text)) = shown.items.last_mut().map(|item| &mut item.body) {
        text.text = said;
    }
    let (window, shell, _, _) = shell_with_chat("chat-page-link", shown, cx);
    draw(window, cx);
    let answer = VisualTestContext::from_window(window, cx)
        .debug_bounds("chat-row-2")
        .expect("the answer's row is drawn");
    let on_link = point(answer.left() + px(60.), answer.top() + px(8.));
    let mut visual = VisualTestContext::from_window(window, cx);
    let none = Modifiers::default();
    visual.simulate_mouse_move(on_link, None, none);
    visual.simulate_mouse_down(on_link, MouseButton::Left, none);
    visual.simulate_mouse_up(on_link, MouseButton::Left, none);
    draw(window, cx);
    assert_eq!(cx.opened_url(), None, "the system's browser opened nothing");
    let (kinds, shown) = shell.read_with(cx, |shell, _| {
        let unit = shell.workspace.branch().expect("a checkout is selected");
        let kinds = (unit.tabs.iter()).map(|tab| tab.first().kind()).collect::<Vec<_>>();
        let shown = (shell.workspace.tab_shown())
            .and_then(|tab| tab.first().address().map(str::to_owned));
        (kinds, shown)
    });
    assert_eq!(kinds, [PaneKind::Chat, PaneKind::Browser]);
    assert_eq!(shown.as_deref(), Some(url), "the page's tab is shown");
}
