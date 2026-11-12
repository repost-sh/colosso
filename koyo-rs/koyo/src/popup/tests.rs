//! How the floating windows go, on test windows: a press in the window they float over, but on a
//! menu's own trigger; that window showing something else, but for a dialog, or moving; one of a
//! kind opening over another; leaving, but for a kept dialog; a key a menu has no use for; and a
//! panel fitting what it holds.

use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, App, Bounds, Context, InteractiveElement as _, IntoElement, Modifiers,
    ParentElement as _, Pixels, Render, Size, TestAppContext, VisualTestContext, Window, div,
    point, px, size,
};

use super::*;
use crate::menu::{MenuBody, PADDING, Trigger};
use crate::test_support::{Blank, in_window};

/// A menu's panel as big as it's told, which hears Down.
struct Sized(Size<Pixels>);

impl Render for Sized {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl MenuBody for Sized {
    fn hears(&mut self, key: &str, _: &mut Context<Self>) -> bool {
        key == "down"
    }

    fn extent(&self, _: &Window, _: &App) -> Size<Pixels> {
        self.0
    }
}

/// A window whose whole page keeps the pointer to itself, as a text field or a native view does,
/// with the listener drawn over it.
struct Occluded;

impl Render for Occluded {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(div().id("page").size_full().occlude())
            .child(listener())
    }
}

/// A window to float over, made after the app.
fn behind(cx: &mut TestAppContext) -> AnyWindowHandle {
    cx.update(crate::init);
    in_window(size(px(800.), px(600.)), cx, |_, cx| cx.new(|_| Blank)).0
}

/// A floating window of `kind` over `over`.
fn float(
    kind: Kind,
    source: Option<Source>,
    kept: bool,
    over: AnyWindowHandle,
    cx: &mut TestAppContext,
) -> AnyWindowHandle {
    let opening = Opening {
        kind,
        bounds: Bounds::new(point(px(100.), px(100.)), size(px(200.), px(100.))),
        kept,
        source,
    };
    let opened = cx.update_window(over, |_, window, cx| {
        open(opening, window, cx, |_, _| Blank).map(AnyWindowHandle::from)
    });
    let floating = opened.unwrap().unwrap();
    cx.run_until_parked();
    floating
}

/// Whether `window` is still up, once what was asked of it is done.
fn up(window: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    cx.run_until_parked();
    cx.windows().contains(&window)
}

#[gpui_kit::test]
fn a_press_in_the_window_puts_away_what_floats_over_it_but_on_a_menus_trigger(
    cx: &mut TestAppContext,
) {
    let over = behind(cx);
    let trigger = Bounds::new(point(px(10.), px(10.)), size(px(20.), px(20.)));
    let source = Source::new("trigger");
    source.drawn.set(trigger);
    let menu = float(Kind::Menu, Some(source), false, over, cx);
    let dialog = float(Kind::Dialog, None, false, over, cx);
    // On the trigger, which toggles the menu itself; a dialog goes all the same.
    cx.update(|cx| pressed(over, point(px(15.), px(15.)), cx));
    assert!(up(menu, cx));
    assert!(!up(dialog, cx));
    // Anywhere else, whatever the page shows.
    cx.update(|cx| pressed(over, point(px(400.), px(300.)), cx));
    assert!(!up(menu, cx));
}

#[gpui_kit::test]
fn the_listener_hears_a_press_on_what_keeps_the_pointer_to_itself(cx: &mut TestAppContext) {
    cx.update(crate::init);
    let (over, _) = in_window(size(px(800.), px(600.)), cx, |_, cx| cx.new(|_| Occluded));
    let menu = float(Kind::Menu, None, false, over, cx);
    let page = &mut VisualTestContext::from_window(over, cx);
    page.simulate_click(point(px(400.), px(300.)), Modifiers::none());
    assert!(!up(menu, cx));
}

#[gpui_kit::test]
fn the_window_showing_something_else_puts_away_a_menu_and_a_card_but_not_a_dialog(
    cx: &mut TestAppContext,
) {
    let over = behind(cx);
    cx.update(|cx| showing(over, &"a page", cx));
    let menu = float(Kind::Menu, None, false, over, cx);
    let card = float(Kind::Card, None, false, over, cx);
    let dialog = float(Kind::Dialog, None, false, over, cx);
    // The same again, as the window redraws, changes nothing.
    cx.update(|cx| showing(over, &"a page", cx));
    assert!(up(menu, cx));
    cx.update(|cx| showing(over, &"another page", cx));
    assert!(!up(menu, cx));
    assert!(!up(card, cx));
    assert!(up(dialog, cx));
}

#[gpui_kit::test]
fn the_window_moving_or_resized_puts_away_what_floats_over_it(cx: &mut TestAppContext) {
    let over = behind(cx);
    let menu = float(Kind::Menu, None, false, over, cx);
    cx.simulate_window_resize(over, size(px(900.), px(600.)));
    assert!(!up(menu, cx));
}

#[gpui_kit::test]
fn one_of_a_kind_opening_puts_the_last_away(cx: &mut TestAppContext) {
    let over = behind(cx);
    let first = float(Kind::Menu, None, false, over, cx);
    let second = float(Kind::Menu, None, false, over, cx);
    assert!(!up(first, cx));
    assert!(up(second, cx));
    assert_eq!(cx.read(|cx| window(Kind::Menu, cx)), Some(second));
}

#[gpui_kit::test]
fn leaving_puts_away_all_but_a_kept_dialog(cx: &mut TestAppContext) {
    let over = behind(cx);
    (cx.update_window(over, |_, window, _| window.activate_window())).unwrap();
    let menu = float(Kind::Menu, None, false, over, cx);
    let kept = float(Kind::Dialog, None, true, over, cx);
    // The person goes to another window.
    let (elsewhere, _) = in_window(size(px(400.), px(300.)), cx, |_, cx| cx.new(|_| Blank));
    (cx.update_window(elsewhere, |_, window, _| window.activate_window())).unwrap();
    cx.executor()
        .advance_clock(Duration::from_millis(2 * SETTLE));
    assert!(!up(menu, cx));
    assert!(up(kept, cx));
}

#[gpui_kit::test]
fn a_key_a_menu_has_no_use_for_puts_it_away(cx: &mut TestAppContext) {
    let over = behind(cx);
    let trigger = Trigger::new("panel");
    let body = cx.new(|_| Sized(size(px(200.), px(100.))));
    (cx.update_window(over, |_, window, cx| {
        trigger.toggle_panel(|_| body, window, cx);
    }))
    .unwrap();
    cx.run_until_parked();
    let menu = cx.read(|cx| window(Kind::Menu, cx)).unwrap();
    // One it hears keeps it up.
    cx.simulate_keystrokes(over, "down");
    assert!(up(menu, cx));
    // One it doesn't, as the shortcut to Settings, puts it away on its way to the window.
    cx.simulate_keystrokes(over, "cmd-,");
    assert!(!up(menu, cx));
}

#[gpui_kit::test]
fn a_panel_fits_what_it_holds_as_that_changes(cx: &mut TestAppContext) {
    let over = behind(cx);
    let trigger = Trigger::new("panel");
    let body = cx.new(|_| Sized(size(px(200.), px(100.))));
    let shown = body.clone();
    (cx.update_window(over, |_, window, cx| {
        trigger.toggle_panel(|_| shown, window, cx);
    }))
    .unwrap();
    cx.run_until_parked();
    let menu = cx.read(|cx| window(Kind::Menu, cx)).unwrap();
    body.update(cx, |body, cx| {
        body.0 = size(px(200.), px(180.));
        cx.notify();
    });
    cx.run_until_parked();
    // With the window's padding round it.
    let fitted = cx.update_window(menu, |_, window, _| window.bounds().size);
    let padded = size(px(200. + 2. * PADDING), px(180. + 2. * PADDING));
    assert_eq!(fitted.unwrap(), padded);
}
