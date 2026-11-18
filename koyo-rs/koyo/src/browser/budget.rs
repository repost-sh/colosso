//! What a page's news costs the window (`crate::budget`): what its profile's extensions set
//! their icons to, and its address copied. Each is held at what it costs: the views that show
//! it built again, and no others.

use std::time::Duration;

use gpui_kit::{Modifiers, TestAppContext, VisualTestContext, point, px};
use koyo_browser_ipc::client::Incoming;
use koyo_browser_ipc::{HostEvent, PaneId};

use super::extensions::{extension, set};
use super::host::Browsers;
use crate::budget::{Built, built_after, built_during, ratchet, settle, within};
use crate::model::Edge;
use crate::settings::Layout;
use crate::test_support::{center_of, draw, hover, open_page, open_page_view, shell_on};

/// What a change to one page builds: its view, and one view more that draws from it.
const ONE_PAGE: Built = Built {
    shell: 0,
    views: 2,
    frames: 1,
};

fn heard(event: HostEvent) -> impl Fn(&mut TestAppContext) {
    move |cx| {
        let browsers = cx.update(Browsers::global);
        browsers.update(cx, |browsers, cx| browsers.receive(Incoming::Event(event.clone()), cx));
    }
}

/// `profile`'s one extension, pinned, named `name`.
fn listed(profile: &str, name: &str) -> HostEvent {
    HostEvent::Extensions {
        profile: profile.into(),
        extensions: vec![extension("extension", name, true, true)],
    }
}

/// The extension of `profile` set its badge to `badge` for `pane`'s page, or every page.
fn acted(profile: &str, pane: Option<PaneId>, badge: &str) -> HostEvent {
    HostEvent::ExtensionAction {
        profile: profile.into(),
        extension: "extension".into(),
        pane,
        action: set(badge, "", true),
    }
}

/// In the Arc layout the extensions' icons are in the sidebar's address bar, and a page's view
/// reads the engine's state. Two pages of a profile side by side: what the host says of the
/// profile's extensions builds those two and the bar again when it changes them, and nothing
/// when it says again what it said, as it does of every page; what it sets for one page builds
/// that page's view and the bar, never the other page's.
#[gpui_kit::test]
fn what_the_host_says_of_extensions_builds_what_shows_them(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-extensions", Layout::Arc, cx);
    let first = open_page(window, &shell, Some("https://example.com/"), cx);
    let second = open_page(window, &shell, Some("https://example.org/"), cx);
    shell.update(cx, |shell, cx| {
        let unit = shell.workspace.branch_mut().expect("a checkout is selected");
        assert!(unit.move_pane(second, first, Edge::Right), "the pages side by side");
        cx.notify();
    });
    draw(window, cx);
    let (a, profile) = shell.read_with(cx, |shell, cx| {
        let pane = shell.workspace.pane(first).expect("the first page's pane");
        let view = pane.view().expect("its page").read(cx);
        (view.pane, view.profile().expect("a page's profile").to_owned())
    });
    built_after(window, cx, heard(listed(&profile, "One")));
    built_after(window, cx, heard(acted(&profile, None, "1")));

    let both = Built {
        shell: 0,
        views: 3,
        frames: 1,
    };
    ratchet(built_after(window, cx, heard(listed(&profile, "One"))), Built::default(), "the same list");
    within(built_after(window, cx, heard(listed(&profile, "Two"))), both, "a new list");
    ratchet(built_after(window, cx, heard(acted(&profile, None, "1"))), Built::default(), "the same action");
    within(built_after(window, cx, heard(acted(&profile, None, "2"))), both, "a new action");
    ratchet(built_after(window, cx, heard(acted(&profile, Some(a), "3"))), ONE_PAGE, "a new action for one page");
}

/// The page's address copied lights the copy-link button in its address field for a while,
/// which builds the page's view again as it goes out. The press itself builds the window over
/// twice, as GPUI draws it all again as a button with a click of its own is pressed and let go.
#[gpui_kit::test]
fn the_address_copied_builds_the_page_again_as_it_lights_and_goes_out(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-copy-link", Layout::Orca, cx);
    open_page_view(window, &shell, "https://example.com/", cx);
    let button = center_of(window, "site-copy-link", cx);
    hover(window, button + point(px(1.), px(0.)), cx);
    settle(cx);

    let pressed = Built {
        shell: 2,
        views: 24,
        frames: 2,
    };
    let press = |cx: &mut TestAppContext| {
        let at = center_of(window, "site-copy-link", cx);
        VisualTestContext::from_window(window, cx).simulate_click(at, Modifiers::default());
    };
    ratchet(built_after(window, cx, press), pressed, "the address copied");
    let built = built_during(window, cx, Duration::from_secs(2));
    within(built, ONE_PAGE, "the copy-link button going out");
}
