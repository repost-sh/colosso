//! The person's word for what is theirs to allow (`docs/agent-desktop.md` 5): lending an app
//! they run, and taking the focus. Asked for as Prisma's guard asks before an agent resets
//! a database: the first asking is refused with what the agent must tell the person and
//! ask; the agent comes back with the exact text of the message in which they agreed.
//! Words count once, and only after that refusal: what was had and went back is asked for
//! anew.

use koyo_desk_wire::link::{Refusal, RefusalCode};

/// What the person is asked to allow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// Their own running app, by its pid, on the agent desktop.
    App(i32),
    /// The agent desktop on their screen.
    Focus,
}

/// What agents were told to ask, and what was allowed before.
#[derive(Default)]
pub struct Consents {
    /// What an agent was told how to ask for, since it was last had.
    told: Vec<Asked>,
    /// What was had before.
    before: Vec<Asked>,
}

impl Consents {
    /// The person's words for `asked`, when they count: there are some, and the agent was
    /// told what to ask since `asked` was last had. Otherwise the agent is told now, and
    /// the answer is whether `asked` was had before, for the refusal to say so.
    pub fn take<'a>(&mut self, asked: Asked, consent: Option<&'a str>) -> Result<&'a str, bool> {
        let said = consent.map(str::trim).filter(|said| !said.is_empty());
        let told = self.told.contains(&asked);
        match said {
            Some(said) if told => {
                self.told.retain(|other| *other != asked);
                self.before.push(asked);
                Ok(said)
            }
            _ => {
                if !told {
                    self.told.push(asked);
                }
                Err(self.before.contains(&asked))
            }
        }
    }
}

/// The refusal that sends the agent to the person: what the act `means` for them, what to
/// `tell` them, and the `tool` to call again with their words.
pub fn ask(means: &str, tell: &str, tool: &str) -> Refusal {
    let message = format!(
        "{means} Stop now, and tell them in a clear message: {tell} Then ask whether they \
         want that. If they explicitly agree, call {tool} again with `consent` set to the \
         exact text of the message in which they agreed. An unclear answer is not consent: \
         ask for a plain yes. Nothing they said before this refusal counts as consent."
    );
    Refusal::new(RefusalCode::NeedsConsent, message)
}

#[cfg(test)]
mod tests {
    use super::{Asked, Consents, ask};
    use koyo_desk_wire::link::RefusalCode;

    /// Words are taken only after a refusal said what to ask, and once: what went back is
    /// refused again, whatever words come with the asking; empty words are none.
    #[test]
    fn words_count_once_and_only_after_a_refusal() {
        let (arc, focus) = (Asked::App(7), Asked::Focus);
        let mut consents = Consents::default();
        assert_eq!(
            consents.take(arc, Some("yes")),
            Err(false),
            "no refusal came before"
        );
        assert_eq!(consents.take(arc, Some("   ")), Err(false), "empty words");
        assert_eq!(
            consents.take(arc, Some("  yes, bring it over \n")),
            Ok("yes, bring it over")
        );
        assert_eq!(
            consents.take(arc, Some("yes")),
            Err(true),
            "those words are used up"
        );
        assert_eq!(
            consents.take(arc, None),
            Err(true),
            "told again, with no words yet"
        );
        assert_eq!(consents.take(arc, Some("yes, again")), Ok("yes, again"));
        // Each thing asked for by itself.
        assert_eq!(consents.take(focus, None), Err(false));
        assert_eq!(consents.take(Asked::App(8), Some("go ahead")), Err(false));
        assert_eq!(consents.take(focus, Some("go ahead")), Ok("go ahead"));
    }

    #[test]
    fn the_refusal_ends_with_how_to_come_back_with_their_words() {
        let asks = ask("It means this.", "what and why.", "desktop_open");
        assert_eq!(asks.code, RefusalCode::NeedsConsent);
        assert!(asks.message.starts_with(
            "It means this. Stop now, and tell them in a clear message: what and why. Then ask"
        ));
        for part in [
            "call desktop_open again with `consent` set to the exact text",
            "An unclear answer is not consent",
            "Nothing they said before this refusal counts as consent.",
        ] {
            assert!(asks.message.contains(part), "{part}: {}", asks.message);
        }
    }
}
