//! The sidebar's shell side with the tabs in it (`docs/layout.md`, Sidebar): spaces shown and
//! followed, a project moved, a space deleted with its projects, and what the address bar says.

use std::path::Path;

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Background, Edges, Entity, Focusable as _, Hsla, Modifiers,
    Pixels, Point, TestAppContext, VisualTestContext,
};

use crate::browser::profiles::By;
use crate::chat::{Activity, Attention};
use crate::files::tests::{dialog, tabs};
use crate::launcher::Launcher;
use crate::model::{Browser, Content, Edge, Pane, PaneKind, Tab, Unloaded};
use crate::settings::{self, Tabs};
use crate::navigation::SettingsPage;
use crate::projects::MoveProjectToNewSpace;
use crate::shell::spaces::NewSpace;
use crate::shell::spaces::delete::DeleteSpace;
use crate::shell::{CloseTab, Shell};
use koyo_agent::protocol::model::{Status, ThreadSummary};
use koyo_agent::protocol::requests::RequestKind;
use koyo_agent::work::Work;

use crate::test_support::{
    act, center_of, click_pane, draw, drawn, hold, let_go, let_go_on, move_held, open_page, repo,
    select, shell_with, take_up,
};

mod address;
mod branches;
mod mark_drag;
mod one_theme;
mod pinned;
mod project_fold;
mod project_keys;
mod rows;
mod slide_budget;
mod space_keys;
mod space_menu;
mod space_switch;
mod spaces;
mod switcher;
mod tab_keys;
mod tab_places;
mod tabs;
mod unpinned;
mod unpinned_slide;

/// What's filled or edged at `at` in the frame `window` drew last, from the back.
pub(super) fn painted(
    at: Point<Pixels>,
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
) -> Vec<(Background, Hsla)> {
    let quads = |window: &gpui_kit::Window| {
        let at = at.scale(window.scale_factor());
        (window.painted_quads().into_iter())
            .filter(|quad| quad.bounds.contains(&at) && quad.content_mask.bounds.contains(&at))
            .filter(|quad| {
                let edged = quad.border_widths != Edges::default();
                !quad.background.is_transparent() || (edged && quad.border_color.a > 0.)
            })
            .map(|quad| (quad.background, quad.border_color))
            .collect()
    };
    cx.update_window(window, |_, window, _| quads(window)).unwrap()
}

/// The shell with the tabs in the sidebar and a project for each of `names`, saved, the first
/// selected, motion reduced.
fn sidebar_shell(
    names: &[&str],
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, Vec<std::path::PathBuf>) {
    let roots: Vec<_> = (names.iter())
        .map(|name| repo(name, &[("README.md", "hi\n")]))
        .collect();
    let refs: Vec<&Path> = roots.iter().map(std::path::PathBuf::as_path).collect();
    let (window, shell) = shell_with(&refs, cx);
    cx.update(|cx| settings::update(cx, |settings| settings.tabs = Tabs::Sidebar));
    cx.update(|cx| cx.set_reduce_motion(true));
    shell.update(cx, Shell::save_projects);
    select(&shell, &roots[0], cx);
    draw(window, cx);
    (window, shell, roots)
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
    let (window, shell, roots) = sidebar_shell(&["sidebar-spaces-app", "sidebar-spaces-site"], cx);
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
    let (window, shell, roots) = sidebar_shell(&["sidebar-delete-app", "sidebar-delete-site"], cx);
    act(window, MoveProjectToNewSpace(roots[1].clone()), cx);
    let made = cx.read(|cx| settings::get(cx).shown_space_id());
    act(window, crate::shell::spaces::DeleteSpace(made), cx);
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

/// What the address bar says.
pub(super) fn said(shell: &Entity<Shell>, cx: &TestAppContext) -> String {
    cx.read(|cx| {
        let shell = shell.read(cx);
        super::address::said(shell, shell.shown_page().as_ref()).0.to_string()
    })
}

#[gpui_kit::test]
fn the_address_bar_says_what_is_shown(cx: &mut TestAppContext) {
    let (window, shell, _) = sidebar_shell(&["sidebar-address"], cx);
    assert_eq!(said(&shell, cx), "Search or enter address", "with no tab open");
    let unloaded = Unloaded {
        address: "https://www.example.com/docs?page=2".into(),
        favicon: None,
        restored: true,
        session: koyo_browser_ipc::Session::default(),
    };
    let page = Pane {
        title: "Example".into(),
        ..Pane::of(Content::Browser(Browser::waiting(unloaded, By::Person)))
    };
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
        shell.open_settings_page(Some(SettingsPage::General), cx);
    });
    draw(window, cx);
    assert_eq!(said(&shell, cx), "Settings");
}

/// Clicks the address bar: the launcher it opened, if it opened one, dismissed again.
fn click_address(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    let at = vcx.debug_bounds("sidebar-address").expect("the bar").center();
    vcx.simulate_click(at, Modifiers::default());
    cx.run_until_parked();
    let Some((launcher, _)) = dialog::<Launcher>(cx) else {
        return false;
    };
    crate::test_support::press(launcher, "escape", cx);
    cx.run_until_parked();
    true
}

/// The page shown's address, and whether its address field has the keyboard.
fn address_shown(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    cx: &mut TestAppContext,
) -> Option<(String, bool)> {
    draw(window, cx);
    cx.update_window(window, |_, window, cx| {
        let page = shell.read(cx).shown_page()?;
        let page = page.read(cx);
        let typing = page.address_field().read(cx).focus_handle(cx).is_focused(window);
        Some((page.address(), typing))
    })
    .unwrap()
}

#[gpui_kit::test]
fn with_no_tab_open_a_click_on_the_address_bar_opens_a_page_to_type_an_address_in(
    cx: &mut TestAppContext,
) {
    let (window, shell, _) = sidebar_shell(&["sidebar-address-click"], cx);
    assert_eq!(tabs(&shell, cx).len(), 0);
    assert_eq!(address_shown(window, &shell, cx), None);

    assert!(!click_address(window, cx), "the launcher opened where a page was asked for");
    assert_eq!(tabs(&shell, cx).len(), 1, "a click with no tab open opened no page");
    assert_eq!(
        address_shown(window, &shell, cx),
        Some((String::new(), true)),
        "the new page isn't on its start page with its address ready to type in"
    );

    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("Plan the release"));
        cx.notify();
    });
    assert!(!click_address(window, cx), "a chat has no address");
    assert_eq!(tabs(&shell, cx).len(), 2, "a click on a chat's bar opened a tab");
    open_page(window, &shell, Some("https://example.com/"), cx);
    assert!(click_address(window, cx), "a page's address, to change");
    assert_eq!(tabs(&shell, cx).len(), 3);
}

#[gpui_kit::test]
fn with_no_tab_open_cmd_l_opens_a_page_to_type_an_address_in(cx: &mut TestAppContext) {
    let (window, shell, _) = sidebar_shell(&["sidebar-address-cmd-l"], cx);
    assert_eq!(tabs(&shell, cx).len(), 0);
    crate::test_support::press(window, "cmd-l", cx);
    cx.run_until_parked();
    assert!(dialog::<Launcher>(cx).is_none(), "the launcher opened where a page was asked for");
    assert_eq!(tabs(&shell, cx).len(), 1, "⌘L with no tab open opened no page");
    assert_eq!(address_shown(window, &shell, cx), Some((String::new(), true)));
}

#[gpui_kit::test]
fn the_address_bar_and_the_page_controls_follow_the_pane_the_person_is_in(cx: &mut TestAppContext) {
    let (window, shell, _) = sidebar_shell(&["sidebar-address-split"], cx);
    let said = |cx: &TestAppContext| said(&shell, cx);
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
        cx.read(|cx| (shell.read(cx).shown_page()).map(|page| page.read(cx).address()))
    };
    let click = |pane: u64, cx: &mut TestAppContext| click_pane(window, pane, cx);
    click(app, cx);
    assert_eq!(said(cx), "localhost:3000");
    assert_eq!(page(cx).as_deref(), Some("http://localhost:3000/"));
    click(docs, cx);
    assert_eq!(said(cx), "example.com");
    assert_eq!(page(cx).as_deref(), Some("https://www.example.com/docs"));
    click(chat, cx);
    assert_eq!(said(cx), "Plan the release");
    assert_eq!(page(cx).as_deref(), Some("https://www.example.com/docs"));
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
    crate::chat::tests::tell(shell, summary, cx);
}

/// A tab of chat `0001`, called "A chat", and another tab after it, shown, so nothing marks the
/// chat seen: its pane.
fn unseen_chat(shell: &Entity<Shell>, cx: &mut TestAppContext) -> u64 {
    shell.update(cx, |shell, cx| {
        let thread = koyo_agent::protocol::ids::ThreadId("0001".into());
        let pane = Pane {
            title: "A chat".into(),
            ..Pane::of(Content::Chat(Some(thread)))
        };
        let id = pane.id;
        shell.workspace.push_tab(Tab::Single(pane));
        shell.workspace.push_tab(chat("another"));
        cx.notify();
        id
    })
}

/// What goes on with a turn running or not, and something for the person or nothing.
fn going_on(turn: bool, attention: Option<Attention>) -> Activity {
    let work = Work {
        turn,
        ..Work::default()
    };
    Activity { work, attention }
}

#[gpui_kit::test]
fn a_tab_and_its_checkout_say_a_chat_works_and_that_it_wants_the_person(cx: &mut TestAppContext) {
    let (window, shell, roots) = sidebar_shell(&["sidebar-attention"], cx);
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
            let activity = shell.tab_activity(tab, cx);
            assert_eq!(shell.checkout_activity(&worktree, cx), activity);
            activity
        })
    };
    hear(Status::Working, None, false, cx);
    assert_eq!(told(cx), going_on(true, None));
    hear(Status::Idle, None, true, cx);
    assert_eq!(told(cx), going_on(false, Some(Attention::Unread)));
    hear(Status::Working, None, true, cx);
    assert_eq!(told(cx), going_on(true, Some(Attention::Unread)), "both");
    hear(Status::Working, Some(RequestKind::Approval), false, cx);
    assert_eq!(told(cx), going_on(false, Some(Attention::NeedsYou)));
    hear(Status::Idle, None, false, cx);
    assert_eq!(told(cx), Activity::default(), "seen: nothing");
}

/// The selected checkout's tabs, by their first panes' titles.
fn listed(shell: &Entity<Shell>, cx: &TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        let unit = shell.read(cx).workspace.branch().expect("a checkout");
        let titles = unit.tabs.iter().map(|tab| tab.first().title.to_string());
        titles.collect()
    })
}

/// Takes up what `from` names by its middle and lets it go over the middle of what `to` names,
/// drawing at each step as a person's drag would be drawn.
fn drag(window: AnyWindowHandle, from: &'static str, to: &'static str, cx: &mut TestAppContext) {
    let (held, target) = (center_of(window, from, cx), center_of(window, to, cx));
    hold(window, held, cx);
    move_held(window, target, cx);
    draw(window, cx);
    let_go(window, target, cx);
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
    let (window, shell, roots) = sidebar_shell(&["sidebar-slide-app", "sidebar-slide-site"], cx);
    act(window, MoveProjectToNewSpace(roots[1].clone()), cx);
    assert_eq!(shown(&shell, cx).1, 1);
    cx.update(|cx| cx.set_reduce_motion(false));
    shell.update(cx, |shell, cx| shell.slide_to_space(0, cx));
    let offset = |cx: &TestAppContext| shell.read_with(cx, |shell, _| shell.swipe.offset);
    let frame = std::time::Duration::from_millis(8);
    let mut seen = Vec::new();
    for _ in 0..240 {
        cx.executor().advance_clock(frame);
        (cx.update_window(window, |_, window, cx| window.simulate_next_frame(cx))).unwrap();
        draw(window, cx);
        seen.push((offset(cx), shown(&shell, cx).1));
    }
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
