//! ⌃⇥ tapped shows the tab looked at before with no panel. Held a moment, or pressed again, it
//! shows its panel: the tabs looked at last, files and the rest, the latest first, the one before
//! picked. Every key is heard in the window behind, which keeps the keyboard: each ⌃⇥ moves the
//! pick, letting go of ⌃ shows the tab picked, and Escape leaves things as they were. The pointer
//! moved over a card picks it, and a click shows its tab. Tabs open but not looked at yet are
//! listed after those that were.

use std::path::PathBuf;

use gpui_kit::{
    AnyWindowHandle, Entity, Modifiers, TestAppContext, VisualTestContext, point, px, size,
};

use super::card::Card;
use super::{HOLD, Switcher, placed};
use crate::files::At;
use crate::files::tests::{checkout, editing, let_go, new_chat, press, shell_showing, tabs};
use crate::model::tests::chat;
use crate::popup::{self, Kind};
use crate::shell::Shell;
use crate::test_support::draw;

/// The switcher, if it's up: its cards' titles, the pick, and whether its panel shows.
fn up(cx: &TestAppContext) -> Option<(Vec<String>, usize, bool)> {
    cx.read(super::state)
}

/// The pick and whether the panel shows, with the switcher up.
fn picked(cx: &TestAppContext) -> Option<(usize, bool)> {
    up(cx).map(|(_, picked, shown)| (picked, shown))
}

/// ⌃ held long enough for the panel to show.
fn hold(cx: &mut TestAppContext) {
    cx.executor().advance_clock(HOLD);
    cx.run_until_parked();
}

/// The tab shown, as the tab strip names it.
fn shown(shell: &Entity<Shell>, cx: &TestAppContext) -> Option<String> {
    (tabs(shell, cx).into_iter()).find(|tab| tab.starts_with('['))
}

/// A checkout's shell with its README, `src/lib.rs`, a chat and `src/main.rs` shown in turn: the
/// switcher lists them the latest first, `main.rs`, the chat, `lib.rs`, the README.
fn four_tabs(name: &str, cx: &mut TestAppContext) -> (PathBuf, AnyWindowHandle, Entity<Shell>) {
    let root = checkout(name);
    let (window, shell, _) = shell_showing(&root, cx);
    editing(window, &shell, "src/lib.rs", At::Bytes(0..0), cx);
    // A chat among the files, listed as they are.
    new_chat(window, cx);
    editing(window, &shell, "src/main.rs", At::Bytes(0..0), cx);
    (root, window, shell)
}

/// The four tabs as the switcher lists them.
fn latest() -> Vec<String> {
    ["main.rs", "New chat", "lib.rs", "README.md"]
        .map(String::from)
        .to_vec()
}

#[gpui_kit::test]
fn ctrl_tab_tapped_shows_the_tab_before_and_no_panel(cx: &mut TestAppContext) {
    let (root, window, shell) = four_tabs("shell-switcher-tap", cx);

    // ⌃⇥: the switcher is up on the tabs looked at last, the one before picked, its panel
    // unseen.
    press(window, "ctrl-tab", cx);
    assert_eq!(up(cx), Some((latest(), 1, false)));
    // ⌃ let go at once: the tab before shows, and no panel does, then or later.
    let_go(window, cx);
    assert_eq!(up(cx), None, "gone once ⌃ is let go");
    assert_eq!(shown(&shell, cx).as_deref(), Some("[New chat]"));
    hold(cx);
    assert_eq!(up(cx), None, "no panel after a tap");
    // Tapped again, it's back where it was.
    press(window, "ctrl-tab", cx);
    let_go(window, cx);
    assert_eq!(shown(&shell, cx).as_deref(), Some("[main.rs]"));

    // ⌃⇥ pressed again shows the panel at once, the pick moved on: two back.
    press(window, "ctrl-tab ctrl-tab", cx);
    assert_eq!(picked(cx), Some((2, true)));
    let_go(window, cx);
    assert_eq!(shown(&shell, cx).as_deref(), Some("[lib.rs]"));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn ctrl_tab_held_shows_the_panel_and_letting_go_shows_the_tab_picked(cx: &mut TestAppContext) {
    let (root, window, shell) = four_tabs("shell-switcher-hold", cx);

    // ⌃ held a moment after ⌃⇥: the panel shows.
    press(window, "ctrl-tab", cx);
    assert_eq!(picked(cx), Some((1, false)), "unseen at first");
    hold(cx);
    assert_eq!(up(cx), Some((latest(), 1, true)));
    // Each ⌃⇥ moves the pick on, round the end, and ⌃⇧⇥ back: heard in the window behind.
    press(window, "ctrl-tab ctrl-tab", cx);
    assert_eq!(picked(cx), Some((3, true)));
    press(window, "ctrl-tab", cx);
    assert_eq!(picked(cx), Some((0, true)));
    press(window, "ctrl-shift-tab", cx);
    assert_eq!(picked(cx), Some((3, true)));
    // ⌃ let go: the tab picked shows and the switcher's gone.
    let_go(window, cx);
    assert_eq!(up(cx), None, "gone once ⌃ is let go");
    assert_eq!(shown(&shell, cx).as_deref(), Some("[README.md]"));

    // Held again once the pointer has moved in the window: the panel shows all the same. It
    // once went by what the window had last heard of ⌃, which a moving pointer says isn't held
    // and a ⌃ pressed after a panel that had the keyboard never corrects.
    let within = point(px(300.), px(300.));
    VisualTestContext::from_window(window, cx).simulate_mouse_move(within, None, Modifiers::none());
    press(window, "ctrl-tab", cx);
    hold(cx);
    assert_eq!(picked(cx), Some((1, true)), "shown the second time too");
    // Escape leaves things as they were, and ⌃ let go after it changes nothing.
    press(window, "escape", cx);
    cx.run_until_parked();
    assert_eq!(up(cx), None, "gone on Escape");
    let_go(window, cx);
    assert_eq!(shown(&shell, cx).as_deref(), Some("[README.md]"));
    let _ = std::fs::remove_dir_all(&root);
}

#[gpui_kit::test]
fn tabs_open_but_not_looked_at_yet_are_switched_among_too(cx: &mut TestAppContext) {
    // As the window comes back: the README shown, and two chats beside it never looked at.
    let root = checkout("shell-switcher-unseen");
    let (window, shell, _) = shell_showing(&root, cx);
    shell.update(cx, |shell, cx| {
        let readme = shell.workspace.shown_tab().expect("the README's tab");
        for title in ["Second", "First"] {
            shell.workspace.insert_tab_after(readme, chat(title));
        }
        cx.notify();
    });

    // ⌃⇥ lists them after the README, as the tabs are in order, the first of them picked.
    press(window, "ctrl-tab", cx);
    let listed = ["README.md", "First", "Second"].map(String::from).to_vec();
    assert_eq!(up(cx), Some((listed, 1, false)));
    // Only the tab that's been shown has a picture; the others show their icons.
    let pictured = cx.read(|cx| {
        let switcher = popup::root::<Switcher>(Kind::Switcher, cx)?;
        Some((switcher.cards.iter().map(Card::has_picture)).collect::<Vec<_>>())
    });
    assert_eq!(pictured, Some(vec![true, false, false]));
    let_go(window, cx);
    assert_eq!(shown(&shell, cx).as_deref(), Some("[First]"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_panel_is_over_its_window_on_its_screen_and_shrinks_to_fit_a_narrow_one() {
    let on = |x: f32, y: f32, width: f32, height: f32| {
        gpui_kit::Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    };
    let screen = on(0., 0., 1512., 950.);
    // Over the middle of its window, at full size: five cards are 960 by 185 points.
    let window = on(100., 100., 1200., 800.);
    let (panel, scale) = placed(5, window, Some(screen));
    assert_eq!((panel, scale), (on(220., 407.5, 960., 185.), 1.));
    // Over a window at the screen's right side it's moved to lie on the screen, a margin in.
    let (panel, scale) = placed(5, on(900., 100., 600., 800.), Some(screen));
    assert_eq!((panel, scale), (on(528., 407.5, 960., 185.), 1.));
    // A screen too narrow for it shrinks it, cards and all, to fit with the margins.
    let narrow = on(0., 0., 848., 600.);
    let (panel, scale) = placed(5, on(0., 0., 848., 600.), Some(narrow));
    assert!((scale - 800. / 960.).abs() < 1e-6, "{scale}");
    assert_eq!(panel.origin.x, px(24.));
    // Fewer cards make a narrower panel.
    let (panel, _) = placed(2, window, Some(screen));
    assert_eq!(panel.size, size(px(402.), px(185.)));
    // A window that isn't on its screen at all, a check's, keeps the panel over it.
    let away = on(-10000., -10000., 1600., 1000.);
    let (panel, scale) = placed(5, away, Some(screen));
    assert_eq!((panel.center(), scale), (away.center(), 1.));
}

#[gpui_kit::test]
fn the_pointer_over_a_card_picks_it_and_a_click_shows_its_tab(cx: &mut TestAppContext) {
    let (root, window, shell) = four_tabs("shell-switcher-pointer", cx);
    press(window, "ctrl-tab", cx);
    hold(cx);
    let panel = cx.read(|cx| popup::window(Kind::Switcher, cx));
    let panel = panel.expect("the switcher's panel");
    draw(panel, cx);
    let mut pointer = VisualTestContext::from_window(panel, cx);
    let last = pointer.debug_bounds("tab-switcher-card-3");
    let third = pointer.debug_bounds("tab-switcher-card-2");

    // Moved over the last card, the pointer picks it.
    let last = last.expect("the last card").center();
    pointer.simulate_mouse_move(last, None, Modifiers::default());
    assert_eq!(picked(cx), Some((3, true)));
    // A click on the third, `lib.rs`, shows its tab, and the switcher's gone.
    let third = third.expect("the third card").center();
    pointer.simulate_click(third, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(up(cx), None, "gone once a card is clicked");
    assert_eq!(shown(&shell, cx).as_deref(), Some("[lib.rs]"));
    let _ = std::fs::remove_dir_all(&root);
}
