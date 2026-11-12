//! A real iPhone or iPad in its tab: never counted as a simulator, what its tab says in each of
//! its states and the way on from each, and its start once the step it missed is done.

use std::time::Instant;

use gpui_kit::{Entity, TestAppContext};
use koyo_device_wire::{FromHost, Missing, State as Said};

use super::super::connected::Connected;
use super::super::view::{Chosen, DeviceView};
use super::super::{Link, Standing, devices};
use super::screen;
use crate::test_support::{draw, repo, select, shell_with};

fn ipad() -> Chosen {
    Chosen {
        udid: "IPAD".into(),
        name: "Sam's iPad".into(),
        ipad: true,
        phone: true,
    }
}

/// The iPad's tab, shown, its host a stand-in standing ready.
fn tab(name: &str, cx: &mut TestAppContext) -> (gpui_kit::AnyWindowHandle, Entity<DeviceView>, Entity<Link>) {
    let root = repo(name, &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    shell.update(cx, |shell, cx| shell.device_pane(Some(ipad()), true, cx));
    let view = cx.read(|cx| shell.read(cx).shown_device_view(cx)).expect("its tab");
    let link = cx.read(|cx| view.read(cx).link().cloned()).expect("its link");
    let _told = link.update(cx, |link, cx| link.stand_in(screen(), cx));
    (window, view, link)
}

fn says(link: &Entity<Link>, state: Said, cx: &mut TestAppContext) {
    link.update(cx, |link, cx| link.hear(FromHost::State { state }, cx));
    cx.run_until_parked();
}

/// What the tab says, and how many buttons it offers.
fn notice(view: &Entity<DeviceView>, cx: &mut TestAppContext) -> Option<(String, usize)> {
    view.update(cx, |view, cx| view.notice(cx)).map(|notice| (notice.message, notice.buttons.len()))
}

fn unavailable(code: Missing, why: &str) -> Said {
    Said::Unavailable { code, why: why.into() }
}

#[gpui_kit::test]
fn a_real_devices_tab_says_what_it_needs_and_isnt_counted_as_a_simulator(cx: &mut TestAppContext) {
    let (window, view, link) = tab("device-phone-states", cx);
    assert!(cx.read(|cx| link.read(cx).running_or_starting()));
    assert!(!cx.read(|cx| link.read(cx).holds_simulator()), "no share of the simulators' limit");
    assert_eq!(notice(&view, cx), None, "ready: the picture");

    says(&link, Said::Locked { screen: screen() }, cx);
    assert_eq!(notice(&view, cx), Some(("Unlock your iPad to continue.".into(), 0)), "it moves on by itself");

    let unplugged = "Sam's iPad was unplugged. Plug it back in to continue.";
    says(&link, Said::Gone { why: unplugged.into() }, cx);
    assert_eq!(notice(&view, cx), Some((unplugged.into(), 1)), "back by itself, or Start Again");

    let off = "Developer Mode is off on Sam's iPad. Turn it on in Settings → Privacy & Security → Developer Mode.";
    says(&link, unavailable(Missing::DeveloperMode, off), cx);
    assert_eq!(notice(&view, cx), Some((off.into(), 1)), "Set Up…");

    let old = "Sam's iPad runs iPadOS 26.6, and Koyo needs iPadOS 27 or later.";
    says(&link, unavailable(Missing::System, old), cx);
    assert_eq!(notice(&view, cx), Some((old.into(), 0)), "an update is the person's, on the device");

    let held = "Another app is showing Sam's iPad.";
    says(&link, unavailable(Missing::InUse, held), cx);
    assert_eq!(notice(&view, cx), Some((held.into(), 1)), "Start Again");
    draw(window, cx);
}

fn listed(missing: Option<Missing>) -> Vec<Connected> {
    vec![Connected {
        udid: "IPAD".into(),
        name: "Sam's iPad".into(),
        ipad: true,
        system: "iPadOS 27.0.1".into(),
        missing: missing.map(|code| (code, "a step".into())),
    }]
}

#[gpui_kit::test]
fn a_device_starts_again_by_itself_once_listed_ready_after_it_ended_three_times_at_most(cx: &mut TestAppContext) {
    let (_window, _view, link) = tab("device-phone-steps", cx);
    let registry = cx.update(|cx| devices(cx));
    let ended = |cx: &mut TestAppContext| cx.read(|cx| matches!(link.read(cx).standing(), Standing::Ended { .. }));
    let afresh = |missing, since, cx: &mut TestAppContext| {
        registry.update(cx, |registry, cx| registry.connected_afresh(Ok(listed(missing)), since, cx));
        cx.run_until_parked();
    };
    let begun = Instant::now();
    says(&link, unavailable(Missing::DeveloperMode, "Developer Mode is off."), cx);
    afresh(None, begun, cx);
    assert!(ended(cx), "a listing begun before it ended doesn't count");
    afresh(Some(Missing::DeveloperMode), Instant::now(), cx);
    assert!(ended(cx), "still missing: it waits");
    afresh(None, Instant::now(), cx);
    assert!(!ended(cx), "the step done: started again");

    // A host that disagrees with the list is started three times, then left for the person.
    for _ in 0..2 {
        says(&link, unavailable(Missing::DeveloperMode, "Developer Mode is off."), cx);
        afresh(None, Instant::now(), cx);
        assert!(!ended(cx));
    }
    says(&link, unavailable(Missing::DeveloperMode, "Developer Mode is off."), cx);
    afresh(None, Instant::now(), cx);
    assert!(ended(cx), "three times at most");

    // The person's start counts afresh; what the list can't see (another app holding the
    // device) isn't started by it.
    link.update(cx, Link::start);
    says(&link, unavailable(Missing::InUse, "Another app is showing it."), cx);
    afresh(None, Instant::now(), cx);
    assert!(ended(cx), "in use isn't the list's to see");
}

#[gpui_kit::test]
fn a_device_the_list_says_misses_a_step_isnt_started(cx: &mut TestAppContext) {
    let root = repo("device-phone-held-back", &[("README.md", "hi\n")]);
    let (_window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let registry = cx.update(|cx| devices(cx));
    registry.update(cx, |registry, cx| registry.connected_afresh(Ok(listed(Some(Missing::System))), Instant::now(), cx));
    shell.update(cx, |shell, cx| shell.device_pane(Some(ipad()), true, cx));
    let view = cx.read(|cx| shell.read(cx).shown_device_view(cx)).expect("its tab");
    let link = cx.read(|cx| view.read(cx).link().cloned()).expect("its link");
    link.update(cx, Link::start);
    cx.run_until_parked();
    assert!(!cx.read(|cx| link.read(cx).running_or_starting()), "no host for it: nothing is sent to it");
    assert_eq!(notice(&view, cx), Some(("a step".into(), 0)), "the list's words; an update is the person's");
    // Once the list says nothing is missing it starts by itself: the test above.
}
