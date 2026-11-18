//! The host keeps a device's picture current while a tab draws it in a window a person can see:
//! covered, minimized or on another desktop, the window draws nothing, so the host is told the
//! device isn't seen, and that it is once the window is seen again.

use std::sync::mpsc::Receiver;

use gpui_kit::TestAppContext;
use koyo_device_wire::ToHost;

use super::screen;
use crate::test_support::{draw, iphone, on_screen, repo, select, shell_with};

/// Whether the host was last told the device is seen, of what it's been told since it was last
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
fn the_host_is_told_the_device_isnt_seen_while_its_window_is_unseen(cx: &mut TestAppContext) {
    let root = repo("device-sight", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    shell.update(cx, |shell, cx| shell.device_pane(Some(iphone("PHONE")), true, cx));
    let view = cx.read(|cx| shell.read(cx).shown_device_view(cx)).expect("its tab");
    let link = cx.read(|cx| view.read(cx).link().cloned()).expect("its link");
    let told = link.update(cx, |link, cx| link.stand_in(screen(), cx));
    draw(window, cx);
    assert_eq!(told_seen(&told), Some(true), "drawn, it's seen");

    on_screen(window, false, cx);
    assert_eq!(told_seen(&told), Some(false), "in a window nobody sees");
    on_screen(window, true, cx);
    assert_eq!(told_seen(&told), Some(true), "the window seen again");
}
