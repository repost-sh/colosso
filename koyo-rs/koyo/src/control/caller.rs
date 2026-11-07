//! Who calls: the client a hello names, and where it runs, as it says.

use std::path::PathBuf;

use koyo_app_control::{ClientInfo, Hello};

/// A client that said hello, as it said it: self-reported, so it names the caller in
/// prompts and the activity log, and picks its defaults, but proves nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Caller {
    pub client: ClientInfo,
    /// The Koyo terminal it runs in, a `term_…` handle, for an agent in a terminal.
    pub terminal: Option<String>,
    /// The Koyo chat whose agent it is, the chat's id, for Koyo's MCP server in a chat.
    pub chat: Option<String>,
    /// The worktree its environment names (`KOYO_WORKTREE`).
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
            chat: hello.chat.clone().filter(|id| !id.is_empty()),
            worktree: path(&hello.worktree),
            cwd: path(&hello.cwd),
        }
    }

    /// Where in Koyo it runs, for an agent Koyo runs: `("chat", id)`, else `("terminal", handle)`.
    fn agent(&self) -> Option<(&'static str, &str)> {
        (self.chat.as_deref().map(|id| ("chat", id)))
            .or_else(|| self.terminal.as_deref().map(|handle| ("terminal", handle)))
    }

    /// What receipts, prompts and the activity log know the caller by: `chat:<id>` for a
    /// chat's agent, `terminal:<handle>` for an agent in a terminal, else the client's name.
    pub fn key(&self) -> String {
        self.agent().map_or_else(
            || self.client.name.clone(),
            |(kind, id)| format!("{kind}:{id}"),
        )
    }

    /// Whether it calls from outside Koyo's terminals and chats, so the policy decides for it.
    pub fn external(&self) -> bool {
        self.agent().is_none()
    }

    /// How the person's prompts name it.
    pub fn label(&self) -> String {
        let name = &self.client.name;
        self.agent().map_or_else(
            || name.clone(),
            |(kind, id)| format!("{name} (in {kind} {id})"),
        )
    }
}
