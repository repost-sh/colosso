//! An app of the person's own on the agent desktop (`docs/agent-desktop.md` 5). An agent may
//! use the instance the person runs (asked for as theirs, or of an app that takes no second
//! instance), but only on the person's word (`consent`).
//!
//! The app stays theirs: lent. It's put on the agent desktop with every window it has, and
//! given back, never quit: when the agent closes it, when the person takes it back in the
//! tab, and when the host ends. A host that crashed leaves a record of what was lent, and
//! the next one gives it back before all else. Its coming to the front gives nothing back:
//! macOS brings an app forward for reasons of its own (a page it was asked for, the
//! desktop it used to be on), and a lent app that went back each time was of no use (seen
//! with Arc, 2026-11-10). The front is given to the person's app, as for any app of the
//! agent's (`front`).
//!
//! The Dock remembers each desktop's front app and brings it forward as the person arrives
//! there; an app brought forward shows its window, and the Dock follows that window to the
//! desktop it's on: the person, going to the desktop their app left, would land on the
//! agent's. Telling the window server another front app for that desktop
//! (`SLSSpaceSetFrontPSN`, which the Dock itself calls) changed nothing: the Dock brought
//! the lent app forward all the same. What keeps the person where they went is the front
//! taken from the app again within milliseconds (`front`). Given back, the app goes to the
//! desktop it came from.

use std::fmt::Write as _;

use koyo_desk_wire::link::{FromHost, Lent, LogLevel, Refusal, RefusalCode};

use crate::app::App;
use crate::consent::{self, Asked};
use crate::host::Host;
use crate::{apps, desktop};

/// What an agent said of the instance the person runs.
#[derive(Clone, Debug, Default)]
pub struct Wish {
    /// It asked for theirs, not one of its own.
    pub theirs: bool,
    /// The person's words agreeing to it, as the agent gave them.
    pub consent: Option<String>,
}

/// How a lent app goes back.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Back {
    /// Where it was, with nothing brought forward: the agent closed it, the host ends.
    Quiet,
    /// In front of the person, who took it back.
    Front,
}

/// What the agent has to do before the person's own running `asked` is taken. `wished`:
/// the agent asked for theirs, not the app for it by taking no second instance. `again`:
/// it was lent before, and went back.
fn refusal(asked: &str, wished: bool, again: bool) -> Refusal {
    let why = if again {
        format!(
            "{asked} was lent to you before and is the person's again. What they said then is \
             used up."
        )
    } else if wished {
        format!("You asked for the person's own {asked}, the one they run.")
    } else {
        format!(
            "{asked} is running as the person's own and takes no second instance, so none can \
             be started for you."
        )
    };
    let means = format!(
        "{why} Using it means taking their {asked} onto the agent desktop: every window it \
         has, with all they have open in it, leaves their desktops, and you act in their app, \
         with their data and accounts, until it is given back (desktop_close with its pid \
         gives it back, and they can take it back in Koyo at any time). As an agent you must \
         not do this without the person's explicit consent."
    );
    let tell = format!(
        "that you want to bring their running {asked} to the agent desktop, and why; that all \
         of its windows will leave their desktop until it is given back; and what you mean to \
         do in it."
    );
    let mut asks = consent::ask(&means, &tell, "desktop_open");
    let _ = write!(
        asks.message,
        " If {asked} is a web browser, tell them this too: their own Chromium browser (Arc, \
         Chrome), once lent, draws no page and gives none to read while the agent desktop \
         isn't on their screen, \
         so you would also have to ask for the focus (desktop_take_focus), which puts the \
         agent desktop on their screen while you work in it; for a web page Koyo's own \
         browser tools serve better."
    );
    asks
}

impl Host {
    /// Takes the person's own running `pid` in when they agreed to it; the refusal that
    /// says what to tell them and ask, when they haven't yet.
    pub fn borrow(
        &mut self,
        pid: i32,
        asked: &str,
        bundle: Option<&str>,
        wish: &Wish,
    ) -> Result<(), Refusal> {
        // The Finder holds the person's desktop itself.
        if bundle == Some(apps::FINDER) {
            let message = format!(
                "{asked} is running as the person's and is never lent to agents: use another app"
            );
            return Err(Refusal::new(RefusalCode::AppIsThePersons, message));
        }
        let said = (self.consents)
            .take(Asked::App(pid), wish.consent.as_deref())
            .map_err(|again| refusal(asked, wish.theirs, again))?
            .to_owned();
        let shown = desktop::shown(pid, self.space());
        // An app assigns whole, and a full screen is a space of its own that doesn't.
        if shown.full_screen {
            let message = format!(
                "{asked} has a window in full screen, which can't be brought over: ask the \
                 person to take it out of full screen, then open it again"
            );
            return Err(Refusal::new(RefusalCode::Failed, message));
        }
        let Some(launched) = apps::running_app(pid) else {
            let message = format!("{asked} isn't running any more");
            return Err(Refusal::new(RefusalCode::Failed, message));
        };
        let home = shown.home;
        self.apps.push(App::lent(launched, home));
        self.apps_changed();
        self.log(
            LogLevel::Info,
            format!(
                "{asked} (process {pid}) is the person's own, lent to the agent desktop from \
                 desktop {home:?} on their word: {said:?}"
            ),
        );
        Ok(())
    }

    /// Gives a lent app back to the person, and forgets what the agent held of it. Their
    /// screen first, when the focus was taken for it: the app leaves the desktop on it.
    pub fn give_back(&mut self, pid: i32, back: Back) {
        let Some(at) = (self.apps.iter()).position(|app| app.pid == pid && app.lent) else {
            return;
        };
        if self.taken.as_ref().is_some_and(|taken| taken.pid == pid) {
            let _ = self.return_focus();
        }
        let mut app = self.apps.remove(at);
        // Theirs again before it's touched: the guard of the front takes it for the
        // agent's until it's told otherwise.
        self.apps_changed();
        app.leave_as_found();
        // Nothing an agent held of it reaches it from now on: an element of its windows,
        // text on its way to it, a window held by its bar.
        self.refs.clear();
        self.moving = None;
        self.grab = self.grab.filter(|grab| grab.pid != pid);
        let name = &app.name;
        let message = format!(
            "{name} went back to the person while this was being typed: part of the text is in"
        );
        let refusal = Refusal::new(RefusalCode::AppIsThePersons, message);
        self.stop_typing_where(|typing| typing.target.pid == pid, &refusal);
        desktop::hand_back(pid, app.home, self.space());
        let why = match back {
            Back::Quiet => "",
            Back::Front => {
                apps::activate(&app.running);
                ": they took it back"
            }
        };
        self.log(
            LogLevel::Info,
            format!("gave {name} back to the person{why}"),
        );
        self.list();
    }

    /// Gives every lent app back: the host is ending.
    pub fn give_all_back(&mut self) {
        let lent: Vec<i32> = (self.apps.iter())
            .filter(|app| app.lent)
            .map(|app| app.pid)
            .collect();
        for pid in lent {
            self.give_back(pid, Back::Quiet);
        }
    }

    /// Tells the app which of the person's apps are lent now, when that changed.
    pub fn says_lent(&mut self) {
        let apps: Vec<Lent> = (self.apps.iter())
            .filter(|app| app.lent)
            .map(|app| Lent {
                pid: app.pid,
                app: app.name.clone(),
            })
            .collect();
        if apps != self.lent_said {
            self.lent_said.clone_from(&apps);
            self.tell(&FromHost::Lent { apps });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::refusal;
    use koyo_desk_wire::link::RefusalCode;

    /// The refusal names the app, says what lending it means and what to ask, and where to
    /// put the person's words.
    #[test]
    fn an_app_of_the_persons_is_refused_with_what_to_ask_them() {
        let asks = refusal("Arc", false, false);
        assert_eq!(asks.code, RefusalCode::NeedsConsent);
        for part in [
            "Arc is running as the person's own and takes no second instance",
            "every window it has",
            "must not do this without the person's explicit consent",
            "call desktop_open again with `consent` set to the exact text",
            "Nothing they said before this refusal counts",
            "If Arc is a web browser",
        ] {
            assert!(asks.message.contains(part), "{part}: {}", asks.message);
        }
        let wished = refusal("Xcode", true, false).message;
        assert!(
            wished.starts_with("You asked for the person's own Xcode"),
            "{wished}"
        );
        assert!(wished.contains("explicit consent"), "{wished}");
        let again = refusal("Arc", false, true).message;
        assert!(again.starts_with("Arc was lent to you before"), "{again}");
        assert!(again.contains("used up") && again.contains("explicit consent"));
    }
}
