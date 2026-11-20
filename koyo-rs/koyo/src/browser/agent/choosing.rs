//! A file chooser a page opens while an agent holds it, as an agent's click on a file input
//! does: the agent's to answer, never the system's panel on the person's screen. It holds
//! nothing up; the agent gives it files (`browser_upload`) or cancels it (`browser_dialog`),
//! and it's cancelled when the agent lets the page go or the person takes it, so the page can
//! open the next one, which Chromium does one at a time.

use gpui_kit::Context;
use koyo_app_control::ops::browser::{DialogKind, PageDialog};
use koyo_browser_ipc::{Answer, FileMode};

use super::super::view::BrowserView;
use super::{Actor, Unanswered};

/// A file chooser the page waits on, the agent's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chooser {
    /// The page's question.
    id: u64,
    pub mode: FileMode,
    /// The page's title for it.
    title: String,
    /// What it accepts: MIME types (`image/*`) or extensions (`.png`).
    accept: Vec<String>,
}

/// Which file chooser an agent read: the page's question, and what it asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asked {
    id: u64,
    pub mode: FileMode,
}

impl Chooser {
    /// The chooser as an agent reads it.
    fn as_agents_see(&self) -> PageDialog {
        let asks = match self.mode {
            FileMode::Open => "a file",
            FileMode::OpenMultiple => "files",
            FileMode::Folder => "a folder",
            FileMode::Save => "where to save a file",
        };
        let accepts = if self.accept.is_empty() {
            String::new()
        } else {
            format!(" ({})", self.accept.join(", "))
        };
        let titled = if self.title.is_empty() {
            String::new()
        } else {
            format!(": {}", self.title)
        };
        PageDialog {
            kind: DialogKind::FileChooser,
            message: format!("The page asks for {asks}{accepts}{titled}."),
            default_text: None,
        }
    }
}

impl BrowserView {
    /// The page asks for files as question `id`: kept for the agent that holds the page,
    /// whose act opened it, and `true`; `false` when no agent holds it, the person's to answer.
    pub(in crate::browser) fn chooser_for_agent(
        &mut self,
        id: u64,
        (mode, title, accept): (FileMode, &str, &[String]),
        cx: &mut Context<Self>,
    ) -> bool {
        if self.agent_holding().is_none() {
            return false;
        }
        self.chooser_dismissed(cx);
        self.chooser = Some(Chooser {
            id,
            mode,
            title: title.to_owned(),
            accept: accept.to_vec(),
        });
        cx.notify();
        true
    }

    /// The file chooser the page waits on, as an agent reads it.
    pub(super) fn chooser_dialog(&self) -> Option<PageDialog> {
        self.chooser.as_ref().map(Chooser::as_agents_see)
    }

    /// The file chooser the page waits on, if it waits on one: which it is, and what it asks
    /// for.
    pub fn chooser_asked(&self) -> Option<Asked> {
        (self.chooser.as_ref()).map(|chooser| Asked {
            id: chooser.id,
            mode: chooser.mode,
        })
    }

    /// The page withdrew its question `id`: if it's the agent's chooser, it's gone.
    pub(in crate::browser) fn chooser_withdrawn(&mut self, id: u64) {
        self.chooser.take_if(|chooser| chooser.id == id);
    }

    /// The agent's chooser is cancelled, if there is one: it let the page go, the person took
    /// it, or the agent said so.
    pub(in crate::browser) fn chooser_dismissed(&mut self, cx: &mut Context<Self>) {
        if let Some(chooser) = self.chooser.take() {
            self.send_answer(chooser.id, Answer::Dismiss, cx);
            cx.notify();
        }
    }

    /// `actor` gives the page's file chooser `paths`, absolute paths it checked it may give
    /// as `asked`, the chooser it read, asks; it must hold the page or find it free. The page
    /// asking anew since, its chooser takes none of them.
    pub fn agent_chooses(
        &mut self,
        actor: &Actor,
        asked: Asked,
        paths: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Result<(), Unanswered> {
        (self.agent_claims(actor.clone(), false, None, cx)).map_err(Unanswered::Refused)?;
        if self.chooser_asked().is_some_and(|now| now != asked) {
            return Err(Unanswered::AskedAnew);
        }
        let chooser = self.chooser.take().ok_or(Unanswered::NothingAsked)?;
        self.send_answer(chooser.id, Answer::Files { paths }, cx);
        cx.notify();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
