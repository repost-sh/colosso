//! The Arc sidebar's shell side (`docs/arc-sidebar.md`): spaces shown and followed, a project
//! moved, a space deleted with its projects, a worktree cleared, and what the address bar says.

use std::path::Path;

use gpui_kit::{
    Action, AnyWindowHandle, AppContext as _, Entity, Modifiers, MouseButton, TestAppContext,
    VisualTestContext, point, px,
};

use super::actions::{ClearWorktree, MoveProjectToNewSpace, NewSpace};
use super::clear::Clear;
use super::delete_space::DeleteSpace;
use crate::files::tests::{dialog, tabs};
use crate::launcher::Launcher;
use crate::model::{Edge, Pane, PaneKind, Tab, Unloaded};
use crate::navigation::Route;
use crate::settings::{self, Layout};
use crate::settings_view::SettingsPage;
use crate::shell::{CloseTab, Shell};
use koyo_agent::protocol::model::{Status, ThreadSummary};
use koyo_agent::protocol::requests::RequestKind;

use crate::test_support::{click_pane, draw, open_page, repo, select, shell_with};

mod loose;
mod pinned;
mod space_keys;
mod spaces;
mod switcher;
mod tabs;
mod tiles;
mod worktrees;

/// The shell in the Arc layout with a project for each of `names`, saved, the first selected.
fn arc_shell(
    names: &[&str],
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, Vec<std::path::PathBuf>) {
    let roots: Vec<_> = (names.iter())
        .map(|name| repo(name, &[("README.md", "hi\n")]))
        .collect();
    let refs: Vec<&Path> = roots.iter().map(std::path::PathBuf::as_path).collect();
    let (window, shell) = shell_with(&refs, cx);
    cx.update(|cx| settings::update(cx, |settings| settings.layout = Layout::Arc));
    // Spaces switch at once, rather than sliding over frames these tests don't step through.
    cx.update(|cx| cx.set_reduce_motion(true));
    shell.update(cx, Shell::save_projects);
    select(&shell, &roots[0], cx);
    draw(window, cx);
    (window, shell, roots)
}

fn act(window: AnyWindowHandle, action: impl Action, cx: &mut TestAppContext) {
    (cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(action), cx);
    }))
    .unwrap();
    cx.run_until_parked();
    draw(window, cx);
}

/// The selected project's folder, and the space shown.
fn shown(shell: &Entity<Shell>, cx: &TestAppContext) -> (std::path::PathBuf, usize) {
    cx.read(|cx| {
        let project = shell.read(cx).workspace.project().unwrap().path.clone();
        (project, settings::get(cx).space)
    })
}

use crate::model::tests::chat;

#[gpui_kit::test]
fn a_space_shows_the_project_it_showed_last_and_an_empty_one_hides_the_others(
    cx: &mut TestAppContext,
) {
    let (window, shell, roots) = arc_shell(&["arc-spaces-app", "arc-spaces-site"], cx);
    let (app, site) = (roots[0].clone(), roots[1].clone());
    assert_eq!(shown(&shell, cx), (app.clone(), 0));

    act(window, MoveProjectToNewSpace(site.clone()), cx);
    assert_eq!(
        shown(&shell, cx),
        (site.clone(), 1),
        "the project's shown in its new space"
    );
    cx.read(|cx| {
        let spaces = &settings::get(cx).spaces;
        assert_eq!(spaces[0].projects, vec![app.clone()]);
        assert_eq!(spaces[1].projects, vec![site.clone()]);
    });

    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    cx.run_until_parked();
    assert_eq!(shown(&shell, cx), (app, 0));

    // A tab on the project, which the empty space hides along with it.
    shell.update(cx, |shell, _| {
        shell
            .workspace
            .branch_mut()
            .unwrap()
            .tabs
            .push(chat("hidden"));
    });
    act(window, NewSpace, cx);
    let hidden = cx.read(|cx| shell.read(cx).workspace.project().is_none());
    assert!(
        hidden,
        "an empty space shows no project, so none of another space's tabs"
    );
    assert_eq!(cx.read(|cx| settings::get(cx).space), 2);
    let tabs = |cx: &mut TestAppContext| {
        cx.read(|cx| shell.read(cx).workspace.projects[0].branches[0].tabs.len())
    };
    let before = tabs(cx);
    act(window, CloseTab, cx);
    assert_eq!(
        tabs(cx),
        before,
        "⌘W has no tab to close while no project is shown"
    );

    // Selected some other way, as app control or the Orca layout select, the project's
    // space is shown.
    shell.update(cx, |shell, cx| {
        let site = shell.workspace.project_index(&site).unwrap();
        shell.workspace.select_project(Some(site));
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(shown(&shell, cx), (site, 1));
}

#[gpui_kit::test]
fn deleting_a_space_can_take_its_projects_out_of_koyo(cx: &mut TestAppContext) {
    let (window, shell, roots) = arc_shell(&["arc-delete-app", "arc-delete-site"], cx);
    act(window, MoveProjectToNewSpace(roots[1].clone()), cx);
    act(window, super::actions::DeleteSpace, cx);
    let (dialog_window, asked) = dialog::<DeleteSpace>(cx).expect("it asks first");
    (cx.update_window(dialog_window, |_, window, cx| {
        asked.update(cx, |asked, cx| {
            asked.remove = true;
            asked.delete(window, cx);
        });
    }))
    .unwrap();
    cx.run_until_parked();
    cx.read(|cx| {
        let settings = settings::get(cx);
        assert_eq!(settings.spaces.len(), 1);
        assert_eq!(settings.projects, vec![roots[0].clone()]);
        let workspace = &shell.read(cx).workspace;
        assert_eq!(workspace.projects.len(), 1);
        assert_eq!(workspace.projects[0].path, roots[0]);
    });
    assert_eq!(shown(&shell, cx), (roots[0].clone(), 0));
}

#[gpui_kit::test]
fn clear_asks_first_then_closes_every_tab_of_the_worktree(cx: &mut TestAppContext) {
    let (window, shell, roots) = arc_shell(&["arc-clear"], cx);
    shell.update(cx, |shell, cx| {
        for title in ["one", "two", "three"] {
            shell.workspace.push_tab(chat(title));
        }
        cx.notify();
    });
    draw(window, cx);
    act(window, ClearWorktree(roots[0].clone()), cx);
    let (dialog_window, asked) = dialog::<Clear>(cx).expect("Clear asks first");
    assert_eq!(
        tabs(&shell, cx).len(),
        3,
        "nothing closes until it's agreed"
    );
    (cx.update_window(dialog_window, |_, window, cx| {
        asked.update(cx, |asked, cx| asked.clear(window, cx));
    }))
    .unwrap();
    cx.run_until_parked();
    assert!(tabs(&shell, cx).is_empty());
    // They can come back, as any tab closed can.
    let closed = shell.update(cx, |shell, _| shell.workspace.take_closed().is_some());
    assert!(closed);
}

#[gpui_kit::test]
fn the_address_bar_says_what_is_shown(cx: &mut TestAppContext) {
    let (window, shell, _) = arc_shell(&["arc-address"], cx);
    let said = |shell: &Entity<Shell>, cx: &TestAppContext| {
        cx.read(|cx| super::top::said(shell.read(cx), cx).0.to_string())
    };
    assert_eq!(said(&shell, cx), "No tab open");
    let page = Pane::unloaded(
        Unloaded {
            address: "https://www.example.com/docs?page=2".into(),
            favicon: None,
        },
        "Example".into(),
    );
    let shown = [
        (Tab::Single(page), "example.com"),
        (chat("Plan the release"), "Plan the release"),
        (Tab::Single(Pane::new(PaneKind::Terminal)), "zsh"),
        (Tab::Single(Pane::new(PaneKind::Editor)), "File"),
    ];
    for (tab, expected) in shown {
        shell.update(cx, |shell, cx| {
            shell.workspace.push_tab(tab);
            cx.notify();
        });
        assert_eq!(said(&shell, cx), expected);
    }
    shell.update(cx, |shell, cx| {
        shell.navigate(Route::Settings(SettingsPage::General), cx);
    });
    draw(window, cx);
    assert_eq!(said(&shell, cx), "Settings");
}

/// Clicks the address bar: the launcher it opened, if it opened one, dismissed again.
fn click_address(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    let at = vcx.debug_bounds("arc-address").expect("the bar").center();
    vcx.simulate_click(at, Modifiers::default());
    cx.run_until_parked();
    let Some((launcher, _)) = dialog::<Launcher>(cx) else {
        return false;
    };
    crate::files::tests::press(launcher, "escape", cx);
    cx.run_until_parked();
    true
}

#[gpui_kit::test]
fn a_click_on_the_address_bar_opens_the_launcher_only_on_a_page(cx: &mut TestAppContext) {
    let (window, shell, _) = arc_shell(&["arc-address-click"], cx);
    assert!(!click_address(window, cx), "no tab, no address");
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("Plan the release"));
        cx.notify();
    });
    assert!(!click_address(window, cx), "a chat has no address");
    assert_eq!(tabs(&shell, cx).len(), 1);
    open_page(window, &shell, Some("https://example.com/"), cx);
    assert!(click_address(window, cx), "a page's address, to change");
}

#[gpui_kit::test]
fn the_address_bar_and_the_page_controls_follow_the_pane_the_person_is_in(cx: &mut TestAppContext) {
    let (window, shell, _) = arc_shell(&["arc-address-split"], cx);
    let said =
        |cx: &TestAppContext| cx.read(|cx| super::top::said(shell.read(cx), cx).0.to_string());
    // A chat on the left, two pages right of it.
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("Plan the release"));
        cx.notify();
    });
    let chat = cx.read(|cx| {
        shell
            .read(cx)
            .workspace
            .tab_shown()
            .map(|tab| tab.first().id)
    });
    let chat = chat.expect("the chat's pane");
    let docs = open_page(window, &shell, Some("https://www.example.com/docs"), cx);
    let app = open_page(window, &shell, Some("http://localhost:3000/"), cx);
    shell.update(cx, |shell, cx| {
        let unit = shell.workspace.branch_mut().unwrap();
        assert!(unit.drop_tab(1, chat, Edge::Right));
        assert!(unit.drop_tab(1, docs, Edge::Right));
        cx.notify();
    });
    draw(window, cx);
    let page = |cx: &TestAppContext| {
        cx.read(|cx| (shell.read(cx).shown_page(cx)).map(|page| page.read(cx).address()))
    };
    let click = |pane: u64, cx: &mut TestAppContext| click_pane(window, pane, cx);
    click(app, cx);
    assert_eq!(said(cx), "localhost:3000");
    assert_eq!(page(cx).as_deref(), Some("http://localhost:3000/"));
    click(docs, cx);
    assert_eq!(said(cx), "example.com");
    assert_eq!(page(cx).as_deref(), Some("https://www.example.com/docs"));
    // In the chat, the bar says the chat; back, forward and reload stay the tab's first page's.
    click(chat, cx);
    assert_eq!(said(cx), "Plan the release");
    assert_eq!(page(cx).as_deref(), Some("https://www.example.com/docs"));
    // It's remembered while something outside the panes has the keyboard.
    click(app, cx);
    cx.update_window(window, |_, window, cx| {
        let root = shell.read(cx).focus.clone();
        window.focus(&root, cx);
    })
    .unwrap();
    assert_eq!(said(cx), "localhost:3000");
    let focused = cx.update_window(window, |_, window, cx| {
        shell.read(cx).focused_pane(window, cx)
    });
    assert_eq!(focused.unwrap(), Some(app), "as app control reports it");
}

/// The chats' host says of chat `id`, of the checkout at `cwd`, that it's at `status`, waiting
/// on the person for `needs` or not, with something they haven't read or not.
fn hear(
    shell: &Entity<Shell>,
    (id, cwd): (&str, &Path),
    (status, needs, unread): (Status, Option<RequestKind>, bool),
    cx: &mut TestAppContext,
) {
    let summary = ThreadSummary {
        status,
        needs,
        unread,
        ..crate::chat::tests::summary(id, cwd, "A chat", true)
    };
    tell(shell, summary, cx);
}

/// The chats' host sends `summary`.
fn tell(shell: &Entity<Shell>, summary: ThreadSummary, cx: &mut TestAppContext) {
    let chats = shell.read_with(cx, |shell, _| shell.chats().clone());
    chats.update(cx, |chats, cx| chats.hear_summaries(vec![summary], cx));
    cx.run_until_parked();
}

/// A tab of chat `0001`, called "A chat", and another tab after it, shown, so nothing marks the
/// chat seen: its pane.
fn unseen_chat(shell: &Entity<Shell>, cx: &mut TestAppContext) -> u64 {
    shell.update(cx, |shell, cx| {
        let mut pane = Pane::new(PaneKind::Chat);
        pane.title = "A chat".into();
        pane.chat = Some(koyo_agent::protocol::ids::ThreadId("0001".into()));
        let id = pane.id;
        shell.workspace.push_tab(Tab::Single(pane));
        shell.workspace.push_tab(chat("another"));
        cx.notify();
        id
    })
}

#[gpui_kit::test]
fn a_tab_and_its_projects_tile_say_when_a_chat_wants_the_person(cx: &mut TestAppContext) {
    use crate::chat::Activity;

    let (window, shell, roots) = arc_shell(&["arc-attention"], cx);
    unseen_chat(&shell, cx);
    draw(window, cx);
    let worktree = roots[0].clone();
    let hear = |status, needs, unread, cx: &mut TestAppContext| {
        hear(&shell, ("0001", &worktree), (status, needs, unread), cx);
    };
    let told = |cx: &TestAppContext| {
        cx.read(|cx| {
            let shell = shell.read(cx);
            let tab = &shell.workspace.branch().unwrap().tabs[0];
            let activity = shell.checkout_activity(&worktree, cx);
            (shell.tab_wants_attention(tab, cx), activity)
        })
    };
    hear(Status::Working, None, false, cx);
    assert_eq!(
        told(cx),
        (false, Activity::Working),
        "working: a spinner, and nothing for the person"
    );
    hear(Status::Idle, None, true, cx);
    assert_eq!(
        told(cx),
        (true, Activity::Attention),
        "done and unread: a dot"
    );
    hear(Status::Working, Some(RequestKind::Approval), false, cx);
    assert_eq!(told(cx), (true, Activity::Attention), "asking: a dot");
    hear(Status::Idle, None, false, cx);
    assert_eq!(told(cx), (false, Activity::Quiet), "seen: nothing");
}

/// The selected checkout's tabs, by their first panes' titles.
fn listed(shell: &Entity<Shell>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        let unit = shell.read(cx).workspace.branch().expect("a checkout");
        let titles = unit.tabs.iter().map(|tab| tab.first().title.to_string());
        titles.collect()
    })
}

/// Where `selector` names something drawn in `window`, if it names anything.
fn drawn(
    window: AnyWindowHandle,
    selector: impl Into<String>,
    cx: &mut TestAppContext,
) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
    let selector: &'static str = Box::leak(selector.into().into_boxed_str());
    VisualTestContext::from_window(window, cx).debug_bounds(selector)
}

/// Takes up what `from` names by its middle and lets it go over the middle of what `to` names,
/// drawing at each step as a person's drag would be drawn.
fn drag(window: AnyWindowHandle, from: &'static str, to: &'static str, cx: &mut TestAppContext) {
    let none = Modifiers::default();
    let mut vcx = VisualTestContext::from_window(window, cx);
    let held = vcx.debug_bounds(from).expect("what's dragged").center();
    let target = vcx.debug_bounds(to).expect("where it's dropped").center();
    vcx.simulate_mouse_down(held, MouseButton::Left, none);
    vcx.simulate_mouse_move(held + point(px(3.), px(3.)), Some(MouseButton::Left), none);
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.simulate_mouse_move(target, Some(MouseButton::Left), none);
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.simulate_mouse_up(target, MouseButton::Left, none);
    draw(window, cx);
}

/// Takes up what `from` names by its middle and carries it well off whatever list or grid it's
/// on, over the panes, as a person dragging it elsewhere does: what takes it shows from then.
fn take_up(window: AnyWindowHandle, from: &'static str, cx: &mut TestAppContext) {
    let (none, held) = (Modifiers::default(), Some(MouseButton::Left));
    let mut vcx = VisualTestContext::from_window(window, cx);
    let at = vcx.debug_bounds(from).expect("what's dragged").center();
    vcx.simulate_mouse_down(at, MouseButton::Left, none);
    vcx.simulate_mouse_move(at + point(px(3.), px(3.)), held, none);
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.simulate_mouse_move(point(px(900.), px(450.)), held, none);
    draw(window, cx);
}

/// Carries what's in hand over the middle of what `to` names, as it's drawn now, and lets it go.
fn let_go_on(window: AnyWindowHandle, to: &'static str, cx: &mut TestAppContext) {
    let none = Modifiers::default();
    let mut vcx = VisualTestContext::from_window(window, cx);
    let target = vcx.debug_bounds(to).expect("where it's dropped").center();
    vcx.simulate_mouse_move(target, Some(MouseButton::Left), none);
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    vcx.simulate_mouse_up(target, MouseButton::Left, none);
    draw(window, cx);
}

/// Drags what `from` names off its list or grid and onto what `to` names, where that is once
/// what's in hand has left: a strip or a zone that takes it shows only then.
fn drag_onto(
    window: AnyWindowHandle,
    from: &'static str,
    to: &'static str,
    cx: &mut TestAppContext,
) {
    take_up(window, from, cx);
    let_go_on(window, to, cx);
}

#[gpui_kit::test]
fn a_space_slides_out_the_next_slides_in_and_both_come_to_rest(cx: &mut TestAppContext) {
    let (window, shell, roots) = arc_shell(&["arc-slide-app", "arc-slide-site"], cx);
    act(window, MoveProjectToNewSpace(roots[1].clone()), cx);
    assert_eq!(shown(&shell, cx).1, 1);
    cx.update(|cx| cx.set_reduce_motion(false));
    shell.update(cx, |shell, cx| shell.slide_to_space(0, cx));
    let offset = |cx: &TestAppContext| shell.read_with(cx, |shell, _| shell.swipe.offset);
    let frame = std::time::Duration::from_millis(8);
    let mut seen = Vec::new();
    for _ in 0..240 {
        cx.executor().advance_clock(frame);
        // The display asks for the next frame, as the settling space wants one.
        (cx.update_window(window, |_, window, cx| window.simulate_next_frame(cx))).unwrap();
        draw(window, cx);
        seen.push((offset(cx), shown(&shell, cx).1));
    }
    // Going back a space, the space shown moves right, out; the one before comes in from the
    // left, still moving right, and settles.
    assert!(
        seen.iter()
            .any(|&(offset, space)| space == 1 && offset > 1.)
    );
    assert!(
        seen.iter()
            .any(|&(offset, space)| space == 0 && offset < -1.)
    );
    assert_eq!(seen.last().copied(), Some((0., 0)));
    assert_eq!(shown(&shell, cx), (roots[0].clone(), 0));
}
