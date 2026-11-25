//! The window has one theme (`docs/layout.md`, Space menu and theme): switching spaces never
//! changes its colours, and Settings → Appearance edits that one theme whatever space shows.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{AnyWindowHandle, Entity, TestAppContext, point};

use super::act;
use crate::gradient::{self, GradientTheme};
use crate::navigation::SettingsPage;
use crate::settings;
use crate::shell::spaces::{NewSpace, NextSpace, PreviousSpace};
use crate::shell::Shell;
use crate::test_support::{click, draw, shell_on};

/// The shell on a checkout of its own, motion reduced so a space switches at once, the window's
/// one theme, light and dark, given a colour of its own.
fn coloured_shell(name: &str, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    let (window, shell, _) = shell_on(name, cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update(|cx| {
        settings::update(cx, |settings| {
            for dark in [false, true] {
                settings.theme_mut(dark).move_primary(point(300., 120.));
            }
        });
    });
    draw(window, cx);
    (window, shell)
}

/// The theme the window shows, and the space shown.
fn shown(cx: &TestAppContext) -> (GradientTheme, usize) {
    cx.read(|cx| (gradient::current(cx).clone(), settings::get(cx).space))
}

/// A new space, then ⌥⇧← and ⌥⇧→ between the two: the window's colours stay as they were.
#[gpui_kit::test]
fn a_space_switch_leaves_the_colours(cx: &mut TestAppContext) {
    let (window, _shell) = coloured_shell("one-theme-switch", cx);
    let (colours, _) = shown(cx);
    assert_ne!(colours, GradientTheme::default(), "the theme has a colour");
    act(window, NewSpace, cx);
    assert_eq!(shown(cx), (colours.clone(), 1), "the new space, in the same colours");
    act(window, PreviousSpace, cx);
    assert_eq!(shown(cx), (colours.clone(), 0));
    act(window, NextSpace, cx);
    assert_eq!(shown(cx), (colours, 1));
}

/// With a second space shown, Settings → Appearance → Glass → Reset resets the window's one
/// theme: the first space shows it reset too.
#[gpui_kit::test]
fn an_appearance_edit_goes_to_the_one_theme(cx: &mut TestAppContext) {
    let (window, shell) = coloured_shell("one-theme-glass", cx);
    act(window, NewSpace, cx);
    shell.update(cx, |shell, cx| shell.open_settings_page(Some(SettingsPage::Appearance), cx));
    draw(window, cx);
    click(window, "reset-glass", cx);
    let reset = GradientTheme::default();
    assert_eq!(shown(cx), (reset.clone(), 1), "the glass is reset");
    let one = cx.read(|cx| settings::get(cx).theme(cx.theme().mode.is_dark()).clone());
    assert_eq!(one, reset, "the window's one theme took the edit");
    act(window, PreviousSpace, cx);
    assert_eq!(shown(cx), (reset, 0), "and every space shows it");
}
