//! The setup for simulators: offered once while something is missing, by the person's own
//! tabs only, and its steps read from what the Mac has.

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use koyo_device_wire::{FromHost, Missing, State as Said, Step as Starting};

use super::super::needs::fixtures::{found, ready, simulator};
use super::super::needs::{Found, Need, Xcode};
use super::super::setup::{self, Setup, Step};
use super::super::view::{Chosen, DeviceView};
use super::super::{Devices, Simulator, devices};
use super::screen;
use crate::browser::control::Actor;
use crate::modal;
use crate::test_support::{draw, repo, select, shell_with};

fn shows(found: Found, simulators: Vec<Simulator>, cx: &mut TestAppContext) {
    let registry: Entity<Devices> = cx.update(|cx| devices(cx));
    registry.update(cx, |registry, cx| registry.listed(Ok(simulators), Some(found), cx));
    cx.run_until_parked();
}

#[gpui_kit::test]
fn the_setup_comes_up_by_itself_once_while_something_is_missing(cx: &mut TestAppContext) {
    let root = repo("device-setup-due", &[("README.md", "hi\n")]);
    let (_window, _shell) = shell_with(&[&root], cx);
    let registry = cx.update(|cx| devices(cx));
    // Unknown yet: nothing is offered.
    assert!(!cx.read(|cx| registry.read(cx).setup_due()));
    shows(found(Xcode::Older("26.4".into()), false, false), Vec::new(), cx);
    assert_eq!(cx.read(|cx| registry.read(cx).need()), Some(Need::Xcode));
    assert!(registry.update(cx, |registry, _| registry.offers_setup()));
    assert!(!registry.update(cx, |registry, _| registry.offers_setup()), "offered once");
    shows(found(ready(), true, true), Vec::new(), cx);
    assert_eq!(cx.read(|cx| registry.read(cx).need()), Some(Need::Device));
    shows(found(ready(), true, true), vec![simulator("U-SETUP")], cx);
    assert_eq!(cx.read(|cx| registry.read(cx).need()), None);
}

#[gpui_kit::test]
fn the_picker_and_the_persons_own_tab_offer_the_setup_and_an_agents_doesnt(cx: &mut TestAppContext) {
    let root = repo("device-setup-offer", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    shell.update(cx, |shell, cx| shell.device_pane(None, false, true, cx));
    let picker: Entity<DeviceView> = cx.read(|cx| shell.read(cx).shown_device_view(cx)).expect("the picker");
    assert!(picker.update(cx, |view, cx| view.may_offer_setup(cx)));
    let chosen = Chosen {
        udid: "U-OFFER".into(),
        name: "iPhone 18 Pro".into(),
        ipad: false,
    };
    picker.update(cx, |view, cx| view.choose(chosen, false, cx));
    let link = cx.read(|cx| picker.read(cx).link().cloned()).expect("its link");
    let _told = link.update(cx, |link, cx| link.stand_in(screen(), cx));
    let xcode = Said::Unavailable {
        code: Missing::Xcode,
        why: "Koyo's devices need Xcode 27 or later.".into(),
    };
    link.update(cx, |link, cx| link.hear(FromHost::State { state: xcode }, cx));
    assert!(picker.update(cx, |view, cx| view.may_offer_setup(cx)), "the person's tab");
    let agent = Actor {
        id: "chat:1".into(),
        label: "Claude".into(),
    };
    let _ = link.update(cx, |link, cx| link.agent_acts(agent, cx));
    assert!(!picker.update(cx, |view, cx| view.may_offer_setup(cx)), "an agent's tab");
    draw(window, cx);
}

#[gpui_kit::test]
fn the_setups_steps_follow_what_the_mac_has_and_the_device_it_made(cx: &mut TestAppContext) {
    let root = repo("device-setup-steps", &[("README.md", "hi\n")]);
    let (window, _shell) = shell_with(&[&root], cx);
    shows(found(Xcode::Missing, false, false), Vec::new(), cx);
    cx.update_window(window, |_, window, cx| setup::open(window, cx)).expect("the window");
    cx.run_until_parked();
    let dialog: Entity<Setup> = cx.read(modal::current::<Setup>).expect("the setup open");
    let step = |cx: &mut TestAppContext| cx.read(|cx| dialog.read(cx).step(cx));
    assert_eq!(step(cx), Step::Needs(Need::Xcode));
    shows(found(ready(), false, false), Vec::new(), cx);
    assert_eq!(step(cx), Step::Needs(Need::Components));
    shows(found(ready(), true, false), Vec::new(), cx);
    assert_eq!(step(cx), Step::Needs(Need::System));
    shows(found(ready(), true, true), Vec::new(), cx);
    assert_eq!(step(cx), Step::Needs(Need::Device));
    // The device made: its first start, then ready.
    let link = cx.update(|cx| devices(cx).update(cx, |registry, cx| registry.link("U-SETUP", "iPhone 18 Pro", cx)));
    let _told = link.update(cx, |link, cx| link.stand_in(screen(), cx));
    let starting = Said::Starting {
        step: Starting::Booting { first: true },
    };
    link.update(cx, |link, cx| link.hear(FromHost::State { state: starting }, cx));
    dialog.update(cx, |dialog, cx| dialog.starts(link.clone(), cx));
    assert_eq!(step(cx), Step::Starting);
    link.update(cx, |link, cx| link.hear(FromHost::State { state: Said::Ready { screen: screen() } }, cx));
    assert_eq!(step(cx), Step::Ready);
}
