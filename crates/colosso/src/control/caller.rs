//! Who calls: the client a hello names, and where it runs, as it says.

use std::path::PathBuf;

use colosso_app_control::{ClientInfo, Hello};

/// A client that said hello, as it said it: self-reported, so it names the caller in
/// prompts and the activity log, and picks its defaults, but proves nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Caller {
    pub client: ClientInfo,
    /// The Colosso terminal it runs in, a `term_…` handle, for an agent in a terminal.
    pub terminal: Option<String>,
    /// The worktree its environment names (`COLOSSO_WORKTREE`).
    pub worktree: Option<PathBuf>,
    /// Its working folder.
    pub cwd: Option<PathBuf>,
}

impl Caller {
    /// The caller a hello describes.
    pub fn from_hello(hello: &Hello) -> Caller {
        let path = |value: &Option<String>| {
            (value.as_deref())
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Caller {
            client: hello.client.clone(),
            terminal: hello.terminal.clone().filter(|handle| !handle.is_empty()),
            worktree: path(&hello.worktree),
            cwd: path(&hello.cwd),
        }
    }

    /// What receipts, prompts and the activity log know the caller by: `terminal:<handle>`
    /// for an agent in a terminal, else the client's name.
    pub fn key(&self) -> String {
        match &self.terminal {
            Some(handle) => format!("terminal:{handle}"),
            None => self.client.name.clone(),
        }
    }

    /// Whether it calls from outside Colosso's terminals, so the policy decides for it.
    pub fn external(&self) -> bool {
        self.terminal.is_none()
    }

    /// How the person's prompts name it.
    pub fn label(&self) -> String {
        match &self.terminal {
            Some(handle) => format!("{} (in terminal {handle})", self.client.name),
            None => self.client.name.clone(),
        }
    }
}
