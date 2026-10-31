use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, Modifiers, MouseButton, TestAppContext,
    VisualTestContext, point, px,
};

use crate::browser::{BrowserView, DOCK_MIN, Dock};
use crate::model::{PaneKind, SplitAxis, Tab};
use crate::settings;
use crate::shell::Shell;
use crate::test_support::{draw, repo, select, shell_with};

fn selector(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

/// The shown tab's panes' ids, and whether they're split side by side.
fn shown(shell: &Entity<Shell>, cx: &mut TestAppContext) -> (Vec<u64>, Option<SplitAxis>) {
    shell.read_with(cx, |shell, _| {
        let unit = shell.workspace.branch().expect("a checkout is selected");
        let tab = &unit.tabs[unit.active_tab];
        let axis = match tab {
            Tab::Split(split) => Some(split.axis),
            Tab::Single(_) => None,
        };
        (tab.panes().iter().map(|pane| pane.id).collect(), axis)
    })
}

#[gpui_kit::test]
fn a_tab_dragged_onto_a_side_of_the_shown_one_splits_with_it_and_leaves_again(
    cx: &mut TestAppContext,
) {
    let root = repo("split-drag", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.new_tab(PaneKind::Chat);
        shell.workspace.new_tab(PaneKind::Terminal);
        cx.notify();
    });
    draw(window, cx);
    let (panes, _) = shown(&shell, cx);
    let target = panes[0];
    let dragged = shell.read_with(cx, |shell, _| {
        shell.workspace.branch().unwrap().tabs[0].panes()[0].id
    });
    let mut vcx = VisualTestContext::from_window(window, cx);
    let none = Modifiers::default();
    let tab = vcx
        .debug_bounds("tab-0-0-0")
        .expect("the first tab is in the strip");
    vcx.simulate_mouse_down(tab.center(), MouseButton::Left, none);
    vcx.simulate_mouse_move(
        tab.center() + point(px(30.), px(30.)),
        Some(MouseButton::Left),
        none,
    );
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    let zone = vcx
        .debug_bounds(selector(format!("drop-{target}-Right")))
        .expect("the pane's sides take a drop while a tab is in hand");
    vcx.simulate_mouse_move(zone.center(), Some(MouseButton::Left), none);
    vcx.simulate_mouse_up(zone.center(), MouseButton::Left, none);
    draw(window, cx);
    assert_eq!(
        shown(&shell, cx),
        (vec![target, dragged], Some(SplitAxis::Row))
    );
    shell.read_with(cx, |shell, _| {
        assert_eq!(
            shell.workspace.branch().unwrap().tabs.len(),
            1,
            "it left the strip"
        );
    });

    // Its header takes it back out into a tab of its own.
    let mut vcx = VisualTestContext::from_window(window, cx);
    let own = vcx
        .debug_bounds(selector(format!("pane-own-tab-{dragged}")))
        .expect("a pane of a split has a header");
    vcx.simulate_click(own.center(), none);
    draw(window, cx);
    assert_eq!(shown(&shell, cx), (vec![dragged], None));
    shell.read_with(cx, |shell, _| {
        assert_eq!(shell.workspace.branch().unwrap().tabs.len(), 2);
    });
}

/// The page of the shown tab, a browser one.
fn page(shell: &Entity<Shell>, cx: &mut TestAppContext) -> Entity<BrowserView> {
    shell.read_with(cx, |shell, _| {
        let unit = shell.workspace.branch().unwrap();
        let tab = &unit.tabs[unit.active_tab];
        let pane = tab.panes()[0];
        pane.browser.clone().expect("a browser pane")
    })
}

#[gpui_kit::test]
fn devtools_dock_at_every_side_detach_into_a_split_and_close_with_their_page(
    cx: &mut TestAppContext,
) {
    let root = repo("devtools-dock", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.open_browser(None, window, cx));
    })
    .unwrap();
    draw(window, cx);
    let page = page(&shell, cx);
    page.update(cx, |page, cx| page.inspect(None, cx));
    draw(window, cx);
    let docked = |cx: &mut TestAppContext| page.read_with(cx, |page, _| page.devtools_dock());
    assert_eq!(docked(cx), Some(Dock::Bottom), "they open at the bottom");
    for dock in [Dock::Left, Dock::Right, Dock::Bottom] {
        page.update(cx, |page, cx| page.dock_devtools(dock, cx));
        draw(window, cx);
        assert_eq!(docked(cx), Some(dock));
        assert_eq!(
            shown(&shell, cx).1,
            None,
            "docked, they're in the page's pane"
        );
    }

    page.update(cx, |page, cx| page.dock_devtools(Dock::Detached, cx));
    draw(window, cx);
    let (panes, axis) = shown(&shell, cx);
    assert_eq!(
        (panes.len(), axis),
        (2, Some(SplitAxis::Row)),
        "detached, they're a pane"
    );
    let tools = page
        .read_with(cx, |page, _| page.devtools().cloned())
        .unwrap();
    shell.read_with(cx, |shell, _| {
        let unit = shell.workspace.branch().unwrap();
        let second = unit.tabs[unit.active_tab].panes()[1].browser.clone();
        assert_eq!(second.as_ref(), Some(&tools), "right of the page");
    });

    page.update(cx, |page, cx| page.dock_devtools(Dock::Right, cx));
    draw(window, cx);
    assert_eq!(shown(&shell, cx).1, None, "docked again, their pane goes");
    assert_eq!(docked(cx), Some(Dock::Right));

    // The next ones open where the last ones went; closing the page's tab takes them along.
    page.update(cx, |page, cx| {
        page.dock_devtools(Dock::Detached, cx);
        page.close_devtools(cx);
    });
    draw(window, cx);
    assert_eq!(shown(&shell, cx).0.len(), 1, "closed, they leave the tab");
    assert_eq!(
        cx.update(|cx| settings::get(cx).devtools.dock),
        Dock::Detached
    );
    page.update(cx, |page, cx| page.inspect(None, cx));
    draw(window, cx);
    assert_eq!(shown(&shell, cx).0.len(), 2, "they open detached");
    shell.update(cx, |shell, cx| {
        let at = (shell.workspace.active_project, 0, 0);
        shell.close_tab_at(at, cx);
    });
    draw(window, cx);
    shell.read_with(cx, |shell, _| {
        assert!(
            shell.workspace.branch().unwrap().tabs.is_empty(),
            "page and DevTools are gone"
        );
    });
}

/// Drags the docked `DevTools`' divider from its middle by `by`, as a person does.
fn drag_dock_divider(window: AnyWindowHandle, by: f32, cx: &mut TestAppContext) {
    let mut vcx = VisualTestContext::from_window(window, cx);
    let divider = vcx
        .debug_bounds("devtools-divider")
        .expect("docked DevTools have a divider");
    let none = Modifiers::default();
    let from = divider.center();
    vcx.simulate_mouse_down(from, MouseButton::Left, none);
    // The first move, past gpui's few pixels, picks the divider up; those after carry it.
    vcx.simulate_mouse_move(
        from + point(px(0.), px(5. * by.signum())),
        Some(MouseButton::Left),
        none,
    );
    let to = from + point(px(0.), px(by));
    vcx.simulate_mouse_move(to, Some(MouseButton::Left), none);
    vcx.simulate_mouse_up(to, MouseButton::Left, none);
    draw(window, cx);
}

#[gpui_kit::test]
fn docked_devtools_take_the_room_their_divider_is_dragged_to_and_keep_it(cx: &mut TestAppContext) {
    let root = repo("devtools-divider", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.open_browser(None, window, cx));
    })
    .unwrap();
    draw(window, cx);
    let page = page(&shell, cx);
    page.update(cx, |page, cx| page.inspect(None, cx));
    draw(window, cx);
    let height = |cx: &mut TestAppContext| cx.update(|cx| settings::get(cx).devtools.height);
    let before = height(cx);

    drag_dock_divider(window, -80., cx);
    let taller = height(cx);
    assert!(
        (taller - (before + 80.)).abs() < 2.,
        "dragged 80 px up from {before}, they're {taller} tall"
    );

    // However far it goes, page and DevTools each keep some room.
    drag_dock_divider(window, 5000., cx);
    assert!((height(cx) - DOCK_MIN).abs() < 0.5, "{}", height(cx));
    drag_dock_divider(window, -5000., cx);
    let tallest = height(cx);
    let window_height = cx
        .update_window(window, |_, window, _| {
            window.viewport_size().height.as_f32()
        })
        .unwrap();
    assert!(
        tallest > taller && tallest < window_height - DOCK_MIN,
        "{tallest} of a {window_height} px window"
    );

    // The size is the next DevTools' too.
    let divider_at = |window: AnyWindowHandle, cx: &mut TestAppContext| {
        let mut vcx = VisualTestContext::from_window(window, cx);
        let divider = vcx
            .debug_bounds("devtools-divider")
            .expect("docked DevTools have a divider");
        divider.center().y.as_f32()
    };
    let dragged_to = divider_at(window, cx);
    page.update(cx, BrowserView::close_devtools);
    draw(window, cx);
    page.update(cx, |page, cx| page.inspect(None, cx));
    draw(window, cx);
    let reopened_at = divider_at(window, cx);
    assert!(
        (reopened_at - dragged_to).abs() < 1.,
        "they open at {reopened_at}, where the last were dragged to {dragged_to}"
    );
}

#[gpui_kit::test]
fn the_divider_of_a_split_shares_its_room_as_dragged(cx: &mut TestAppContext) {
    let root = repo("split-divider", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let target = shell.update(cx, |shell, cx| {
        shell.workspace.new_tab(PaneKind::Chat);
        shell.workspace.new_tab(PaneKind::Terminal);
        let unit = shell.workspace.branch_mut().unwrap();
        let target = unit.tabs[1].panes()[0].id;
        assert!(unit.drop_tab(0, target, crate::model::Edge::Bottom));
        cx.notify();
        target
    });
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    let divider = vcx.debug_bounds("split-[]").expect("the split's divider");
    let none = Modifiers::default();
    vcx.simulate_mouse_down(divider.center(), MouseButton::Left, none);
    // The first move picks the divider up; those after carry it.
    let start = divider.center() + point(px(0.), px(5.));
    vcx.simulate_mouse_move(start, Some(MouseButton::Left), none);
    let lower = divider.center() + point(px(0.), px(60.));
    vcx.simulate_mouse_move(lower, Some(MouseButton::Left), none);
    vcx.simulate_mouse_up(lower, MouseButton::Left, none);
    draw(window, cx);
    shell.update(cx, |shell, _| {
        let unit = shell.workspace.branch_mut().unwrap();
        let split = unit.tabs[0].split_at_mut(&[]).expect("still split");
        assert!(split.ratio > 0.55, "the top grew to {}", split.ratio);
        assert!(unit.tabs[0].has_pane(target));
    });
}
