//! The app's side of devices against a stand-in host (`docs/devices.md` 13): no host process is
//! started under test (`host::program`), so what a host would say is played to the link.

use std::time::Duration;

use gpui_kit::{AppContext as _, TestAppContext};
use koyo_device_wire::{
    Answer, Ask, FromHost, Orientation, PixelSize, Refusal, RefusalCode, Screen, Size,
    State as Said, Step, ToHost,
};

use super::host::Running;
use super::link::{Link, State};

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
    }
}

/// A link with a stand-in host: what the app tells it comes out of the receiver.
fn running(cx: &mut TestAppContext) -> (gpui_kit::Entity<Link>, std::sync::mpsc::Receiver<ToHost>) {
    let (tell, told) = std::sync::mpsc::channel();
    let link = cx.new(|cx| Link::new("UDID-1".into(), "iPhone 17 Pro".into(), cx));
    link.update(cx, |link, cx| link.use_host(Running::stand_in(tell), cx));
    (link, told)
}

#[gpui_kit::test]
fn what_the_host_says_is_how_the_device_stands(cx: &mut TestAppContext) {
    let (link, told) = running(cx);
    assert_eq!(told.try_recv().ok(), Some(ToHost::Visible { visible: false }));
    link.update(cx, |link, cx| {
        link.hear(FromHost::State { state: Said::Starting { step: Step::Input } }, cx);
        assert!(!link.ready());
        link.hear(FromHost::State { state: Said::Ready { screen: screen() } }, cx);
        assert!(link.ready());
        assert_eq!(link.screen(), Some(screen()));
    });
}

#[gpui_kit::test]
fn an_ask_is_answered_by_its_number(cx: &mut TestAppContext) {
    let (link, told) = running(cx);
    let _ = told.try_recv();
    let task = link.update(cx, |link, cx| link.ask(Ask::Snapshot { text: false }, Duration::from_secs(5), cx));
    let Ok(ToHost::Request { request, ask }) = told.try_recv() else {
        panic!("no request");
    };
    assert_eq!(ask, Ask::Snapshot { text: false });
    link.update(cx, |link, cx| {
        link.hear(FromHost::Answer { request, result: Ok(Answer::Done) }, cx);
    });
    cx.run_until_parked();
    assert_eq!(futures::FutureExt::now_or_never(task), Some(Ok(Answer::Done)));
}

#[gpui_kit::test]
fn an_ask_that_runs_out_of_time_tells_the_host_to_drop_it(cx: &mut TestAppContext) {
    let (link, told) = running(cx);
    let _ = told.try_recv();
    let task = link.update(cx, |link, cx| link.ask(Ask::Apps, Duration::from_millis(50), cx));
    let Ok(ToHost::Request { request, .. }) = told.try_recv() else {
        panic!("no request");
    };
    cx.executor().advance_clock(Duration::from_millis(60));
    cx.run_until_parked();
    let answer = futures::FutureExt::now_or_never(task);
    assert!(matches!(answer, Some(Err(Refusal { code: RefusalCode::TimedOut, .. }))), "{answer:?}");
    assert_eq!(told.try_recv().ok(), Some(ToHost::Cancel { request }));
}

#[gpui_kit::test]
fn a_host_that_goes_answers_what_waited_and_is_started_again(cx: &mut TestAppContext) {
    let (link, told) = running(cx);
    let _ = told.try_recv();
    let task = link.update(cx, |link, cx| link.ask(Ask::Snapshot { text: false }, Duration::from_secs(5), cx));
    link.update(cx, Link::host_closed);
    cx.run_until_parked();
    let answer = futures::FutureExt::now_or_never(task);
    assert!(matches!(answer, Some(Err(Refusal { code: RefusalCode::Gone, .. }))), "{answer:?}");
    // Started again: under test there is no host to start, which it says.
    link.read_with(cx, |link, _| {
        assert!(matches!(&link.state, State::Failed(why) if why.contains("isn't started under test")), "{:?}", link.state);
    });
}

#[gpui_kit::test]
fn a_link_that_isnt_running_refuses_at_once(cx: &mut TestAppContext) {
    let link = cx.new(|cx| Link::new("UDID-2".into(), "iPad".into(), cx));
    let task = link.update(cx, |link, cx| link.ask(Ask::Snapshot { text: false }, Duration::from_secs(5), cx));
    let answer = futures::FutureExt::now_or_never(task);
    let Some(Err(Refusal { code, message })) = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(code, RefusalCode::Gone);
    assert_eq!(message, "iPad isn't running.");
}

#[gpui_kit::test]
fn the_person_s_input_reaches_a_ready_device_and_holds_agents_off(cx: &mut TestAppContext) {
    let (link, told) = running(cx);
    let _ = told.try_recv();
    link.update(cx, |link, cx| {
        let tap = koyo_device_wire::Input::Text { text: "a".into() };
        // Not ready yet: nothing goes, nothing is kept.
        link.input(tap.clone(), cx);
        link.hear(FromHost::State { state: Said::Ready { screen: screen() } }, cx);
        link.input(tap, cx);
        let agent = crate::browser::control::Actor { id: "a:1".into(), label: "Claude".into() };
        assert!(link.agent_acts(agent, cx).is_err());
    });
    let sent: Vec<ToHost> = told.try_iter().collect();
    assert_eq!(sent.len(), 1, "{sent:?}");
}

#[test]
fn a_record_whose_host_is_gone_is_a_leftover_and_one_that_runs_isnt() {
    let folder = crate::test_support::scratch("device-records");
    let me = std::process::id();
    // This test's own process: running, with its own start time.
    let started = super::host::started_at(i32::try_from(me).unwrap()).unwrap();
    std::fs::write(folder.join("ALIVE.host"), format!("{me} {started} ALIVE\n")).unwrap();
    std::fs::write(folder.join("REUSED.host"), format!("{me} 1 REUSED\n")).unwrap();
    std::fs::write(folder.join("GONE.host"), "999999 1 GONE\n").unwrap();
    std::fs::write(folder.join("GONE.log"), "not a record").unwrap();
    let mut left: Vec<String> = super::host::leftovers(&folder).into_iter().map(|(_, udid)| udid).collect();
    left.sort();
    assert_eq!(left, ["GONE", "REUSED"]);
    std::fs::remove_dir_all(&folder).unwrap();
}

#[gpui_kit::test]
fn a_device_in_two_tabs_stops_when_the_last_closes(cx: &mut TestAppContext) {
    let (link, told) = running(cx);
    let _ = told.try_recv();
    link.update(cx, |link, cx| {
        link.hear(FromHost::State { state: Said::Ready { screen: screen() } }, cx);
        link.shown_in_tab();
        link.shown_in_tab();
        link.tab_gone(cx);
        assert_eq!(link.state, State::Running);
    });
    assert!(told.try_recv().is_err(), "the host was told something when one of two tabs closed");
    link.update(cx, |link, cx| {
        link.tab_gone(cx);
        assert_eq!(link.state, State::Off);
    });
    assert_eq!(told.try_recv().ok(), Some(ToHost::Quit));
}

mod hand;

#[gpui_kit::test]
fn a_host_that_gave_up_isnt_started_again_and_its_word_stays(cx: &mut TestAppContext) {
    let (link, told) = running(cx);
    let _ = told.try_iter().count();
    let xcode = Said::Unavailable {
        code: koyo_device_wire::Missing::Xcode,
        why: "Koyo's devices need Xcode 27 or later.".into(),
    };
    link.update(cx, |link, cx| {
        link.hear(FromHost::State { state: xcode.clone() }, cx);
        link.host_closed(cx);
        assert_eq!(link.state, State::Off, "not started again");
        assert_eq!(link.said, Some(xcode.clone()), "what it said stays for the tab");
        assert!(link.not_running().is_some_and(|(why, person)| why.contains("Xcode 27") && person));
    });
}

#[gpui_kit::test]
fn start_again_ends_a_host_that_gave_up_and_starts_afresh(cx: &mut TestAppContext) {
    let (link, told) = running(cx);
    let _ = told.try_iter().count();
    let input = Said::Unavailable {
        code: koyo_device_wire::Missing::Input,
        why: "the simulator's input never answered".into(),
    };
    link.update(cx, |link, cx| {
        link.hear(FromHost::State { state: input }, cx);
        assert!(link.gave_up());
        link.start(cx);
    });
    assert_eq!(told.try_recv().ok(), Some(ToHost::Quit), "the host that gave up was ended");
    // Under test no host is started: the try shows as a failed start, not as the old state.
    let state = cx.read(|cx| link.read(cx).state.clone());
    assert!(matches!(state, State::Failed(_)), "{state:?}");
}
