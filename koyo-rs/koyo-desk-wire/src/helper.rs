//! The root helper's requests (`docs/agent-desktop.md` 5.4) and its record of the setup
//! (5.3). JSON lines on [`crate::paths::HELPER_SOCKET`], which answers only the person who set
//! it up.

use serde::{Deserialize, Serialize};

/// What the person's side asks the helper daemon.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ask", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum HelperAsk {
    Status,
    /// Anchor checked, Screen Sharing on: answered with the account's name and password for
    /// the holder's login.
    PrepareLogin,
    /// The holder has logged in: the override that would start Screen Sharing at the next boot
    /// is taken back, the running service left as it is.
    LoginDone,
    /// The holder's lease: answered at once, then held open for as long as the holder holds the
    /// session. Its end, however it ends, ends the session (`docs/agent-desktop.md` 6.1).
    Hold,
    /// The account logged out and every process of its uid gone; Screen Sharing off.
    EndSession,
    /// Copy the host bundle at `bundle` into the shared folder.
    InstallHost {
        bundle: String,
    },
    /// Restart the host in the account's session.
    RestartHost,
    /// Remove everything setup made. `admin` and `password` delete the user record (root
    /// can't), are checked before anything is touched, and are kept nowhere.
    Remove {
        admin: String,
        password: String,
    },
}

/// The helper's answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "answer",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HelperAnswer {
    Status(HelperStatus),
    Login {
        account: String,
        password: String,
    },
    /// Done; `steps` says what was done, in order.
    Done {
        steps: Vec<String>,
    },
    /// Refused or failed; `steps` says what was done before it stopped.
    Failed {
        message: String,
        steps: Vec<String>,
    },
    /// No login yet: the kernel still lists the account's last session, or it ended less than
    /// [`crate::paths::AFTER_AN_END`] ago (`docs/agent-desktop.md` 6.1). Ask again in `wait_ms`.
    NotYet {
        wait_ms: u64,
        why: String,
    },
}

/// How the setup stands, as the helper sees it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperStatus {
    pub helper_version: String,
    /// The running macOS; [`crate::paths::macos_is_safe`] says whether it has the Screen Sharing
    /// fix.
    pub macos: String,
    /// Why a login would be refused, when it would: [`crate::paths::macos_refusal`].
    pub refusal: Option<String>,
    pub account: String,
    pub uid: u32,
    pub screen_sharing_on: bool,
    pub anchor_loaded: bool,
    /// Whether the account is logged in: its GUI domain exists.
    pub session: bool,
    /// The installed host's version, from its bundle.
    pub host_version: Option<String>,
}

/// The helper's record of the setup, `state.json`, root's alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupState {
    /// The one user the daemon answers.
    pub person_uid: u32,
    pub uid: u32,
    pub gid: u32,
    pub password: String,
    /// The reference `pfctl -E` gave, released on removal.
    pub pf_token: Option<String>,
    /// The macOS it was set up on.
    pub macos: String,
    /// Set up on a macOS without the Screen Sharing fix, by a developer's choice.
    pub accepted_risk: bool,
    /// Whether setup made `/Library/PrivilegedHelperTools`, so removal takes it away if empty.
    pub made_helper_tools: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holder::line;

    #[test]
    fn asks_and_answers_read_as_their_lines() {
        let text = line(&HelperAsk::InstallHost {
            bundle: "/x/Koyo Agent Desktop.app".into(),
        })
        .unwrap();
        assert_eq!(
            text,
            "{\"ask\":\"installHost\",\"bundle\":\"/x/Koyo Agent Desktop.app\"}\n"
        );
        let answer: HelperAnswer =
            serde_json::from_str(r#"{"answer":"login","account":"koyo-agents","password":"p"}"#)
                .unwrap();
        assert_eq!(
            answer,
            HelperAnswer::Login {
                account: "koyo-agents".into(),
                password: "p".into()
            }
        );
        let status = HelperAnswer::Status(HelperStatus {
            macos: "26.7".into(),
            ..HelperStatus::default()
        });
        let text = line(&status).unwrap();
        assert_eq!(serde_json::from_str::<HelperAnswer>(&text).unwrap(), status);
    }
}
