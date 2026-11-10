//! The desk against a host that isn't one: what the host says is kept, asks find their
//! answers or their end, and the host hears what a tab and the person do.

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use futures::FutureExt as _;
use gpui_kit::{AppContext as _, Entity, EntityId, Task, TestAppContext};
use koyo_desk_wire::link::{
    Answer, Ask, Button, DesktopState, FromHost, Input, Lent, PROTOCOL, RefusalCode, ToHost, Window,
};
use koyo_desk_wire::{Point, Rect, Size};

use super::control::RESTS_AFTER;
use super::granting::{Grant, Step};
use super::host::{Incoming, Running};
use super::{Desk, State};
use crate::browser::control::{Actor, IMPLICIT_HOLD, Mode, Refused};

/// A desk whose host stands in: what it's told comes out of the channel.
fn running(cx: &mut TestAppContext) -> (Entity<Desk>, Receiver<ToHost>) {
    let (tell, told) = channel();
    let desk = cx.new(Desk::new);
    desk.update(cx, |desk, cx| {
        desk.state = State::Starting;
        assert!(desk.started(desk.starts, Ok(Running::stand_in(tell)), cx));
    });
    assert_eq!(told.try_recv(), Ok(ToHost::Visible { visible: false }));
    (desk, told)
}

/// What a task came to, which it has by now.
fn done<T: 'static>(task: Task<T>, cx: &mut TestAppContext) -> T {
    cx.run_until_parked();
    task.now_or_never().expect("the ask still waits")
}

fn says(desk: &Entity<Desk>, said: FromHost, cx: &mut TestAppContext) {
    desk.update(cx, |desk, cx| desk.receive(Incoming::Said(said), cx));
}

fn window(id: u32) -> Window {
    Window {
        id,
        pid: 700,
        app: "TextEdit".into(),
        title: format!("Untitled {id}"),
        frame: Rect {
            x: 10.,
            y: 20.,
            width: 600.,
            height: 400.,
        },
        layer: 0,
    }
}

#[gpui_kit::test]
fn what_the_host_says_is_how_the_desk_stands(cx: &mut TestAppContext) {
    let (desk, _told) = running(cx);
    says(
        &desk,
        FromHost::Hello {
            protocol: PROTOCOL,
            pid: 77,
            accessibility: true,
            screen_recording: false,
        },
        cx,
    );
    let size = Size {
        width: 1800.,
        height: 1169.,
    };
    let state = DesktopState::Ready { size, scale: 2. };
    says(&desk, FromHost::Desktop { state }, cx);
    let windows = vec![window(4), window(5)];
    let backdrop = Some(window(9));
    says(&desk, FromHost::Windows { windows, backdrop }, cx);
    desk.read_with(cx, |desk, _| {
        assert_eq!(desk.state, State::Running);
        let grants = desk.grants.unwrap();
        assert!(grants.accessibility && !grants.screen_recording);
        assert_eq!(desk.size(), Some(size));
        assert_eq!(desk.windows.len(), 2);
    });
    // A host of another version of the link isn't talked to.
    says(
        &desk,
        FromHost::Hello {
            protocol: PROTOCOL + 1,
            pid: 77,
            accessibility: true,
            screen_recording: true,
        },
        cx,
    );
    desk.read_with(cx, |desk, _| {
        assert!(matches!(desk.state, State::Failed(_)));
    });
}

#[gpui_kit::test]
fn an_ask_gets_the_answer_with_its_number(cx: &mut TestAppContext) {
    let (desk, told) = running(cx);
    let limit = Duration::from_secs(30);
    let first = desk.update(cx, |desk, cx| desk.ask(Ask::Windows, limit, cx));
    let wait = Ask::Wait {
        quiet_ms: 500,
        timeout_ms: 1000,
    };
    let second = desk.update(cx, |desk, cx| desk.ask(wait, limit, cx));
    let numbers: Vec<u64> = (told.try_iter())
        .map(|message| match message {
            ToHost::Request { request, .. } => request,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(numbers.len(), 2);
    assert_ne!(numbers[0], numbers[1]);
    // Answered out of order, each ask gets its own.
    let waited = Answer::Waited {
        still: true,
        millis: 510,
    };
    let answer = |request, result| FromHost::Answer { request, result };
    says(&desk, answer(numbers[1], Ok(waited.clone())), cx);
    says(&desk, answer(numbers[0], Ok(Answer::Done)), cx);
    cx.run_until_parked();
    assert_eq!(done(first, cx), Ok(Answer::Done));
    assert_eq!(done(second, cx), Ok(waited));
}

#[gpui_kit::test]
fn an_ask_nobody_answers_ends_in_time_and_the_host_is_told_to_drop_it(cx: &mut TestAppContext) {
    let (desk, told) = running(cx);
    let asked = desk.update(cx, |desk, cx| {
        desk.ask(Ask::Windows, Duration::from_secs(5), cx)
    });
    let Ok(ToHost::Request { request, .. }) = told.try_recv() else {
        panic!("nothing was asked");
    };
    cx.executor().advance_clock(Duration::from_secs(6));
    cx.run_until_parked();
    let refusal = done(asked, cx).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::TimedOut);
    assert_eq!(told.try_recv(), Ok(ToHost::Cancel { request }));
}

#[gpui_kit::test]
fn with_the_host_gone_what_waited_and_what_comes_is_refused(cx: &mut TestAppContext) {
    let (desk, _told) = running(cx);
    let limit = Duration::from_secs(30);
    let waiting = desk.update(cx, |desk, cx| desk.ask(Ask::Windows, limit, cx));
    says(
        &desk,
        FromHost::Windows {
            windows: vec![window(4)],
            backdrop: None,
        },
        cx,
    );
    desk.update(cx, |desk, cx| desk.receive(Incoming::Closed, cx));
    cx.run_until_parked();
    let refusal = done(waiting, cx).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::NoDesktop);
    desk.read_with(cx, |desk, _| {
        assert!(matches!(desk.state, State::Failed(_)));
        assert!(desk.windows.is_empty() && desk.grants.is_none());
    });
    let after = desk.update(cx, |desk, cx| desk.ask(Ask::Windows, limit, cx));
    let refusal = done(after, cx).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::NoDesktop);
}

#[gpui_kit::test]
fn turned_off_an_ask_is_refused_with_the_way_to_turn_it_on(cx: &mut TestAppContext) {
    let desk = cx.new(Desk::new);
    let asked = desk.update(cx, |desk, cx| {
        desk.ask(Ask::Windows, Duration::from_secs(5), cx)
    });
    let refusal = done(asked, cx).unwrap_err();
    assert_eq!(refusal.code, RefusalCode::NoDesktop);
    assert!(refusal.message.contains("Settings"), "{}", refusal.message);
}

#[gpui_kit::test]
fn what_is_asked_while_the_host_starts_is_asked_once_it_runs(cx: &mut TestAppContext) {
    let desk = cx.new(Desk::new);
    let (tell, told) = channel();
    let tab = EntityId::from(7u64);
    let _asked = desk.update(cx, |desk, cx| {
        desk.state = State::Starting;
        desk.set_shown_by(tab, true);
        desk.ask(Ask::Windows, Duration::from_secs(30), cx)
    });
    assert!(told.try_recv().is_err());
    desk.update(cx, |desk, cx| {
        assert!(desk.started(desk.starts, Ok(Running::stand_in(tell)), cx));
    });
    let told: Vec<ToHost> = told.try_iter().collect();
    // The tab was drawn before the host ran: it's told once, as it is now.
    assert_eq!(told.first(), Some(&ToHost::Visible { visible: true }));
    assert!(
        told.iter()
            .any(|message| matches!(message, ToHost::Request { .. }))
    );
}

#[gpui_kit::test]
fn the_host_is_told_whether_a_tab_draws_the_desktop(cx: &mut TestAppContext) {
    let (desk, told) = running(cx);
    let (one, other) = (EntityId::from(1u64), EntityId::from(2u64));
    desk.update(cx, |desk, _| {
        desk.set_shown_by(one, true);
        desk.set_shown_by(other, true);
        desk.set_shown_by(one, false);
    });
    assert_eq!(
        told.try_iter().collect::<Vec<_>>(),
        [ToHost::Visible { visible: true }]
    );
    desk.update(cx, |desk, _| desk.set_shown_by(other, false));
    assert_eq!(told.try_recv(), Ok(ToHost::Visible { visible: false }));
}

#[gpui_kit::test]
fn the_persons_input_goes_to_the_host_and_holds_agents_off(cx: &mut TestAppContext) {
    let (desk, told) = running(cx);
    assert!(!desk.read_with(cx, |desk, _| desk.person_holds()));
    let input = Input::Down {
        at: Point { x: 40., y: 60. },
        button: Button::Left,
        clicks: 1,
        flags: 0,
    };
    desk.update(cx, |desk, cx| desk.input(input.clone(), cx));
    assert_eq!(told.try_recv(), Ok(ToHost::Input { input }));
    assert!(desk.read_with(cx, |desk, _| desk.person_holds()));
}

#[gpui_kit::test]
fn turned_off_the_host_is_told_to_quit(cx: &mut TestAppContext) {
    let (desk, told) = running(cx);
    desk.update(cx, Desk::stop);
    assert_eq!(told.try_recv(), Ok(ToHost::Quit));
    desk.read_with(cx, |desk, _| assert_eq!(desk.state, State::Off));
}

#[gpui_kit::test]
fn a_start_overtaken_has_its_host_told_to_quit_and_changes_nothing(cx: &mut TestAppContext) {
    let (desk, told) = running(cx);
    let (late, told_late) = channel();
    desk.update(cx, |desk, cx| {
        // A start numbered before the one that counts comes to its end only now.
        desk.starts += 1;
        let stale = desk.starts - 1;
        assert!(!desk.started(stale, Ok(Running::stand_in(late)), cx));
        assert_eq!(desk.state, State::Running);
    });
    assert_eq!(told_late.try_recv(), Ok(ToHost::Quit));
    assert!(
        told.try_recv().is_err(),
        "the host that runs heard nothing of it"
    );
}

#[gpui_kit::test]
fn an_app_the_person_asked_for_is_offered_and_started_only_on_their_word(cx: &mut TestAppContext) {
    let (desk, told) = running(cx);
    let wanted = FromHost::Wanted {
        app: "Calculator".into(),
        bundle: "com.apple.calculator".into(),
    };
    says(&desk, wanted, cx);
    assert!(told.try_recv().is_err(), "nothing is started on a guess");
    desk.read_with(cx, |desk, _| {
        assert_eq!(
            desk.wanted.as_ref().map(|wanted| wanted.app.as_str()),
            Some("Calculator")
        );
    });
    desk.update(cx, Desk::open_for_person);
    let bundle = "com.apple.calculator".to_owned();
    assert_eq!(told.try_recv(), Ok(ToHost::OpenForPerson { bundle }));
    desk.read_with(cx, |desk, _| assert!(desk.wanted.is_none()));
}

/// An app of the person's own that the host says is lent is kept for the tab to say, and
/// the host hears of it only when the person takes it back.
#[gpui_kit::test]
fn an_app_of_the_persons_that_is_lent_is_taken_back_on_their_word(cx: &mut TestAppContext) {
    let (desk, told) = running(cx);
    let lent = vec![Lent {
        pid: 412,
        app: "Arc".into(),
    }];
    says(&desk, FromHost::Lent { apps: lent.clone() }, cx);
    desk.read_with(cx, |desk, _| assert_eq!(desk.lent, lent));
    assert!(told.try_recv().is_err(), "nothing is given back unasked");
    desk.update(cx, |desk, cx| desk.take_back(412, cx));
    assert_eq!(told.try_recv(), Ok(ToHost::TakeBack { pid: 412 }));
    desk.read_with(cx, |desk, _| assert!(desk.lent.is_empty()));
}

#[gpui_kit::test]
fn what_the_person_presses_while_the_host_starts_is_not_kept(cx: &mut TestAppContext) {
    let desk = cx.new(Desk::new);
    let (tell, told) = channel();
    desk.update(cx, |desk, cx| {
        desk.state = State::Starting;
        desk.input(
            Input::Text {
                text: "typed too early".into(),
            },
            cx,
        );
        assert!(desk.started(desk.starts, Ok(Running::stand_in(tell)), cx));
    });
    let told: Vec<ToHost> = told.try_iter().collect();
    assert!(
        !told
            .iter()
            .any(|message| matches!(message, ToHost::Input { .. })),
        "{told:?}"
    );
}

/// The loader in Koyo's bundle is run from a copy that stands by itself, so macOS files its
/// grants under its own name; one that already stands alone is run where it is.
#[test]
fn a_loader_inside_koyos_bundle_is_run_from_a_copy_standing_alone() {
    let root = crate::test_support::scratch("desk-loader");
    let frameworks = root.join("Koyo.app/Contents/Frameworks");
    let loader = frameworks.join("Koyo Agent Desktop.app");
    let program = loader.join("Contents/MacOS/Koyo Agent Desktop");
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::write(&program, "loader").unwrap();
    std::fs::write(loader.join("Contents/Info.plist"), "plist").unwrap();
    std::fs::write(
        frameworks.join("libkoyo_desk_host.dylib"),
        "host, build one",
    )
    .unwrap();
    let state = root.join("state");

    let copy = super::host::standing_alone(&loader, &state).unwrap();
    assert_eq!(copy, state.join("host/Koyo Agent Desktop.app"));
    let read = |path: &str| std::fs::read_to_string(state.join("host").join(path)).unwrap();
    assert_eq!(
        read("Koyo Agent Desktop.app/Contents/MacOS/Koyo Agent Desktop"),
        "loader"
    );
    assert_eq!(read("libkoyo_desk_host.dylib"), "host, build one");

    // A new build brings a new library; the loader's copy stays the file it was.
    let placed = std::fs::metadata(copy.join("Contents/Info.plist")).unwrap();
    std::fs::write(
        frameworks.join("libkoyo_desk_host.dylib"),
        "host, build two",
    )
    .unwrap();
    assert_eq!(super::host::standing_alone(&loader, &state).unwrap(), copy);
    assert_eq!(read("libkoyo_desk_host.dylib"), "host, build two");
    let kept = std::fs::metadata(copy.join("Contents/Info.plist")).unwrap();
    assert_eq!(placed.modified().unwrap(), kept.modified().unwrap());

    // One that isn't inside another app is run where it is.
    assert_eq!(super::host::standing_alone(&copy, &state).unwrap(), copy);
}

/// The desktop's lease is a browser page's: an agent's while it acts, the person's once
/// they do, theirs to keep when they ask for it, and nobody's once an agent rests.
#[gpui_kit::test]
fn the_desktop_is_an_agents_while_it_acts_and_the_persons_once_they_do(cx: &mut TestAppContext) {
    let (desk, _told) = running(cx);
    let agent = Actor {
        id: "claude-code:7f3a".into(),
        label: "Claude Code".into(),
    };
    desk.update(cx, |desk, cx| {
        assert_eq!(desk.agent_acts(agent.clone(), cx), Ok(()));
        assert_eq!(desk.control.agent(), Some(&agent));
    });

    // The person acts in the tab: theirs while they're at it, the agent waiting, named.
    let input = Input::Text { text: "x".into() };
    desk.update(cx, |desk, cx| {
        desk.input(input, cx);
        assert!(desk.person_holds());
        assert_eq!(desk.control.displaced, Some(agent.clone()));
        let refused = desk.agent_acts(agent.clone(), cx).unwrap_err();
        let implicit = Mode::Implicit;
        assert!(matches!(refused, Refused::PersonHasControl { mode, .. } if mode == implicit));
    });
    // Left alone, it goes back by itself.
    cx.executor()
        .advance_clock(IMPLICIT_HOLD + Duration::from_millis(100));
    cx.run_until_parked();
    desk.update(cx, |desk, cx| {
        assert!(!desk.person_holds());
        assert_eq!(desk.agent_acts(agent.clone(), cx), Ok(()));
    });

    // Asked for, it's theirs until they give it back.
    desk.update(cx, |desk, cx| desk.person_acts(Mode::Explicit, cx));
    cx.executor().advance_clock(IMPLICIT_HOLD * 3);
    cx.run_until_parked();
    desk.update(cx, |desk, cx| {
        let refused = desk.agent_acts(agent.clone(), cx).unwrap_err();
        let explicit = Mode::Explicit;
        assert!(matches!(refused, Refused::PersonHasControl { mode, .. } if mode == explicit));
        desk.give_back(cx);
        assert_eq!(desk.agent_acts(agent.clone(), cx), Ok(()));
    });

    // An agent that rests has let the desktop go.
    cx.executor()
        .advance_clock(RESTS_AFTER + Duration::from_millis(100));
    cx.run_until_parked();
    desk.read_with(cx, |desk, _| assert_eq!(desk.control.holder, None));
}

/// The setup follows what the host says it holds: one grant asked for at a time, each seen
/// as it's given, and the setup offered by itself only once.
#[gpui_kit::test]
fn the_setup_takes_the_grants_one_at_a_time_as_the_host_reports_them(cx: &mut TestAppContext) {
    let (desk, told) = running(cx);
    desk.read_with(cx, |desk, _| assert_eq!(desk.step(), Step::Starting));
    let hello = |accessibility, screen_recording| FromHost::Hello {
        protocol: PROTOCOL,
        pid: 77,
        accessibility,
        screen_recording,
    };
    says(&desk, hello(false, false), cx);
    desk.update(cx, |desk, cx| {
        assert_eq!(desk.step(), Step::Needs(Grant::Accessibility));
        assert!(desk.setup_due());
        assert!(desk.offers_setup());
        assert!(!desk.offers_setup(), "offered once");
        desk.ask_grant(Grant::Accessibility, cx);
        assert!(desk.expecting(), "asked for: looked for from now on");
    });
    // The asking is a process of the loader's own, not the host's: the host is told nothing.
    assert_eq!(told.try_iter().count(), 0);

    // Given: the host says so by itself, and the next one is due.
    let grants = FromHost::Grants {
        accessibility: true,
        screen_recording: false,
    };
    says(&desk, grants, cx);
    desk.read_with(cx, |desk, _| {
        assert_eq!(desk.step(), Step::Needs(Grant::ScreenRecording));
    });

    // A host started with both makes the desktop: ready.
    says(&desk, hello(true, true), cx);
    desk.read_with(cx, |desk, _| assert_eq!(desk.step(), Step::Starting));
    let size = Size {
        width: 1800.,
        height: 1169.,
    };
    let state = DesktopState::Ready { size, scale: 2. };
    says(&desk, FromHost::Desktop { state }, cx);
    desk.read_with(cx, |desk, _| {
        assert_eq!(desk.step(), Step::Ready);
        assert!(!desk.setup_due());
    });
}
