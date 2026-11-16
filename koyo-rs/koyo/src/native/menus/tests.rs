use std::time::Duration;

use super::{Input, Origin, PRIVATE_STATE, Posters, Refusal, WITHIN, classify, decide};

/// `kCGEventSourceStateCombinedSessionState` and `kCGEventSourceStateHIDSystemState`.
const COMBINED: i64 = 0;
const HID: i64 = 1;

const KOYO: i64 = 4100;
const DESK: i64 = 4200;

fn posters() -> Posters {
    Posters {
        own: KOYO,
        desk: DESK,
    }
}

fn origin(state: i64, posted_by: i64) -> Origin {
    classify(Input { state, posted_by }, posters())
}

/// A press `ago`, from `state`, posted by `posted_by`, for a window on screen or not.
fn verdict(on_screen: bool, ago: Duration, state: i64, posted_by: i64) -> Result<(), Refusal> {
    decide(on_screen, Some((ago, origin(state, posted_by))))
}

const NOW: Duration = Duration::from_millis(40);

#[test]
fn the_hardwares_press_in_a_window_on_screen_shows_its_menu() {
    assert_eq!(verdict(true, NOW, HID, 0), Ok(()));
    assert_eq!(verdict(true, NOW, COMBINED, 0), Ok(()));
    // A busy page asking late is still answered.
    assert_eq!(verdict(true, WITHIN, HID, 0), Ok(()));
}

#[test]
fn a_press_posted_for_the_person_by_a_tool_of_theirs_shows_its_menu() {
    // A remapped button or Screen Sharing posts as a process of its own.
    assert_eq!(verdict(true, NOW, HID, 777), Ok(()));
    assert_eq!(verdict(true, NOW, COMBINED, 777), Ok(()));
}

#[test]
fn a_press_from_a_private_event_source_shows_no_menu() {
    // The agent desktop's host posts from one, `CGEventPostToPid` keeping its pid or not.
    for posted_by in [0, DESK, 777] {
        assert!(matches!(
            verdict(true, NOW, PRIVATE_STATE, posted_by),
            Err(Refusal::Posted(_))
        ));
    }
}

#[test]
fn a_press_the_agent_desktops_host_posted_shows_no_menu_whatever_state_it_claims() {
    for state in [HID, COMBINED, PRIVATE_STATE] {
        assert!(matches!(
            verdict(true, NOW, state, DESK),
            Err(Refusal::Posted(_))
        ));
    }
}

#[test]
fn a_press_koyo_posted_itself_shows_no_menu() {
    for state in [HID, COMBINED] {
        assert!(matches!(
            verdict(true, NOW, state, KOYO),
            Err(Refusal::Posted(_))
        ));
    }
}

#[test]
fn no_press_lately_shows_no_menu() {
    // An agent's click through the page's protocol, or a press handed straight to the
    // window, leaves no press of the system's.
    assert_eq!(decide(true, None), Err(Refusal::NoInput));
    let late = WITHIN + Duration::from_millis(1);
    assert_eq!(verdict(true, late, HID, 0), Err(Refusal::NoInput));
}

#[test]
fn a_window_not_on_the_persons_screen_shows_no_menu_even_for_the_hardwares_press() {
    assert_eq!(verdict(false, NOW, HID, 0), Err(Refusal::NotOnScreen));
    assert_eq!(decide(false, None), Err(Refusal::NotOnScreen));
}

#[test]
fn no_agent_desktop_host_yet_leaves_pid_0_the_hardwares() {
    let none = Posters { own: KOYO, desk: 0 };
    let hardware = classify(
        Input {
            state: HID,
            posted_by: 0,
        },
        none,
    );
    assert_eq!(hardware, Origin::Person);
}

#[test]
fn a_windows_own_last_input_decides_not_another_windows() {
    super::WATCHING.set(true);
    super::note(
        11,
        Input {
            state: HID,
            posted_by: 0,
        },
    );
    super::note(
        12,
        Input {
            state: PRIVATE_STATE,
            posted_by: DESK,
        },
    );
    assert_eq!(super::judge("a test's menu", 11, true), Ok(()));
    assert!(matches!(
        super::judge("a test's menu", 12, true),
        Err(Refusal::Posted(_))
    ));
    assert_eq!(
        super::judge("a test's menu", 13, true),
        Err(Refusal::NoInput)
    );
    assert_eq!(super::latest_window(), Some(12));
    // A later press of the person's in the window is what counts.
    super::note(
        12,
        Input {
            state: HID,
            posted_by: 0,
        },
    );
    assert_eq!(super::judge("a test's menu", 12, true), Ok(()));
}

#[test]
fn while_nothing_is_noted_only_the_screen_decides() {
    super::WATCHING.set(false);
    assert_eq!(super::judge("a test's menu", 21, true), Ok(()));
    assert_eq!(
        super::judge("a test's menu", 21, false),
        Err(Refusal::NotOnScreen)
    );
}
