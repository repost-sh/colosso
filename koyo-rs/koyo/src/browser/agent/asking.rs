//! A page's questions as an agent sees them: what the page asks and waits on, the ones an
//! agent answers (an alert, a confirm, a prompt, a page asking before it's left), which hold
//! up every command until they're answered, and the ones only the person does (a sign-in, a
//! permission, a certificate warning). An agent's command under way when the page asks one
//! that holds it up stops waiting at once ([`super::super::host::Browsers::page_asks`]).

use gpui_kit::{App, Context};
use koyo_app_control::ops::browser::{DialogAction, DialogKind, PageDialog};
use koyo_browser_ipc::{Answer, Dialog};

use super::super::dialog::{Question, says};
use super::super::view::BrowserView;
use super::{Actor, AgentError, Refused};

/// Why an agent's answer to a page's question didn't go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unanswered {
    /// The page's lease keeps the agent out.
    Refused(Refused),
    /// The page asks nothing.
    NothingAsked,
    /// The page asks the person, as `kind`.
    PersonAnswers(DialogKind),
}

/// What `dialog` is to an agent; a file chooser, which the system's panel asks, is nothing.
fn kind(dialog: &Dialog) -> Option<DialogKind> {
    Some(match dialog {
        Dialog::Alert { .. } => DialogKind::Alert,
        Dialog::Confirm { .. } => DialogKind::Confirm,
        Dialog::Prompt { .. } => DialogKind::Prompt,
        Dialog::BeforeUnload { .. } => DialogKind::Leave,
        Dialog::Auth { .. } => DialogKind::SignIn,
        Dialog::Permission { .. } => DialogKind::Permission,
        Dialog::Certificate { .. } => DialogKind::Certificate,
        Dialog::File { .. } => return None,
    })
}

/// Whether the page waits on `dialog`, doing nothing else, until it's answered.
pub(in crate::browser) fn holds_up(dialog: &Dialog) -> bool {
    kind(dialog).is_some_and(DialogKind::holds_up)
}

/// `question` as an agent reads it, in the words the person's dialog has.
fn as_agents_see(question: &Question) -> Option<PageDialog> {
    let default_text = match &question.asked {
        Dialog::Prompt { default_text, .. } => Some(default_text.clone()),
        _ => None,
    };
    Some(PageDialog {
        kind: kind(&question.asked)?,
        message: says(question),
        default_text,
    })
}

impl BrowserView {
    /// What the page asks and waits on, the first of its questions.
    pub fn dialog(&self) -> Option<PageDialog> {
        self.dialogs.front().and_then(as_agents_see)
    }

    /// Whether the page takes an agent's command now: the engine has it open, and it waits
    /// on no question that holds it up.
    pub fn ready_for_agents(&self, cx: &App) -> Result<(), AgentError> {
        if let Some(why) = self.not_open(cx) {
            return Err(AgentError::NotOpen(why));
        }
        let holding = (self.dialogs.front())
            .filter(|question| holds_up(&question.asked))
            .and_then(as_agents_see);
        match holding {
            Some(asked) => Err(AgentError::Asking(asked)),
            None => Ok(()),
        }
    }

    /// `actor` answers the question the page waits on, as `browser_dialog` does: one the
    /// page holds up for, which the person may have on screen too.
    pub fn agent_answers(
        &mut self,
        actor: &Actor,
        action: DialogAction,
        text: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<(), Unanswered> {
        (self.agent_claims(actor.clone(), false, None, cx)).map_err(Unanswered::Refused)?;
        let question = self.dialogs.front().ok_or(Unanswered::NothingAsked)?;
        let kind = kind(&question.asked).ok_or(Unanswered::NothingAsked)?;
        if !kind.holds_up() {
            return Err(Unanswered::PersonAnswers(kind));
        }
        let answer = match (action, &question.asked) {
            (DialogAction::Dismiss, _) => Answer::Dismiss,
            (DialogAction::Accept, Dialog::Prompt { default_text, .. }) => Answer::Text {
                text: text.unwrap_or_else(|| default_text.clone()),
            },
            (DialogAction::Accept, _) => Answer::Accept,
        };
        let id = question.id;
        self.put_away(id, cx);
        self.answered(id, answer, cx);
        Ok(())
    }
}
