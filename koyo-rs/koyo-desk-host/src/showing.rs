//! Taking the focus (`docs/agent-desktop.md` 5): the agent desktop put on the person's
//! screen, with an agent's app in front, for work an app won't do unseen (the person's
//! own Chromium browser, lent, draws no page, and gives none to read, on a desktop that
//! isn't showing; one the host started does both). It is the one thing here that changes
//! what the person is looking at, so it is done only on their word (`consent`).
//!
//! The desktops are switched by Mission Control's own buttons, pressed through
//! accessibility, as the agent desktop is made: no key or pointer event, and the desktop
//! meant, whatever macOS is set to do when an app comes forward. The focus goes back when
//! the agent returns it, when the lent app it was taken for goes back, and when the host
//! ends: to the desktop the person was on. Going to a desktop of their own takes it back
//! at once.

use std::time::Duration;

use koyo_desk_wire::link::{Answer, LogLevel, Refusal, RefusalCode};

use crate::consent::{self, Asked};
use crate::host::{Host, no_desktop};
use crate::skylight::{self, SpaceId};
use crate::{apps, desktop, mission, ns};

/// How long the guard of the front stands down while the desktops are switched.
const SWITCHES_WITHIN: Duration = Duration::from_secs(4);

/// The focus as it's held: where the person was when it was taken, and the app it was
/// taken for.
pub struct Taken {
    from: Option<SpaceId>,
    pub pid: i32,
}

/// What the agent has to do before the focus is taken for `app`. `again`: it was held
/// before, and went back.
fn refusal(app: &str, again: bool) -> Refusal {
    let why = if again {
        "You had the focus before, and it is the person's again. What they said then is used up."
    } else {
        "Taking the focus is the person's to allow."
    };
    let means = format!(
        "{why} It puts the agent desktop on their screen in place of what they are doing, with \
         {app} in front: their screen and keyboard are with it until you give the focus back \
         (desktop_return_focus) or they go to a desktop of their own. As an agent you must not \
         do this without the person's explicit consent, and only when nothing less will do: \
         when an app can't do unseen what you need of it (the person's own browser, lent, draws \
         no page on a desktop that isn't showing; a browser you opened there yourself does)."
    );
    let tell = "that you want to bring the agent desktop onto their screen, and why it's \
                necessary; what you will do while it is there, and about how long; and that \
                their own desktop comes back when you are done, or the moment they go to it \
                themselves.";
    consent::ask(&means, tell, "desktop_take_focus")
}

impl Host {
    /// Puts the agent desktop on the person's screen with `window` (the frontmost, when
    /// none is named) in front, when they agreed to it; the refusal that says what to tell
    /// them and ask, when they haven't yet.
    pub fn take_focus(
        &mut self,
        window: Option<u32>,
        consent: Option<&str>,
    ) -> Result<Answer, Refusal> {
        let Some(space) = self.space() else {
            return Err(no_desktop(&self.missing));
        };
        self.list();
        let shown = match window {
            Some(id) => self.listed(id)?.clone(),
            None => (self.windows.first().cloned()).ok_or_else(|| {
                let message = "no window on the agent desktop to take the focus for";
                Refusal::new(RefusalCode::NotFound, message)
            })?,
        };
        let said = (self.consents)
            .take(Asked::Focus, consent)
            .map_err(|again| refusal(&shown.app, again))?
            .to_owned();
        let from = skylight::active().filter(|active| *active != space);
        // The agent's app is about to be in front, and is meant to be.
        self.front.stand_down(SWITCHES_WITHIN);
        if let Err(why) = mission::go_to(space) {
            let message =
                format!("the agent desktop couldn't be put on the person's screen: {why}");
            return Err(Refusal::new(RefusalCode::Failed, message));
        }
        self.raise(shown.id)?;
        if let Some(app) = self.apps.iter().find(|app| app.pid == shown.pid) {
            apps::activate(&app.running);
        }
        // The desktop the person was on is kept from the first taking: taken again while
        // it's held, that is still where they go back to.
        let from = self.taken.take().map_or(from, |taken| taken.from);
        let pid = shown.pid;
        self.taken = Some(Taken { from, pid });
        let app = &shown.app;
        self.log(
            LogLevel::Info,
            format!(
                "the agent desktop is on the person's screen, {app} in front, on their word: \
                 {said:?}"
            ),
        );
        Ok(Answer::Done)
    }

    /// Gives the focus back: the person's screen shows the desktop they were on again.
    /// Nothing to do when it isn't held, or they left by themselves.
    pub fn return_focus(&mut self) -> Result<Answer, Refusal> {
        let Some(taken) = self.taken.take() else {
            return Ok(Answer::Done);
        };
        let agents = self.space();
        if agents.is_none() || skylight::active() != agents {
            return Ok(Answer::Done);
        }
        let Some(back) = desktop::back_to(taken.from, agents) else {
            return Ok(Answer::Done);
        };
        if let Err(why) = mission::go_to(back) {
            self.taken = Some(taken);
            let message = format!("the person's desktop couldn't be brought back: {why}");
            return Err(Refusal::new(RefusalCode::Failed, message));
        }
        // What macOS brings forward there is answered by the guard, should it be an app
        // of the agent's: the person's own that is lent, on the desktop it left.
        ns::wake();
        self.log(
            LogLevel::Info,
            "gave the focus back: the person's desktop is on their screen again",
        );
        Ok(Answer::Done)
    }

    /// Sees the person leave the agent desktop by themselves: the focus is theirs again.
    pub fn watch_focus(&mut self) {
        if self.taken.is_some() && skylight::active() != self.space() && !self.front.stands_down() {
            self.taken = None;
            self.log(
                LogLevel::Info,
                "the person went to a desktop of their own: the focus is theirs again",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::refusal;
    use koyo_desk_wire::link::RefusalCode;

    /// The refusal says what taking the focus does to the person's screen, that it needs
    /// their word and a need, what to tell them, and where to put their words.
    #[test]
    fn the_focus_is_refused_with_what_to_ask_the_person() {
        let asks = refusal("Arc", false);
        assert_eq!(asks.code, RefusalCode::NeedsConsent);
        for part in [
            "puts the agent desktop on their screen",
            "Arc in front",
            "must not do this without the person's explicit consent",
            "only when nothing less will do",
            "desktop_return_focus",
            "call desktop_take_focus again with `consent` set to the exact text",
            "Nothing they said before this refusal counts",
        ] {
            assert!(asks.message.contains(part), "{part}: {}", asks.message);
        }
        let again = refusal("Arc", true).message;
        assert!(again.starts_with("You had the focus before") && again.contains("used up"));
    }
}
