//! **Tabs** in a test window (`docs/layout.md`, Tabs and Sidebar): every tab moving between the
//! top rail and the sidebar together, the rail going with them; a branch's tabs a step further in
//! than its row, every row as wide as the sidebar; a branch's row collapsing its tabs in the
//! sidebar and selecting it with them on the rail; ⌥↑ and ⌥↓ along the tabs where they are; the
//! Changes and Files buttons where their setting and the tabs put them; and the unpinned pages'
//! rows in the one scroll area with the projects, under the address bar and the pinned tiles.

use std::path::PathBuf;

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Pixels, TestAppContext, point, px,
};

use crate::shell::spaces::{NextTab, PreviousTab};
use crate::model::tests::chat;
use crate::navigation::OpenSettings;
use crate::projects::read_worktrees;
use crate::right_panel::View;
use crate::settings::{
    self, ChangesAndFiles, LoosePages, SetChangesAndFiles, SetLoosePages, SetTabs, Tabs,
};
use crate::shell::Shell;
use crate::test_support::{
    act, center_of, click, draw, drawn, git, hover, open_page, pin, shell_on, shell_on_chat,
};
use crate::theme::TITLE_BAR_HEIGHT;

/// Whether `bounds` lie in the top rail.
fn in_rail(bounds: Bounds<Pixels>) -> bool {
    bounds.top() >= px(0.) && bounds.bottom() <= px(TITLE_BAR_HEIGHT)
}

/// Where `selector` names something in `window`, drawn again; it must be drawn.
fn bounds(window: AnyWindowHandle, selector: &str, cx: &mut TestAppContext) -> Bounds<Pixels> {
    drawn(window, selector, cx).unwrap_or_else(|| panic!("{selector} is drawn"))
}

/// Whether `inner` lies across `outer`'s width and within its height.
fn within(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.left() >= outer.left()
        && inner.right() <= outer.right()
        && inner.top() >= outer.top()
        && inner.bottom() <= outer.bottom()
}

/// The one layout over a chat on its checkout, with an unpinned page and a pinned one: the window
/// and the shell.
fn with_pages(name: &str, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    let (window, shell) = shell_on_chat(name, cx);
    let page = open_page(window, &shell, Some("https://news.example/"), cx);
    shell.update(cx, |shell, cx| shell.loosen_tab_of(page, usize::MAX, cx));
    pin(window, &shell, &["https://maps.example/"], cx);
    (window, shell)
}

/// The folder of the first project's checkout of `branch`.
fn checkout_of(shell: &Shell, branch: &str) -> PathBuf {
    let units = &shell.workspace.projects[0].branches;
    let unit = units.iter().find(|unit| unit.name() == branch);
    unit.expect("the checkout").worktree.path.clone()
}

/// The one layout over a project with a worktree `feat` beside its main checkout, listed in that
/// order, the main checkout selected and motion reduced: the window, the shell, and the project's
/// folder and its two checkouts' as the workspace has them.
fn with_feat(
    name: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, [PathBuf; 3]) {
    let (window, shell, root) = shell_on(name, cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let folder = root.with_file_name(format!("{name}-feat"));
    let _ = std::fs::remove_dir_all(&folder);
    git(&root, &["worktree", "add", "-q", "-b", "feat", folder.to_str().unwrap()]);
    let paths = shell.update(cx, |shell, cx| {
        shell.show_worktrees(&root, read_worktrees(&root), cx);
        let order = ["main", "feat"].map(|branch| checkout_of(shell, branch));
        settings::update(cx, |settings| settings.set_worktree_order(&root, order.to_vec()));
        shell.show_worktrees(&root, read_worktrees(&root), cx);
        shell.workspace.select_branch(0, 0);
        cx.notify();
        let [main, feat] = ["main", "feat"].map(|branch| checkout_of(shell, branch));
        [shell.workspace.projects[0].path.clone(), main, feat]
    });
    draw(window, cx);
    (window, shell, paths)
}

/// With the tabs on the top rail the pinned pages, the unpinned ones, their +, the branch's tabs
/// and the Settings tab are on it, and none in the sidebar; with them in the sidebar there's no
/// rail, the sidebar's top row has the window's buttons, and it lists them all from the top: the
/// pinned tiles, the unpinned rows, the Settings tab's while it's open, New Tab's, then the
/// projects, the branch's tabs under its row.
#[gpui_kit::test]
fn the_tabs_go_into_the_sidebar_together_and_take_the_rail_away(cx: &mut TestAppContext) {
    let (window, _) = with_pages("sidebar-tabs-in-sidebar", cx);
    let rail = ["rail-pinned", "rail-unpinned-0", "rail-new-page", "tab-0-0-0", "rail-title"];
    let sidebar_only = ["sidebar-pinned", "sidebar-unpinned-0", "sidebar-new-page", "sidebar-top"];
    let on_rail = |cx: &mut TestAppContext| {
        rail.map(|selector| drawn(window, selector, cx).is_some_and(in_rail))
    };
    let in_sidebar = |cx: &mut TestAppContext| {
        sidebar_only.map(|selector| drawn(window, selector, cx).is_some())
    };
    assert_eq!(on_rail(cx), [true; 5], "{rail:?} on the rail");
    assert_eq!(in_sidebar(cx), [false; 4], "none of {sidebar_only:?}");

    act(window, SetTabs(Tabs::Sidebar), cx);
    assert_eq!(on_rail(cx), [false; 5], "no rail, nor anything on one");
    assert_eq!(in_sidebar(cx), [true; 4]);
    let sidebar = bounds(window, "sidebar", cx);
    let top = bounds(window, "sidebar-top", cx);
    assert!(top.bottom() <= sidebar.top(), "{top:?} over {sidebar:?}");
    assert!(within(bounds(window, "toggle-sidebar", cx), top), "the toggle in the top row");
    let down = [
        "sidebar-pinned",
        "sidebar-unpinned-0",
        "sidebar-new-page",
        "sidebar-project-0",
        "sidebar-branch-0-0",
        "tab-0-0-0",
    ];
    let rows = down.map(|selector| bounds(window, selector, cx));
    for (selector, row) in down.iter().zip(rows) {
        assert!(within(row, sidebar), "{selector} {row:?} in {sidebar:?}");
    }
    for pair in rows.windows(2) {
        assert!(pair[0].bottom() <= pair[1].top(), "{down:?} from the top: {rows:?}");
    }

    act(window, OpenSettings, cx);
    let rows = ["sidebar-unpinned-0", "sidebar-settings-tab", "sidebar-new-page"];
    let [loose, settings_row, new_tab] = rows.map(|selector| bounds(window, selector, cx));
    let after = loose.bottom() <= settings_row.top() && settings_row.bottom() <= new_tab.top();
    assert!(after, "the Settings tab's row after the unpinned pages', before New Tab's");
    assert!(drawn(window, "rail-settings-tab", cx).is_none(), "not on a rail");

    act(window, SetTabs(Tabs::TopRail), cx);
    assert!(drawn(window, "rail-settings-tab", cx).is_some_and(in_rail), "back on the rail");
    assert!(drawn(window, "sidebar-settings-tab", cx).is_none());
    assert_eq!(in_sidebar(cx), [false; 4], "the sidebar left with its rows alone");
}

/// New Tab's row is an unpinned page's, the next down, as wide and as tall: nothing behind it at
/// rest, and lit under the pointer as theirs are; with no page it's there alone, as wide.
#[gpui_kit::test]
fn new_tab_is_a_row_as_the_unpinned_pages_are_lit_only_under_the_pointer(cx: &mut TestAppContext) {
    let (window, shell) = with_pages("sidebar-new-tab-row", cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    act(window, SetTabs(Tabs::Sidebar), cx);
    hover(window, point(px(1000.), px(500.)), cx);
    let [page, new_tab] =
        ["sidebar-unpinned-0", "sidebar-new-page"].map(|selector| bounds(window, selector, cx));
    assert_eq!(new_tab.size, page.size);
    assert_eq!(new_tab.left(), page.left());
    assert_eq!(new_tab.top(), page.bottom() + px(super::super::rows::GAP));

    let look = cx.read(super::super::Look::of);
    let fill = |cx: &mut TestAppContext| {
        let there = super::painted(new_tab.center(), window, cx);
        there.last().map(|(fill, _)| *fill)
    };
    assert_ne!(fill(cx), Some(look.row_hover.into()), "nothing behind it at rest");
    hover(window, new_tab.center(), cx);
    assert_eq!(fill(cx), Some(look.row_hover.into()), "lit under the pointer");
    hover(window, point(px(1000.), px(500.)), cx);
    assert_ne!(fill(cx), Some(look.row_hover.into()), "nothing once the pointer is off it");

    let key = cx.read(|cx| shell.read(cx).workspace.loose()[0].key());
    cx.update_window(window, |_, window, cx| {
        shell.update(cx, |shell, cx| shell.close_apart(key, Shell::close_loose, window, cx));
    })
    .unwrap();
    assert!(drawn(window, "sidebar-unpinned-0", cx).is_none(), "the page closed");
    let alone = bounds(window, "sidebar-new-page", cx);
    assert_eq!((alone.left(), alone.size), (page.left(), page.size));
}

/// With the tabs in the sidebar a branch's tabs start a step further in than its row, as its row
/// does than its project's, the same step, and every row, the tab shown's lit one too, is as wide
/// as its project's: the sidebar's width.
#[gpui_kit::test]
fn a_branchs_tabs_are_a_step_in_from_its_row_and_as_wide(cx: &mut TestAppContext) {
    let (window, _) = shell_on_chat("sidebar-tabs-indent", cx);
    act(window, SetTabs(Tabs::Sidebar), cx);
    let [project, branch, tab] =
        ["sidebar-project-0", "sidebar-branch-0-0", "tab-0-0-0"].map(|row| bounds(window, row, cx));
    for row in [branch, tab] {
        let wide = (row.left(), row.size.width);
        assert_eq!(wide, (project.left(), project.size.width), "{row:?} as {project:?}");
    }
    let icons = ["sidebar-project-icon-0", "sidebar-branch-icon-0-0", "tab-icon-0-0-0"];
    let [folder, glyph, page] = icons.map(|icon| bounds(window, icon, cx));
    assert_eq!(glyph.left() - folder.left(), px(settings::INDENT), "{glyph:?} in from {folder:?}");
    assert_eq!(page.left() - glyph.left(), px(settings::INDENT), "{page:?} in from {glyph:?}");
}

/// With the tabs in the sidebar a branch's row collapses its tabs and brings them back, kept for
/// the next launch, and selects nothing; with them on the rail it selects the branch, and leaves
/// its tabs as they were.
#[gpui_kit::test]
fn a_branchs_row_collapses_its_tabs_in_the_sidebar_and_selects_it_on_the_rail(
    cx: &mut TestAppContext,
) {
    let (window, shell, [project, main, feat]) = with_feat("sidebar-branch-press", cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("m1"));
        cx.notify();
    });
    act(window, SetTabs(Tabs::Sidebar), cx);
    let selected = |cx: &TestAppContext| {
        cx.read(|cx| (shell.read(cx).workspace.project()).map(|project| project.active_branch))
    };
    let collapsed = |checkout: &PathBuf, cx: &TestAppContext| {
        cx.read(|cx| settings::get(cx).is_collapsed(&project, checkout))
    };
    assert!(drawn(window, "tab-0-0-0", cx).is_some(), "the tab under its branch's row");

    click(window, "sidebar-branch-0-0", cx);
    assert!(collapsed(&main, cx));
    assert!(drawn(window, "tab-0-0-0", cx).is_none(), "collapsed");
    click(window, "sidebar-branch-0-1", cx);
    assert!(collapsed(&feat, cx));
    assert_eq!(selected(cx), Some(0), "the click selects nothing");
    click(window, "sidebar-branch-0-0", cx);
    assert!(!collapsed(&main, cx));
    assert!(drawn(window, "tab-0-0-0", cx).is_some(), "back");

    act(window, SetTabs(Tabs::TopRail), cx);
    click(window, "sidebar-branch-0-1", cx);
    assert_eq!(selected(cx), Some(1), "selected");
    assert!(collapsed(&feat, cx), "its tabs left as they were");
}

/// With the tabs in the sidebar the selected branch's row has nothing behind it while its tabs are
/// listed under it, the one shown lit there; collapsed, or with the tabs on the rail, it's lit.
#[gpui_kit::test]
fn the_selected_branchs_row_is_lit_only_while_its_tabs_arent_listed(cx: &mut TestAppContext) {
    let (window, shell, _) = with_feat("sidebar-branch-lit", cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("m1"));
        cx.notify();
    });
    let pill = cx.read(super::super::Look::of).pill;
    let lit = |selector: &'static str, cx: &mut TestAppContext| {
        hover(window, point(px(1000.), px(500.)), cx);
        let at = bounds(window, selector, cx).center();
        super::painted(at, window, cx).last().map(|(fill, _)| *fill) == Some(pill.into())
    };
    act(window, SetTabs(Tabs::TopRail), cx);
    assert!(lit("sidebar-branch-0-0", cx), "on the rail");
    act(window, SetTabs(Tabs::Sidebar), cx);
    assert!(!lit("sidebar-branch-0-0", cx), "its tabs listed");
    assert!(lit("tab-0-0-0", cx), "the tab shown is");
    click(window, "sidebar-branch-0-0", cx);
    assert!(drawn(window, "tab-0-0-0", cx).is_none());
    assert!(lit("sidebar-branch-0-0", cx), "collapsed");
}

/// The sidebar's rows step in as far as Sidebar indentation says, and stay as wide.
#[gpui_kit::test]
fn the_rows_step_in_as_far_as_set_and_stay_as_wide(cx: &mut TestAppContext) {
    let (window, _) = shell_on_chat("sidebar-indent-set", cx);
    act(window, SetTabs(Tabs::Sidebar), cx);
    for step in settings::INDENT_STOPS {
        cx.update(|cx| settings::update(cx, |settings| settings.sidebar_indent = step));
        draw(window, cx);
        let icons = ["sidebar-project-icon-0", "sidebar-branch-icon-0-0", "tab-icon-0-0-0"];
        let [folder, glyph, page] = icons.map(|icon| bounds(window, icon, cx));
        assert_eq!(glyph.left() - folder.left(), px(step), "{step}");
        assert_eq!(page.left() - glyph.left(), px(step), "{step}");
        let rows = ["sidebar-project-0", "sidebar-branch-0-0", "tab-0-0-0"];
        let [project, branch, tab] = rows.map(|row| bounds(window, row, cx));
        assert_eq!(branch.size.width, project.size.width, "{step}");
        assert_eq!(tab.size.width, project.size.width, "{step}");
    }
}

/// What the panes show: "loose" for an unpinned page, else the title of the branch's tab.
fn shown(shell: &Entity<Shell>, cx: &TestAppContext) -> String {
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        if workspace.loose_shown().is_some() {
            return "loose".into();
        }
        let tab = (workspace.shown_checkout_tab()).and_then(|_| workspace.tab_shown());
        tab.map_or_else(String::new, |tab| tab.first().title.to_string())
    })
}

/// ⌥↓ if `down`, else ⌥↑: what the panes show after.
fn step(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    down: bool,
    cx: &mut TestAppContext,
) -> String {
    if down {
        act(window, NextTab, cx);
    } else {
        act(window, PreviousTab, cx);
    }
    shown(shell, cx)
}

/// ⌥↑ and ⌥↓ follow the tabs: along the rail, the unpinned page and the selected branch's tabs;
/// down the sidebar, the unpinned page and every branch's tabs in the order they're listed, the
/// next branch's selected as its tab shows, a collapsed branch's and a folded project's passed
/// over. Past either end they show nothing else.
#[gpui_kit::test]
fn option_up_and_down_go_along_the_tabs_where_they_are(cx: &mut TestAppContext) {
    let (window, shell, [project, main, _]) = with_feat("sidebar-tab-steps", cx);
    shell.update(cx, |shell, cx| {
        for (branch, title) in [(0, "m1"), (1, "f1")] {
            shell.workspace.select_branch(0, branch);
            shell.workspace.push_tab(chat(title));
        }
        shell.workspace.select_tab(0, 0, 0);
        cx.notify();
    });
    let page = open_page(window, &shell, Some("https://example.com/loose"), cx);
    shell.update(cx, |shell, cx| {
        shell.loosen_tab_of(page, 0, cx);
        shell.workspace.select_tab(0, 0, 0);
        cx.notify();
    });
    draw(window, cx);
    assert_eq!(shown(&shell, cx), "m1");
    let go = |down, cx: &mut TestAppContext| step(window, &shell, down, cx);

    let rail = [false, true, true].map(|down| go(down, cx));
    assert_eq!(rail, ["loose", "m1", "m1"], "the rail has the selected branch's alone");

    act(window, SetTabs(Tabs::Sidebar), cx);
    assert_eq!(go(true, cx), "f1", "on to the next branch's tab");
    let branch = cx.read(|cx| shell.read(cx).workspace.project().map(|it| it.active_branch));
    assert_eq!(branch, Some(1), "its branch selected");
    let sidebar = [true, false, false, false].map(|down| go(down, cx));
    assert_eq!(sidebar, ["f1", "m1", "loose", "loose"], "down the sidebar's rows");

    cx.update(|cx| settings::update(cx, |settings| settings.toggle_collapsed(&project, &main)));
    draw(window, cx);
    assert_eq!(go(true, cx), "f1", "past the collapsed branch's");
    cx.update(|cx| settings::update(cx, |settings| settings.toggle_folded(&project)));
    draw(window, cx);
    let folded = [false, true].map(|down| go(down, cx));
    assert_eq!(folded, ["loose", "loose"], "nothing of the folded project's");
}

/// The Changes and Files buttons are at the rail's end while they're placed there and the tabs
/// are on the rail; placed on the worktree, they're on the branch's row under the pointer, and
/// the one whose panel is open stays; with the tabs in the sidebar there's no rail, so they're on
/// the row wherever they're placed.
#[gpui_kit::test]
fn changes_and_files_sit_on_the_rail_or_on_the_branchs_row(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("sidebar-tabs-panel-buttons", cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    draw(window, cx);
    let away = point(px(900.), px(450.));
    let files = |cx: &mut TestAppContext| drawn(window, "toggle-files-0-0", cx);
    let on_row = |cx: &mut TestAppContext| {
        let row = bounds(window, "sidebar-branch-0-0", cx);
        files(cx).is_some_and(|button| within(button, row))
    };
    let pointed = |cx: &mut TestAppContext| {
        hover(window, center_of(window, "sidebar-branch-0-0", cx), cx);
        on_row(cx)
    };

    hover(window, away, cx);
    assert!(files(cx).is_some_and(in_rail), "at the rail's end");
    assert!(!pointed(cx), "not on the branch's row");

    act(window, SetChangesAndFiles(ChangesAndFiles::Worktree), cx);
    hover(window, away, cx);
    assert!(files(cx).is_none(), "off the rail, and off the row away from the pointer");
    assert!(pointed(cx), "on the branch's row under the pointer");
    click(window, "toggle-files-0-0", cx);
    let open = |cx: &TestAppContext| cx.read(|cx| shell.read(cx).right_panel.shows(View::Files));
    assert!(open(cx));
    hover(window, away, cx);
    assert!(on_row(cx), "kept on the row while its panel is open");
    click(window, "toggle-files-0-0", cx);
    assert!(!open(cx));

    act(window, SetChangesAndFiles(ChangesAndFiles::TopRail), cx);
    act(window, SetTabs(Tabs::Sidebar), cx);
    hover(window, away, cx);
    assert!(files(cx).is_none(), "no rail to hold them");
    assert!(pointed(cx), "on the branch's row, the tabs being in the sidebar");
}

/// With the tabs in the sidebar the unpinned pages' rows and New Tab's are in the one scroll area
/// with the projects, shared or each space's own, and the address bar and the pinned tiles over
/// it, the foot under it, stay out of it.
#[gpui_kit::test]
fn the_unpinned_rows_scroll_with_the_projects_under_the_pinned_tiles(cx: &mut TestAppContext) {
    let (window, _) = with_pages("sidebar-tabs-scroll", cx);
    act(window, SetTabs(Tabs::Sidebar), cx);
    for pages in [LoosePages::Shared, LoosePages::PerSpace] {
        act(window, SetLoosePages(pages), cx);
        let space = bounds(window, "sidebar-space", cx);
        let scrolled = ["sidebar-unpinned-0", "sidebar-new-page", "sidebar-project-0", "tab-0-0-0"];
        for selector in scrolled {
            let row = bounds(window, selector, cx);
            assert!(within(row, space), "{pages:?}: {selector} {row:?} in {space:?}");
        }
        for selector in ["sidebar-address", "sidebar-pinned"] {
            let fixed = bounds(window, selector, cx);
            assert!(fixed.bottom() <= space.top(), "{pages:?}: {selector} {fixed:?} over it");
        }
        let foot = bounds(window, "sidebar-foot", cx);
        assert!(space.bottom() <= foot.top(), "{pages:?}: the foot {foot:?} under it");
    }
}

/// The scrolling rows fade at an edge while there's more past it, the whole width of them and no
/// further than the area they scroll in, and not at an end they're scrolled to; what's still over
/// and under them isn't in it.
#[gpui_kit::test]
fn the_scrolling_rows_fade_at_an_edge_theres_more_past(cx: &mut TestAppContext) {
    let (window, shell) = shell_on_chat("sidebar-edge-fade", cx);
    act(window, SetTabs(Tabs::Sidebar), cx);
    let fades = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, _| {
            let scale = window.scale_factor();
            let unscaled = |length: gpui_kit::ScaledPixels| px(length.0 / scale);
            (window.painted_edge_fades().into_iter())
                .map(|(at, top, bottom)| {
                    let (from, to) = (unscaled(at.top()), unscaled(at.bottom()));
                    ((from, to), unscaled(top), unscaled(bottom))
                })
                .collect::<Vec<_>>()
        })
        .unwrap()
    };
    assert_eq!(fades(cx), [], "they fit");

    shell.update(cx, |shell, cx| {
        for _ in 0..60 {
            shell.workspace.push_tab(chat("a tab"));
        }
        cx.notify();
    });
    draw(window, cx);
    let space = bounds(window, "sidebar-space", cx);
    let whole = (space.top(), space.bottom());
    let full = px(super::super::edge_fade::FADE);
    let scrolled = |to: Option<f32>, cx: &mut TestAppContext| {
        shell.update(cx, |shell, cx| {
            let scroll = &shell.sidebar_scroll;
            let down = to.map_or(scroll.max_offset().y, px);
            scroll.set_offset(point(px(0.), -down));
            cx.notify();
        });
        draw(window, cx);
        fades(cx)
    };
    assert_eq!(scrolled(Some(0.), cx), [(whole, px(0.), full)], "at the top");
    assert_eq!(scrolled(Some(200.), cx), [(whole, full, full)], "between");
    assert_eq!(scrolled(None, cx), [(whole, full, px(0.))], "at the bottom");
}
