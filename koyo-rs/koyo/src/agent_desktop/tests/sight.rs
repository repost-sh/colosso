//! The host sends the desktop's frames while a tab draws it in a window a person can see:
//! covered, minimized or on another desktop, the window draws nothing, so the host is told the
//! desktop isn't seen, and that it is once the window is seen again.

use std::sync::mpsc::Receiver;

use gpui_kit::{AppContext as _, TestAppContext, px, size};
use koyo_desk_wire::Size;
use koyo_desk_wire::link::{DesktopState, FromHost, ToHost};

use super::super::view::DeskView;
use super::super::TheDesk;
use super::{running, says};
use crate::test_support::{draw, in_window, on_screen};

/// Whether the host was last told the desktop is seen, of what it's been told since it was last
/// asked.
fn told_seen(told: &Receiver<ToHost>) -> Option<bool> {
    (told.try_iter())
        .filter_map(|message| match message {
            ToHost::Visible { visible } => Some(visible),
            _ => None,
        })
        .last()
}

#[gpui_kit::test]
fn the_host_is_told_the_desktop_isnt_seen_while_its_window_is_unseen(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(crate::test_support::init);
    let (desk, told) = running(cx);
    cx.update(|cx| cx.set_global(TheDesk(desk.clone())));
    let ready = DesktopState::Ready {
        size: Size {
            width: 1800.,
            height: 1169.,
        },
        scale: 2.,
    };
    says(&desk, FromHost::Desktop { state: ready }, cx);
    let (window, _) = in_window(size(px(900.), px(600.)), cx, |_, cx| cx.new(DeskView::new));
    draw(window, cx);
    assert_eq!(told_seen(&told), Some(true), "drawn, it's seen");

    on_screen(window, false, cx);
    assert_eq!(told_seen(&told), Some(false), "in a window nobody sees");
    on_screen(window, true, cx);
    assert_eq!(told_seen(&told), Some(true), "the window seen again");
}
