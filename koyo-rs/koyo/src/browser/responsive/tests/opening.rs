//! The mode a page an agent opens starts in: the person's size for agents' pages unless the
//! call asks for another, a side it leaves out the default's, and the page opening at it out
//! of sight, before any pane has drawn it.

use std::path::Path;

use gpui_kit::{AppContext as _, TestAppContext, px, size};
use koyo_browser_ipc::Viewport;

use super::super::{Change, Choice, Responsive, SetBy, Zoom, opening};
use super::iphone;
use crate::browser::view::BrowserView;
use crate::test_support::{draw, in_window};

fn agent() -> SetBy {
    SetBy::Agent("claude-code (in terminal term_1)".into())
}

/// What a page opens in, at a window's scale of 2.
fn opened(
    asked: Option<&Change>,
    default: Option<(u32, u32)>,
) -> Result<Option<Responsive>, String> {
    opening(asked, default, 2., &agent())
}

fn sized(width: Option<u32>, height: Option<u32>) -> Change {
    Change {
        width,
        height,
        ..Change::default()
    }
}

#[test]
fn a_page_opens_at_the_persons_size_unless_the_call_asks_for_another() {
    let desktop = Some((1920, 1080));
    assert_eq!(
        opened(None, desktop),
        Ok(Some(Responsive::free(1920, 1080, Zoom::Fit, agent())))
    );
    assert_eq!(
        opened(None, None),
        Ok(None),
        "the person chose to fill the pane"
    );
    let fill = Change {
        fill: true,
        ..Change::default()
    };
    assert_eq!(opened(Some(&fill), desktop), Ok(None));
    // A side left out is the person's size's; with none, the pane's as it's assumed undrawn.
    let narrow = opened(Some(&sized(Some(1280), None)), desktop)
        .unwrap()
        .unwrap();
    assert_eq!((narrow.width, narrow.height), (1280, 1080));
    let both = opened(Some(&sized(Some(1280), Some(720))), None)
        .unwrap()
        .unwrap();
    assert_eq!((both.width, both.height, both.device), (1280, 720, None));
    let phone = Change {
        device: Some(Choice::Preset(iphone())),
        ..Change::default()
    };
    let phone = opened(Some(&phone), desktop).unwrap().unwrap();
    assert_eq!(phone.device, Some(iphone()));
    assert!(phone.mobile && phone.touch);
    let upright = Change {
        landscape: Some(false),
        ..Change::default()
    };
    let upright = opened(Some(&upright), desktop).unwrap().unwrap();
    assert_eq!((upright.width, upright.height), (1080, 1920));
    // Larger than this window shows, the person's size is brought within it, as a drag stops.
    let huge = opened(None, Some((20_000, 1080))).unwrap().unwrap();
    assert_eq!((huge.width, huge.height), (8192, 1080));
    assert_eq!(huge.set_by, agent());
    for wrong in [
        Change::default(),
        Change {
            fill: true,
            width: Some(300),
            ..Change::default()
        },
        sized(Some(20), None),
        Change {
            device: Some(Choice::Preset(iphone())),
            width: Some(300),
            ..Change::default()
        },
    ] {
        assert!(opened(Some(&wrong), desktop).is_err(), "{wrong:?}");
    }
}

/// A page of example.com made in `mode` in a window of `size`, as an agent's page is made:
/// the window's own page when `drawn`, else one no pane draws, as an agent's out of sight.
fn made_in(
    mode: Responsive,
    drawn: bool,
    cx: &mut TestAppContext,
) -> (gpui_kit::AnyWindowHandle, gpui_kit::Entity<BrowserView>) {
    cx.update(crate::init);
    let make = move |window: &mut gpui_kit::Window, cx: &mut gpui_kit::App| {
        cx.new(|cx| {
            let url = "https://example.com/".to_owned();
            let mut view = BrowserView::new(url, Path::new("/work/app"), window, cx);
            view.start_in(mode);
            view
        })
    };
    if drawn {
        return in_window(size(px(1000.), px(800.)), cx, make);
    }
    let (window, _) = in_window(size(px(1000.), px(800.)), cx, |window, cx| {
        cx.new(|cx| BrowserView::new(String::new(), Path::new("/work/app"), window, cx))
    });
    let view = (cx.update_window(window, |_, window, cx| make(window, cx))).unwrap();
    (window, view)
}

fn desktop() -> Responsive {
    opened(None, Some((1920, 1080))).unwrap().unwrap()
}

#[gpui_kit::test]
fn a_page_opened_out_of_sight_opens_at_its_mode(cx: &mut TestAppContext) {
    let (_, view) = made_in(desktop(), false, cx);
    let scale = view.read_with(cx, |view, _| view.window_scale());
    view.read_with(cx, |view, _| {
        assert_eq!(view.viewport, None, "nothing is told before the page opens");
        assert_eq!(view.responsive.mode.as_ref().unwrap().set_by, agent());
    });
    view.update(cx, BrowserView::open_out_of_sight);
    let size = Viewport {
        width: 1920,
        height: 1080,
        scale,
    };
    view.read_with(cx, |view, _| assert_eq!(view.viewport, Some(size)));
}

#[gpui_kit::test]
fn a_page_made_in_its_mode_shows_at_its_size_scaled_once_drawn(cx: &mut TestAppContext) {
    let (window, view) = made_in(desktop(), true, cx);
    draw(window, cx);
    let at = view.read_with(cx, |view, _| {
        view.viewport
            .map(|viewport| (viewport.width, viewport.height))
    });
    assert_eq!(at, Some((1920, 1080)));
    assert!(
        view.read_with(cx, |view, _| view.shown_percent()) < 100,
        "scaled down to fit"
    );
}
