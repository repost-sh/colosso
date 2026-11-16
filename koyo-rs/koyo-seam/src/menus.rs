//! A native menu on the person's screen only when the person asked for it (`docs/browser.md`
//! 14). macOS draws a native menu (a page's context menu, Koyo's own, Chromium's popup for a
//! `<select>`) at menu level on the Space the person is looking at, whatever window or Space it
//! was opened for: a menu for a window on the agent desktop, or one an agent's click opened in a
//! window of the person's, showed on the person's screen.
//!
//! So every native menu the process pops up passes one gate first ([`judge`]), which refuses on
//! evidence and allows otherwise: the window it's for isn't on the screen the person sees, or
//! that window's last press or key, as the layer heard it ([`crate::heard`]), wasn't the
//! person's or was too long ago. A press of the person's own, in a window on their screen,
//! always shows its menu.

use std::time::Duration;

use crate::heard;
use crate::input::{Origin, Poster};

/// How long after the person's press or key a menu still counts as theirs: as long as a busy
/// page may take to ask for its context menu.
pub const WITHIN: Duration = Duration::from_secs(5);

/// Why a menu wasn't shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The window it's for isn't on the screen the person sees.
    NotOnScreen,
    /// No press or key in that window lately: nothing of the person's asked for it.
    NoInput,
    /// The window's last input wasn't the person's, as said.
    NotThePersons(&'static str),
    /// An agent drives the page that asked for it.
    Agent,
}

impl Refusal {
    #[must_use]
    pub fn why(self) -> &'static str {
        match self {
            Refusal::NotOnScreen => "its window isn't on the screen the person sees",
            Refusal::NoInput => "no press or key in its window asked for it",
            Refusal::NotThePersons(whose) => whose,
            Refusal::Agent => "an agent drives the page that asked for it",
        }
    }
}

/// Whether a native menu may show: the window it's for is on the screen the person sees, and
/// that window's last input, `last` (how long ago, and whose), was the person's, lately.
///
/// # Errors
/// Why it may not.
pub fn decide(on_screen: bool, last: Option<(Duration, Origin)>) -> Result<(), Refusal> {
    if !on_screen {
        return Err(Refusal::NotOnScreen);
    }
    let Some((ago, origin)) = last else {
        return Err(Refusal::NoInput);
    };
    if ago > WITHIN {
        return Err(Refusal::NoInput);
    }
    Err(Refusal::NotThePersons(match origin {
        Origin::Person { .. } => return Ok(()),
        Origin::Posted(Poster::DeskHost) => {
            "its window's last input was posted by the agent desktop's host"
        }
        Origin::Posted(Poster::PrivateSource) => {
            "its window's last input was posted from a program's private event source"
        }
        Origin::Engine => "its window's last input was an agent's, through the page's protocol",
        Origin::Probe => "its window's last input was the harness's",
        Origin::Koyo => "its window's last input was made by Koyo itself",
    }))
}

/// The gate's verdict on a menu for window number `window`, which is on the screen the person
/// sees or not, with the reason logged as `what` is refused. Until the layer hears the app's
/// inputs, nothing is known of them, and only the screen decides.
///
/// # Errors
/// Why it may not show.
pub fn judge(what: &str, window: isize, on_screen: bool) -> Result<(), Refusal> {
    let last = heard::last(window);
    let verdict = if heard::hearing() {
        decide(on_screen, last.map(|(ago, input)| (ago, input.origin)))
    } else {
        decide(on_screen, Some((Duration::ZERO, Origin::Person { through: None })))
    };
    // Whose the window's last input was and how long ago: never what was pressed.
    let said = last.map(|(ago, input)| (ago, input.origin));
    match verdict {
        Ok(()) => log::info!(
            target: "koyo::menus",
            "showing {what} for window {window}; its last input (ago, whose): {said:?}"
        ),
        Err(refusal) => log::warn!(
            target: "koyo::menus",
            "refused {what} for window {window}: {}; its last input (ago, whose): {said:?}",
            refusal.why()
        ),
    }
    verdict
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Refusal, WITHIN, decide, judge};
    use crate::heard;
    use crate::input::{Kind, Origin, Poster, Source};

    const NOW: Duration = Duration::from_millis(40);
    const PERSON: Origin = Origin::Person { through: None };

    #[test]
    fn the_persons_press_in_a_window_on_screen_shows_its_menu() {
        assert_eq!(decide(true, Some((NOW, PERSON))), Ok(()));
        // A busy page asking late is still answered.
        assert_eq!(decide(true, Some((WITHIN, PERSON))), Ok(()));
    }

    #[test]
    fn a_press_posted_for_the_person_by_a_tool_of_theirs_shows_its_menu() {
        // A remapped button or Screen Sharing posts as a process of its own.
        assert_eq!(decide(true, Some((NOW, Origin::Person { through: Some(777) }))), Ok(()));
    }

    #[test]
    fn an_input_that_isnt_the_persons_shows_no_menu_whoevers_it_is() {
        for origin in [
            Origin::Posted(Poster::DeskHost),
            Origin::Posted(Poster::PrivateSource),
            Origin::Engine,
            Origin::Probe,
            Origin::Koyo,
        ] {
            let refused = decide(true, Some((NOW, origin)));
            assert!(matches!(refused, Err(Refusal::NotThePersons(_))), "{origin:?}");
        }
    }

    #[test]
    fn no_press_lately_shows_no_menu() {
        assert_eq!(decide(true, None), Err(Refusal::NoInput));
        let late = WITHIN + Duration::from_millis(1);
        assert_eq!(decide(true, Some((late, PERSON))), Err(Refusal::NoInput));
    }

    #[test]
    fn a_window_not_on_the_persons_screen_shows_no_menu_even_for_the_persons_press() {
        assert_eq!(decide(false, Some((NOW, PERSON))), Err(Refusal::NotOnScreen));
        assert_eq!(decide(false, None), Err(Refusal::NotOnScreen));
    }

    const HARDWARE: Option<Source> = Some(Source { state: 1, posted_by: 0 });

    #[test]
    fn until_inputs_are_heard_only_the_screen_decides_and_then_each_windows_own_last() {
        assert_eq!(judge("a test's menu", 21, true), Ok(()));
        assert_eq!(judge("a test's menu", 21, false), Err(Refusal::NotOnScreen));
        heard::listening();
        assert_eq!(judge("a test's menu", 21, true), Err(Refusal::NoInput));
        heard::app_sends(11, HARDWARE, Kind::Press);
        heard::probe_sends(12, Kind::Press);
        assert_eq!(judge("a test's menu", 11, true), Ok(()));
        assert!(matches!(judge("a test's menu", 12, true), Err(Refusal::NotThePersons(_))));
        assert_eq!(judge("a test's menu", 13, true), Err(Refusal::NoInput));
        // A later press of the person's in the window is what counts.
        heard::app_sends(12, HARDWARE, Kind::Press);
        assert_eq!(judge("a test's menu", 12, true), Ok(()));
    }

    #[test]
    fn a_press_the_harness_hands_the_window_after_the_persons_takes_their_menu_from_it() {
        heard::listening();
        heard::app_sends(31, HARDWARE, Kind::Press);
        heard::probe_sends(31, Kind::Press);
        assert!(matches!(judge("a test's menu", 31, true), Err(Refusal::NotThePersons(_))));
    }
}
