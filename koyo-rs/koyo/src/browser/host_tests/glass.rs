//! Glass over a page takes the other tone than the page's (`theme::glass`): read from the page's
//! own frame, a white page is light under it, and the glass dark; a black one the other way round.

use gpui_kit::{Bounds, TestAppContext, point, px, size};

use super::super::glass_dark;
use super::{BrowserView, finish, open, wait_for};

/// A page of nothing but `colour`.
fn page(colour: &str) -> String {
    format!("data:text/html,<body style='margin:0;background:{colour}'></body>")
}

/// How light the page is, all of it, once it's drawn.
fn lightness(view: &BrowserView) -> Option<f32> {
    let page = view.page_bounds()?;
    view.under(page).map(|under| under.lightness)
}

/// Whether glass over all of the page is dark, as a launcher over it works it out.
fn launcher_dark(view: &BrowserView) -> Option<bool> {
    glass_dark([view], view.page_bounds()?, None)
}

#[gpui_kit::test]
#[ignore = "needs the engine script/browser-engine packages"]
fn glass_over_a_page_is_dark_over_a_light_one_and_light_over_a_dark_one(cx: &mut TestAppContext) {
    let (view, cx, scratch) = open("glass", &page("white"), cx);
    wait_for(cx, &view, "a white page under the glass", |view| {
        lightness(view).is_some_and(|light| light > 0.95)
    });
    assert_eq!(
        view.read_with(cx, |view, _| launcher_dark(view)),
        Some(true)
    );
    let bar = view.update(cx, |view, _| view.read_bar_dark());
    assert_eq!(bar, Some(true), "the bar's glass over a white page is dark");

    view.update(cx, |view, cx| view.navigate(page("black"), cx));
    wait_for(cx, &view, "a black page under the glass", |view| {
        lightness(view).is_some_and(|light| light < 0.05)
    });
    assert_eq!(
        view.read_with(cx, |view, _| launcher_dark(view)),
        Some(false)
    );
    let bar = view.update(cx, |view, _| view.read_bar_dark());
    assert_eq!(
        bar,
        Some(false),
        "the bar's glass over a black page is light"
    );

    // Over a region of the window the page isn't in, the glass is the theme's.
    let beyond = Bounds::new(point(px(-200.), px(-200.)), size(px(100.), px(100.)));
    let read = view.read_with(cx, |view, _| glass_dark([view], beyond, None));
    assert_eq!(read, None, "glass over none of the page reads none of it");
    finish(cx, scratch);
}
