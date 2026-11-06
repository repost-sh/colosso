//! The chats no tab shows: listed in the new-tab launcher, and opening where they're shown
//! already or in a new tab.

use gpui_kit::{SharedString, TestAppContext};
use koyo_agent::protocol::ids::{ThreadId, Time};
use koyo_agent::protocol::model::ThreadSummary;

use super::{chats_of, summary, tabs};
use crate::test_support::{draw, repo, select, shell_with};

fn at(id: &str, root: &std::path::Path, title: &str, updated_at: &str) -> ThreadSummary {
    ThreadSummary {
        updated_at: Time(updated_at.into()),
        ..summary(id, root, title, false)
    }
}

#[gpui_kit::test]
fn the_chats_of_a_checkout_with_no_tab_are_recent_latest_first(cx: &mut TestAppContext) {
    let root = repo("chat-recent", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let chats = chats_of(&shell, cx);
    let elsewhere = at(
        "0009",
        std::path::Path::new("/elsewhere"),
        "Elsewhere",
        "2026-11-04T03:00:00Z",
    );
    let listed = vec![
        at("0001", &root, "Older", "2026-11-04T01:00:00Z"),
        at("0002", &root, "Newer", "2026-11-04T02:00:00Z"),
        summary("0003", &root, "Has a tab", true),
        elsewhere,
    ];
    chats.update(cx, |chats, cx| chats.listed(Ok(listed), cx));
    draw(window, cx);

    let titles: Vec<SharedString> = shell.read_with(cx, |shell, cx| {
        shell
            .chats_without_tabs(&root, cx)
            .into_iter()
            .map(|(_, title)| title)
            .collect()
    });

    assert_eq!(titles, ["Newer", "Older"]);
}

#[gpui_kit::test]
fn a_recent_chat_opens_in_a_new_tab_and_a_shown_one_where_it_is(cx: &mut TestAppContext) {
    let root = repo("chat-open", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let chats = chats_of(&shell, cx);
    chats.update(cx, |chats, cx| {
        chats.listed(Ok(vec![summary("0001", &root, "No tab yet", false)]), cx);
    });
    draw(window, cx);
    assert!(tabs(&shell, cx).is_empty());
    let id = ThreadId("0001".into());

    let pane = shell.update(cx, |shell, cx| shell.open_chat(&id, cx));
    draw(window, cx);

    assert!(pane.is_some());
    assert_eq!(tabs(&shell, cx), ["No tab yet"]);
    let again = shell.update(cx, |shell, cx| shell.open_chat(&id, cx));
    draw(window, cx);
    assert_eq!(again, pane);
    assert_eq!(tabs(&shell, cx), ["No tab yet"]);
    let unknown = shell.update(cx, |shell, cx| {
        shell.open_chat(&ThreadId("nope".into()), cx)
    });
    assert_eq!(unknown, None);
}
