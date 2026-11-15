//! A copy of the person's own page, pinned or loose, as a page of the caller's worktree: asked
//! for, as only the person may hand one of their pages on.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ops::shared::{PaneId, PanePlace};
use crate::{Hints, Operation, Scope};

/// The page to copy.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserDuplicateInput {
    /// The person's page, by its browser pane (`persons` in `browser_list`).
    pub pane: PaneId,
    /// Why you want it: the person's question shows it, as your own words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `browser_duplicate`: ask the person for a copy of their page in your worktree.
#[derive(Debug)]
pub struct BrowserDuplicate;

impl Operation for BrowserDuplicate {
    const NAME: &'static str = "browser_duplicate";
    const TITLE: &'static str = "Ask for a copy of the person's page";
    const SUBJECT: &'static [&'static str] = &["pane"];
    const SCOPE: Scope = Scope::Browser;
    const DESCRIPTION: &'static str = "Asks the person, in Koyo's window, for a copy of one of their own pages (pinned or loose: `persons` in browser_list) in your worktree. Allowed, a browser tab of your worktree opens at the address the page had when they were asked, beside your own tab, as a page you open does, and it's yours to drive: only the address is copied, signed in as your worktree's project. A page that changed meanwhile isn't copied. Needs the navigate scope too, as tab_open does. Refused without a worktree of your own (KOYO_WORKTREE, or a working folder in a checkout Koyo has). Cancel, or two minutes without an answer, fails user_declined: you can't ask about that page again for 10 minutes, and others wait a minute.";
    const HINTS: Hints = Hints::CHANGE;
    const ASKS: bool = true;
    type Input = BrowserDuplicateInput;
    type Output = PanePlace;

    fn summary(output: &PanePlace) -> String {
        format!("Copied into {}", output.summary())
    }
}
