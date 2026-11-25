//! The loose pages in the sidebar as the space changes (`docs/layout.md`, Motion): shared,
//! they're every space's, so they stay where they are as a space slides out and the next comes
//! in, by the keys or under two fingers, and the one shown stays shown with the keyboard in it,
//! as a pinned page does; each space's own, they slide away with it.

use gpui_kit::{
    AnyWindowHandle, Bounds, Entity, Modifiers, Pixels, ScrollDelta, ScrollEvent, TestAppContext,
    TouchPhase, VisualTestContext, point, px,
};

use super::pinned::page_of;
use super::{act, chat, drawn, sidebar_shell};
use crate::shell::spaces::NextSpace;
use crate::projects::MoveProjectToNewSpace;
use crate::settings::{self, LoosePages};
use crate::shell::Shell;
use crate::test_support::{click, draw, frames, open_page};

const PAGES: [&str; 2] = ["https://one.example/", "https://two.example/"];

/// The shell with the tabs in the sidebar and two spaces, a project each, the first shown with
/// `PAGES` its loose pages, listed as `pages` says, the first shown with the keyboard in it; with
/// motion. Its key.
fn two_spaces(
    names: [&str; 2],
    pages: LoosePages,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Shell>, u64) {
    let (window, shell, roots) = sidebar_shell(&names, cx);
    shell.update(cx, |shell, cx| {
        shell.workspace.push_tab(chat("A chat"));
        cx.notify();
    });
    act(window, MoveProjectToNewSpace(roots[1].clone()), cx);
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    cx.update(|cx| settings::update(cx, |settings| settings.loose_pages = pages));
    for page in PAGES {
        let key = open_page(window, &shell, Some(page), cx);
        shell.update(cx, |shell, cx| shell.loosen_tab_of(key, usize::MAX, cx));
    }
    draw(window, cx);
    click(window, "sidebar-unpinned-0", cx);
    let key = cx.read(|cx| shell.read(cx).workspace.loose()[0].key());
    cx.update(|cx| cx.set_reduce_motion(false));
    (window, shell, key)
}

/// Where the loose pages' rows are drawn, those that are.
fn rows(window: AnyWindowHandle, cx: &mut TestAppContext) -> Vec<Bounds<Pixels>> {
    (0..PAGES.len())
        .filter_map(|at| drawn(window, format!("sidebar-unpinned-{at}"), cx))
        .collect()
}

fn offset(shell: &Entity<Shell>, cx: &TestAppContext) -> f32 {
    cx.read(|cx| shell.read(cx).swipe.offset)
}

fn space(cx: &TestAppContext) -> usize {
    cx.read(|cx| settings::get(cx).space)
}

/// The address of the page the panes show, and whether it's a loose one.
fn shown(shell: &Entity<Shell>, cx: &TestAppContext) -> (Option<String>, bool) {
    cx.read(|cx| {
        let workspace = &shell.read(cx).workspace;
        let address = (workspace.tab_shown()).and_then(|tab| tab.first().address());
        (address.map(str::to_owned), workspace.loose_shown().is_some())
    })
}

/// Two fingers on the trackpad over the sidebar, in `phase`, moved `dx` across.
fn fingers(window: AnyWindowHandle, phase: TouchPhase, dx: f32, cx: &mut TestAppContext) {
    let mut vcx = VisualTestContext::from_window(window, cx);
    let sidebar = vcx.debug_bounds("sidebar").expect("the sidebar");
    vcx.simulate_event(ScrollEvent {
        position: sidebar.bottom_left() + point(px(40.), px(-120.)),
        delta: ScrollDelta::Pixels(point(px(dx), px(0.))),
        modifiers: Modifiers::none(),
        touch_phase: phase,
        ..ScrollEvent::default()
    });
    draw(window, cx);
}

#[gpui_kit::test]
fn shared_loose_pages_stay_put_as_the_space_slides_and_the_one_shown_stays_shown(
    cx: &mut TestAppContext,
) {
    let names = ["sidebar-unpinned-still-app", "sidebar-unpinned-still-site"];
    let (window, shell, key) = two_spaces(names, LoosePages::Shared, cx);
    let (view, keys) = page_of(window, &shell, key, cx).expect("its page, made");
    assert!(keys, "picked, it takes the keyboard");
    let still = rows(window, cx);
    assert_eq!(still.len(), 2);
    let stays = |cx: &mut TestAppContext| {
        assert_eq!(shown(&shell, cx), (Some(PAGES[0].into()), true));
        let now = page_of(window, &shell, key, cx);
        assert_eq!(now, Some((view, true)), "the same page, the keyboard in it");
    };
    let slide = |cx: &mut TestAppContext| {
        let mut slid: Vec<f32> = Vec::new();
        for _ in 0..60 {
            frames(window, 1, cx);
            slid.push(offset(&shell, cx));
            let along = slid.last();
            assert_eq!(rows(window, cx), still, "the space {along:?} points along");
        }
        slid
    };

    act(window, NextSpace, cx);
    let slid = slide(cx);
    assert!(slid.iter().any(|at| at.abs() > 20.), "it slid: {slid:?}");
    assert_eq!((space(cx), offset(&shell, cx)), (1, 0.));
    stays(cx);

    fingers(window, TouchPhase::Started, 0., cx);
    for _ in 0..4 {
        fingers(window, TouchPhase::Moved, 30., cx);
    }
    let held = offset(&shell, cx);
    assert!(held > 50., "pulled toward the first space: {held}");
    assert_eq!(rows(window, cx), still, "under the fingers too");
    fingers(window, TouchPhase::Ended, 0., cx);
    slide(cx);
    assert_eq!((space(cx), offset(&shell, cx)), (0, 0.));
    stays(cx);

    click(window, "space-mark-1", cx);
    slide(cx);
    assert_eq!((space(cx), offset(&shell, cx)), (1, 0.));
    stays(cx);
}

#[gpui_kit::test]
fn a_spaces_own_loose_pages_slide_away_with_it(cx: &mut TestAppContext) {
    let names = ["sidebar-unpinned-slide-app", "sidebar-unpinned-slide-site"];
    let (window, shell, _) = two_spaces(names, LoosePages::PerSpace, cx);
    let still = rows(window, cx);
    assert_eq!(still.len(), 2);
    assert_eq!(shown(&shell, cx), (Some(PAGES[0].into()), true));

    act(window, NextSpace, cx);
    let mut along = Vec::new();
    for _ in 0..60 {
        frames(window, 1, cx);
        if let [first, _] = rows(window, cx)[..] {
            along.push((first.origin.x - still[0].origin.x).as_f32());
        }
    }
    assert!(along.iter().any(|x| *x < -20.), "out the left: {along:?}");
    assert!(rows(window, cx).is_empty(), "the next space has none");
    assert_eq!((space(cx), offset(&shell, cx)), (1, 0.));
    assert_eq!(shown(&shell, cx), (None, false), "the next space's tab");
}
