//! Where the tab opens that an agent's tool opens by itself (`control/exec/beside.rs`), here
//! the agent desktop's: with the agent that asked, on its checkout and after its own tab,
//! whatever checkout and tab the person is on. The desktop is off in these tests, so
//! `desktop_open` is refused once its tab is open, which is all they look at.

use std::path::{Path, PathBuf};

use gpui_kit::TestAppContext;
use koyo_agent::protocol::ids::ThreadId;
use koyo_app_control::{ErrorCode, Scope};
use serde_json::json;

use super::shown::{show_what_agents_open, shown};
use super::{Caller, Rig, allow, call, chat, controlled, external, read_shell, terminal};
use crate::model::PaneKind;
use crate::test_support::{repo, select, terminal_listed};

/// The person's project and the agents', each with its main checkout; the window shows the
/// person's, at the chat they're reading.
fn rig(name: &str, cx: &mut TestAppContext) -> (Rig, PathBuf, PathBuf) {
    let mine = repo(&format!("{name}-mine"), &[("README.md", "hi\n")]);
    let theirs = repo(&format!("{name}-theirs"), &[("README.md", "hi\n")]);
    let rig = controlled(name, &[&mine, &theirs], cx);
    chat_tab(&rig, &mine, "reading", cx);
    select(&rig.shell, &mine, cx);
    (rig, mine, theirs)
}

/// A tab of chat `id` on the checkout at `root`, after its others and not shown.
fn chat_tab(rig: &Rig, root: &Path, id: &str, cx: &mut TestAppContext) {
    rig.shell.update(cx, |shell, cx| {
        let pane = (shell.workspace).add_chat_tab(root, ThreadId(id.into()), false);
        assert!(pane.is_some(), "a tab for {id}");
        cx.notify();
    });
}

/// The tabs of the checkout at `root`, each by its chat's id, else its title; the one the
/// checkout shows in brackets.
fn tabs(rig: &Rig, root: &Path, cx: &TestAppContext) -> Vec<String> {
    read_shell(rig, cx, |shell, _| {
        let (project, branch) = shell.workspace.unit_at(root).expect("the checkout");
        let unit = &shell.workspace.projects[project].branches[branch];
        (unit.tabs.iter().enumerate())
            .map(|(at, tab)| {
                let pane = tab.first();
                let name = match (&pane.chat, pane.kind) {
                    (Some(chat), _) => chat.0.clone(),
                    (None, PaneKind::Desktop) => "desktop".to_owned(),
                    (None, _) => pane.title.to_string(),
                };
                if at == unit.active_tab {
                    format!("[{name}]")
                } else {
                    name
                }
            })
            .collect()
    })
}

/// The person is where they were: on their checkout, at their chat, with nothing new there.
fn person_untouched(rig: &Rig, mine: &Path, cx: &TestAppContext) {
    assert_eq!(tabs(rig, mine, cx), ["[reading]"], "nothing where they are");
    assert_eq!(shown(rig, cx).0, mine, "and the window stays there");
}

/// `caller` asks for an app on the agent desktop: refused, the desktop being off, with its
/// tab opened all the same.
fn desktop_open(caller: &Caller, cx: &mut TestAppContext) {
    let refused = call(caller, "desktop_open", json!({ "app": "TextEdit" }), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::DesktopUnavailable, "{refused:?}");
}

/// Closes the desktop's tab, as the person does.
fn close_desktop(rig: &Rig, cx: &mut TestAppContext) {
    rig.shell.update(cx, |shell, cx| {
        let open = (shell.workspace).find_pane(|pane| pane.kind == PaneKind::Desktop);
        let (_, pane) = open.expect("the desktop's tab");
        shell.close_pane_now(pane, cx);
    });
    cx.run_until_parked();
}

#[gpui_kit::test]
fn the_desktops_tab_opens_after_the_tab_of_the_chat_that_asked_not_where_the_person_is(
    cx: &mut TestAppContext,
) {
    let (rig, mine, theirs) = rig("control-beside-chat", cx);
    for id in ["first", "asker", "last"] {
        chat_tab(&rig, &theirs, id, cx);
    }
    assert_eq!(tabs(&rig, &theirs, cx), ["[first]", "asker", "last"]);

    desktop_open(&chat("asker"), cx);
    person_untouched(&rig, &mine, cx);
    let beside_asker = ["[first]", "asker", "desktop", "last"];
    assert_eq!(
        tabs(&rig, &theirs, cx),
        beside_asker,
        "after the asker's own tab, the tab its checkout shows left shown"
    );

    // There is one desktop: another chat that asks finds its tab where it is.
    desktop_open(&chat("reading"), cx);
    person_untouched(&rig, &mine, cx);
    assert_eq!(tabs(&rig, &theirs, cx), beside_asker);

    // The person on the asker's checkout, at another chat: theirs stays the one shown.
    close_desktop(&rig, cx);
    select(&rig.shell, &theirs, cx);
    desktop_open(&chat("asker"), cx);
    assert_eq!(tabs(&rig, &theirs, cx), beside_asker);
    assert_eq!(shown(&rig, cx).0, theirs);
}

#[gpui_kit::test]
fn an_agent_in_a_terminal_gets_the_desktops_tab_after_its_terminals(cx: &mut TestAppContext) {
    let (rig, mine, theirs) = rig("control-beside-terminal", cx);
    chat_tab(&rig, &theirs, "first", cx);
    // The daemon runs a terminal on the agents' checkout: it gets a tab there.
    rig.shell.update(cx, |shell, cx| {
        let listed = vec![terminal_listed("term_9", "agent", &theirs)];
        shell.take_terminals(std::slice::from_ref(&theirs), listed, cx);
    });
    cx.run_until_parked();
    chat_tab(&rig, &theirs, "last", cx);
    select(&rig.shell, &mine, cx);
    let before = tabs(&rig, &theirs, cx);
    assert_eq!(before.len(), 3, "{before:?}");

    desktop_open(&terminal("term_9"), cx);
    person_untouched(&rig, &mine, cx);
    let mut after = before;
    after.insert(2, "desktop".to_owned());
    assert_eq!(tabs(&rig, &theirs, cx), after, "after the terminal's tab");
}

#[gpui_kit::test]
fn a_caller_with_no_tab_gets_it_on_its_worktree_and_one_outside_every_checkout_on_the_one_selected(
    cx: &mut TestAppContext,
) {
    let (rig, mine, theirs) = rig("control-beside-worktree", cx);
    for id in ["first", "last"] {
        chat_tab(&rig, &theirs, id, cx);
    }
    // A chat no tab shows runs in its checkout all the same.
    let untabbed = Caller {
        cwd: Some(theirs.join("src")),
        ..chat("untabbed")
    };
    desktop_open(&untabbed, cx);
    person_untouched(&rig, &mine, cx);
    assert_eq!(
        tabs(&rig, &theirs, cx),
        ["[first]", "desktop", "last"],
        "after the tab its checkout shows"
    );

    // A client outside every checkout has no place of its own: the selected checkout, out
    // of sight, as `tab_open` gives it.
    close_desktop(&rig, cx);
    allow("outside", &Scope::ALL, cx);
    desktop_open(&external("outside"), cx);
    assert_eq!(tabs(&rig, &mine, cx), ["[reading]", "desktop"]);
    assert_eq!(tabs(&rig, &theirs, cx), ["[first]", "last"]);

    // A checkout with no tab at all has nothing to keep shown: the desktop's is its tab,
    // and the person, elsewhere, stays where they are.
    close_desktop(&rig, cx);
    rig.shell.update(cx, |shell, cx| {
        let (project, branch) = shell.workspace.unit_at(&theirs).expect("the checkout");
        while shell.workspace.close_tab(project, branch, 0).is_some() {}
        cx.notify();
    });
    desktop_open(&untabbed, cx);
    assert_eq!(tabs(&rig, &theirs, cx), ["[desktop]"]);
    person_untouched(&rig, &mine, cx);
}

#[gpui_kit::test]
fn switch_to_what_agents_open_takes_the_window_to_the_desktops_tab_wherever_it_is(
    cx: &mut TestAppContext,
) {
    let (rig, mine, theirs) = rig("control-beside-shown", cx);
    for id in ["asker", "last"] {
        chat_tab(&rig, &theirs, id, cx);
    }
    show_what_agents_open(true, cx);
    desktop_open(&chat("asker"), cx);
    let shown_beside_asker = ["asker", "[desktop]", "last"];
    assert_eq!(shown(&rig, cx).0, theirs, "the person asked to see it");
    assert_eq!(tabs(&rig, &theirs, cx), shown_beside_asker);
    assert_eq!(tabs(&rig, &mine, cx), ["[reading]"]);

    // Asked for again from the person's own checkout, the one tab there is shows.
    rig.shell.update(cx, |shell, cx| {
        shell.workspace.select_tab(1, 0, 0);
        shell.workspace.select_tab(0, 0, 0);
        cx.notify();
    });
    assert_eq!(tabs(&rig, &theirs, cx), ["[asker]", "desktop", "last"]);
    assert_eq!(shown(&rig, cx).0, mine);
    desktop_open(&chat("reading"), cx);
    assert_eq!(shown(&rig, cx).0, theirs);
    assert_eq!(tabs(&rig, &theirs, cx), shown_beside_asker);
}
