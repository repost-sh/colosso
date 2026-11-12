//! The device tools through the pipeline (`docs/devices.md` 9), against a stand-in host: which
//! device a call finds, what is asked of the host, its answers and refusals in app control's
//! words, and the lease the person pre-empts.

use std::sync::mpsc::Receiver;

use gpui_kit::{Entity, TestAppContext};
use koyo_app_control::{ErrorCode, Scope};
use koyo_device_wire::{
    Act, Answer, Ask, Change, FromHost, Orientation, PixelSize, Refusal, RefusalCode, RequestId,
    Screen, Size, Source, Spot, ToHost,
};
use serde_json::{Value, json};

use super::{DENIAL, activity, allow, call, controlled, external, op, send, taken, terminal};
use crate::browser::control::Mode;
use crate::control::Caller;
use crate::control::activity::{ActivityKind, Decision};
use crate::devices::{Chosen, Link};
use crate::test_support::{repo, select};

fn screen() -> Screen {
    Screen {
        size: Size {
            width: 402.,
            height: 874.,
        },
        scale: 3.,
        pixels: PixelSize {
            width: 1206,
            height: 2622,
        },
        orientation: Orientation::Portrait,
        corner: 62.,
    }
}

/// A device tab of the simulator `udid`, shown, its host a stand-in standing ready.
fn device(udid: &str, rig: &super::Rig, cx: &mut TestAppContext) -> (Entity<Link>, Receiver<ToHost>) {
    opened(udid, false, rig, cx)
}

/// A device tab of `udid`, a real device if `phone`, shown, its host a stand-in standing ready.
fn opened(udid: &str, phone: bool, rig: &super::Rig, cx: &mut TestAppContext) -> (Entity<Link>, Receiver<ToHost>) {
    let chosen = Chosen {
        udid: udid.into(),
        name: format!("iPhone {udid}"),
        ipad: false,
        phone,
    };
    rig.shell.update(cx, |shell, cx| shell.device_pane(Some(chosen), true, cx));
    let link = cx.update(|cx| crate::devices::devices(cx).read(cx).existing(udid)).expect("its link");
    let told = link.update(cx, |link, cx| link.stand_in(screen(), cx));
    let _ = told.try_iter().count();
    (link, told)
}

/// The ask the host was sent last, with its number.
fn asked(told: &Receiver<ToHost>) -> (RequestId, Ask) {
    (told.try_iter())
        .filter_map(|message| match message {
            ToHost::Request { request, ask } => Some((request, ask)),
            _ => None,
        })
        .last()
        .expect("an ask")
}

/// Has the host answer `request` with `result`.
fn answer(link: &Entity<Link>, request: RequestId, result: Result<Answer, Refusal>, cx: &mut TestAppContext) {
    link.update(cx, |link, cx| link.hear(FromHost::Answer { request, result }, cx));
    cx.run_until_parked();
}

fn rig(name: &str, cx: &mut TestAppContext) -> super::Rig {
    let root = repo(name, &[("README.md", "hi\n")]);
    let rig = controlled(name, &[&root], cx);
    select(&rig.shell, &root, cx);
    rig
}

#[gpui_kit::test]
fn a_snapshot_and_a_tap_go_to_the_shown_device_and_answer_in_app_controls_words(cx: &mut TestAppContext) {
    let rig = rig("device-tools-shown", cx);
    let (link, told) = device("U-1", &rig, cx);
    let agent = terminal("t1");

    let pending = send(&agent, None, op("device_snapshot", json!({})), cx);
    cx.run_until_parked();
    let (request, ask) = asked(&told);
    assert_eq!(ask, Ask::Snapshot { text: false });
    let tree = "Button \"General\" [ref=e3] @201,319\n".to_owned();
    let snapshot = Answer::Snapshot {
        tree: tree.clone(),
        source: Source::Accessibility,
        elements: 1,
        truncated: false,
    };
    answer(&link, request, Ok(snapshot), cx);
    let got = taken(&pending).expect("answered").expect("a snapshot");
    assert_eq!((got["tree"].as_str(), got["source"].as_str()), (Some(tree.as_str()), Some("accessibility")));

    let pending = send(&agent, None, op("device_tap", json!({ "ref": "e3" })), cx);
    cx.run_until_parked();
    let (request, ask) = asked(&told);
    let tap = Act::Tap {
        spot: Spot::Ref("e3".into()),
        count: 1,
        hold_ms: 0,
        fingers: 1,
    };
    assert_eq!(ask, Ask::Act(tap));
    let acted = Answer::Acted {
        change: Change::Large,
        region: None,
        still: true,
        millis: 420,
    };
    answer(&link, request, Ok(acted), cx);
    let got = taken(&pending).expect("answered").expect("acted");
    assert_eq!((got["change"].as_str(), got["did"].as_str()), (Some("large"), Some("Tapped e3")));
    assert_eq!(got["name"], "iPhone U-1");
    let holder = cx.read(|cx| link.read(cx).control.holder.clone());
    assert!(matches!(holder, Some(crate::browser::control::Holder::Agent { .. })), "{holder:?}");
}

#[gpui_kit::test]
fn the_person_using_the_device_turns_an_agent_away_and_nothing_is_asked(cx: &mut TestAppContext) {
    let rig = rig("device-tools-person", cx);
    let (link, told) = device("U-2", &rig, cx);
    link.update(cx, |link, cx| link.person_acts(Mode::Implicit, cx));
    let error = call(&terminal("t1"), "device_tap", json!({ "x": 10, "y": 20 }), cx).unwrap_err();
    assert_eq!(error.code, ErrorCode::PersonHasControl);
    assert!(error.retryable, "{}", error.message);
    assert!(told.try_recv().is_err(), "the host was asked while the person held it");
    // Reads aren't acts: a screenshot is still asked.
    let _pending = send(&terminal("t1"), None, op("device_screenshot", json!({})), cx);
    cx.run_until_parked();
    assert_eq!(asked(&told).1, Ask::Screenshot { scale: 1 });
}

#[gpui_kit::test]
fn a_device_is_found_by_id_pane_or_the_last_acted_on_and_else_said_what_to_do(cx: &mut TestAppContext) {
    let rig = rig("device-tools-which", cx);
    let (one, told_one) = device("U-A", &rig, cx);
    let (_two, told_two) = device("U-B", &rig, cx);
    let agent = terminal("t1");
    // By id, though another tab shows.
    let pending = send(&agent, None, op("device_key", json!({ "device": "U-A", "combo": "escape" })), cx);
    cx.run_until_parked();
    let (request, _) = asked(&told_one);
    let acted = Answer::Acted {
        change: Change::Small,
        region: None,
        still: true,
        millis: 50,
    };
    answer(&one, request, Ok(acted), cx);
    assert!(taken(&pending).expect("answered").is_ok());
    // With neither: the one acted on last, not the shown tab's.
    let _pending = send(&agent, None, op("device_snapshot", json!({})), cx);
    cx.run_until_parked();
    assert_eq!(asked(&told_one).1, Ask::Snapshot { text: false });
    assert!(told_two.try_recv().is_err());
    // Another agent has acted on none: the shown tab's.
    let _pending = send(&terminal("t2"), None, op("device_snapshot", json!({})), cx);
    cx.run_until_parked();
    assert_eq!(asked(&told_two).1, Ask::Snapshot { text: false });
    let unknown = call(&agent, "device_snapshot", json!({ "device": "NOPE" }), cx).unwrap_err();
    assert_eq!(unknown.code, ErrorCode::NotFound);
    let both = call(&agent, "device_snapshot", json!({ "device": "U-A", "pane": 1 }), cx).unwrap_err();
    assert_eq!(both.code, ErrorCode::InvalidInput);
}

#[gpui_kit::test]
fn the_hosts_refusals_and_bad_input_read_as_what_to_do(cx: &mut TestAppContext) {
    let rig = rig("device-tools-refused", cx);
    let (link, told) = device("U-3", &rig, cx);
    let agent = terminal("t1");
    let pending = send(&agent, None, op("device_tap", json!({ "ref": "e99" })), cx);
    cx.run_until_parked();
    let (request, _) = asked(&told);
    let stale = Refusal::new(RefusalCode::NotFound, "No element e99: take a new snapshot.");
    answer(&link, request, Err(stale), cx);
    let error = taken(&pending).expect("answered").unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    assert!(error.message.contains("take a new snapshot"));

    let typed = call(&agent, "device_type", json!({ "text": "naïve" }), cx).unwrap_err();
    assert_eq!(typed.code, ErrorCode::InvalidInput);
    assert!(typed.message.contains("'ï'"), "{}", typed.message);
    let tap = call(&agent, "device_tap", json!({ "x": 1, "y": 2, "fingers": 6 }), cx).unwrap_err();
    assert_eq!(tap.code, ErrorCode::InvalidInput);
    assert!(told.try_recv().is_err(), "nothing was asked for bad input");
}

#[gpui_kit::test]
fn an_outside_client_needs_the_devices_scope_to_act_but_not_to_look(cx: &mut TestAppContext) {
    let rig = rig("device-tools-scope", cx);
    let (_link, told) = device("U-4", &rig, cx);
    allow("Outside", &[Scope::Read], cx);
    let outside = external("Outside");
    let denied = call(&outside, "device_tap", json!({ "x": 1, "y": 2 }), cx).unwrap_err();
    assert_eq!(denied.code, ErrorCode::ScopeDenied);
    let _pending = send(&outside, None, op("device_screenshot", json!({ "scale": 2 })), cx);
    cx.run_until_parked();
    assert_eq!(asked(&told).1, Ask::Screenshot { scale: 2 });
}

#[gpui_kit::test]
fn a_device_not_running_says_why_and_isnt_to_be_retried(cx: &mut TestAppContext) {
    let rig = rig("device-tools-not-running", cx);
    let (link, told) = device("U-5", &rig, cx);
    let xcode = koyo_device_wire::State::Unavailable {
        code: koyo_device_wire::Missing::Xcode,
        why: "Koyo's devices need Xcode 27 or later".into(),
    };
    link.update(cx, |link, cx| link.hear(FromHost::State { state: xcode }, cx));
    let needs = call(&terminal("t1"), "device_snapshot", json!({}), cx).unwrap_err();
    assert_eq!(needs.code, ErrorCode::DeviceNeedsPerson, "{}", needs.message);
    assert!(!needs.retryable);
    link.update(cx, Link::stop);
    let off = call(&terminal("t1"), "device_snapshot", json!({ "device": "U-5" }), cx).unwrap_err();
    assert_eq!(off.code, ErrorCode::DeviceUnavailable, "{}", off.message);
    assert!(!off.retryable, "{}", off.message);
    assert!(told.try_iter().all(|told| !matches!(told, ToHost::Request { .. })), "nothing was asked");
}

/// Whether nothing was asked of the host that told `told`.
fn nothing_asked(told: &Receiver<ToHost>) -> bool {
    told.try_iter().all(|told| !matches!(told, ToHost::Request { .. }))
}

#[gpui_kit::test]
fn a_real_device_is_used_once_the_person_lends_it_and_a_call_naming_none_never_finds_it(cx: &mut TestAppContext) {
    let rig = rig("device-tools-lend", cx);
    let (_sim, told_sim) = device("U-SIM", &rig, cx);
    let (_ipad, told_ipad) = opened("U-REAL", true, &rig, cx);
    let (agent, other) = (terminal("t1"), terminal("t2"));
    let code = |caller: &Caller, tool: &str, input: Value, cx: &mut TestAppContext| call(caller, tool, input, cx).map(|_| ()).unwrap_err().code;
    let snapshot = || json!({ "device": "U-REAL" });
    // Its tab shown, a call naming no device goes to the simulator.
    let _pending = send(&agent, None, op("device_snapshot", json!({})), cx);
    cx.run_until_parked();
    assert_eq!(asked(&told_sim).1, Ask::Snapshot { text: false });

    // What it refuses anyway is refused without asking the person.
    let launch = json!({ "device": "U-REAL", "bundle": "com.apple.mobiletimer" });
    assert_eq!(code(&agent, "device_launch", launch, cx), ErrorCode::Unsupported);
    assert!(!cx.has_pending_prompt());

    // A read puts the sheet up and answers at once; while it's up, nobody's call asks again.
    let asking = call(&agent, "device_screenshot", snapshot(), cx).unwrap_err();
    assert_eq!(asking.code, ErrorCode::DeviceNeedsPerson, "{}", asking.message);
    assert!(asking.message.contains("Tell them why you need it"), "{}", asking.message);
    assert!(cx.has_pending_prompt());
    let tap = json!({ "device": "U-REAL", "x": 1, "y": 2 });
    assert_eq!(code(&agent, "device_tap", tap, cx), ErrorCode::DeviceNeedsPerson);
    let waiting = call(&other, "device_snapshot", snapshot(), cx).unwrap_err();
    assert!(waiting.message.contains("another agent"), "{}", waiting.message);
    cx.simulate_prompt_answer("Allow");
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt(), "one sheet, answered");
    assert!(nothing_asked(&told_ipad), "nothing was asked of it before Allow");

    // Lent: its calls go to the device, and the decision is in the activity log.
    let _pending = send(&agent, None, op("device_snapshot", snapshot()), cx);
    cx.run_until_parked();
    assert_eq!(asked(&told_ipad).1, Ask::Snapshot { text: false });
    let lent = activity(cx).into_iter().any(|entry| {
        matches!(entry.kind, ActivityKind::Confirmation { ref operation, decision: Decision::Allowed, .. } if operation == "device_lend")
    });
    assert!(lent);

    // Another agent: Don't Allow holds for a while with no new sheet, then it may ask again.
    assert_eq!(code(&other, "device_snapshot", snapshot(), cx), ErrorCode::DeviceNeedsPerson);
    cx.simulate_prompt_answer("Don't Allow");
    cx.run_until_parked();
    assert_eq!(code(&other, "device_snapshot", snapshot(), cx), ErrorCode::UserDeclined);
    assert!(!cx.has_pending_prompt(), "no new sheet meanwhile");
    cx.executor().advance_clock(DENIAL);
    assert_eq!(code(&other, "device_snapshot", snapshot(), cx), ErrorCode::DeviceNeedsPerson);
    cx.simulate_prompt_answer("Don't Allow");
    cx.run_until_parked();

    // Taken back: the agent's next call asks the person again.
    cx.update(|cx| crate::devices::devices(cx).update(cx, |devices, cx| devices.take_back("U-REAL", cx)));
    assert_eq!(code(&agent, "device_snapshot", snapshot(), cx), ErrorCode::DeviceNeedsPerson);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Don't Allow");
    cx.run_until_parked();

    // Lent ahead to every agent; closing its tab ends that.
    cx.update(|cx| crate::devices::devices(cx).update(cx, |devices, _| devices.lends.lend("U-REAL", None)));
    let closed = call(&agent, "device_close", snapshot(), cx);
    assert!(closed.is_ok(), "{closed:?}");
    cx.run_until_parked();
    let lent = cx.read(|cx| crate::devices::devices(cx).read(cx).lends.is_lent("U-REAL"));
    assert!(!lent, "the lend ended with its tab");
}
