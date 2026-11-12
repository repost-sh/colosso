//! Why a browser tool's command didn't reach its page or came back empty, in app control's
//! words: the page's lease, a page the engine hasn't open (which says whether trying again
//! helps), and a page that didn't answer in time.

use std::time::Duration;

use koyo_app_control::ops::browser::{DialogKind, PageDialog};
use koyo_app_control::{ControlError, ErrorCode};

use super::page::refused;
use crate::browser::agent::{AgentError, NotOpen, Unanswered};

/// Why a protocol command for pane `pane` didn't run.
pub(super) fn failed(error: AgentError, pane: u64) -> ControlError {
    match error {
        AgentError::Refused(why) => refused(why),
        AgentError::NotOpen(NotOpen::Starting) => ControlError::browser_unavailable(
            "The browser engine is still starting, so the page isn't open in it yet. Try \
             again shortly.",
        ),
        AgentError::NotOpen(NotOpen::Unshown) => unshown(pane),
        AgentError::NotOpen(NotOpen::StartPage) => ControlError::browser_unavailable(format!(
            "Pane {pane} shows the start page, which has no page loaded yet; browser_navigate \
             loads one."
        ))
        .retryable(false),
        AgentError::NotOpen(NotOpen::Failed(why)) => ControlError::browser_unavailable(format!(
            "The browser engine isn't running ({why}). The person can start it again with \
             Restart browser on the page."
        ))
        .retryable(false),
        AgentError::Asking(dialog) => ControlError::browser_unavailable(format!(
            "Pane {pane}'s page waits on its {} and does nothing else until it's answered: \
             answer it with browser_dialog (accept or dismiss).",
            asks(&dialog)
        ))
        .retryable(false)
        .with_details(serde_json::json!({ "dialog": dialog })),
    }
}

/// What a question of `kind` is called.
fn noun(kind: DialogKind) -> &'static str {
    match kind {
        DialogKind::Alert => "alert",
        DialogKind::Confirm => "confirm",
        DialogKind::Prompt => "prompt",
        DialogKind::Leave => "question before it's left",
        DialogKind::SignIn => "sign-in",
        DialogKind::Permission => "permission request",
        DialogKind::Certificate => "certificate warning",
    }
}

/// What `dialog` is and says, as a sentence's object.
fn asks(dialog: &PageDialog) -> String {
    let kind = noun(dialog.kind);
    if dialog.message.is_empty() {
        kind.to_owned()
    } else {
        format!("{kind} (“{}”)", dialog.message)
    }
}

/// The page asked `dialog`, or something it answered at once, while `method` ran: the command
/// waits behind it. One that acts likely opened it itself, and so landed.
pub(super) fn asked_meanwhile(
    method: &str,
    dialog: Option<&PageDialog>,
    acts: bool,
) -> ControlError {
    let what = dialog.map_or_else(
        || "a dialog".to_owned(),
        |dialog| format!("its {}", asks(dialog)),
    );
    let landed = if acts {
        " The act most likely opened it, so it landed: don't do it again."
    } else {
        ""
    };
    let error = ControlError::browser_unavailable(format!(
        "The page opened {what} while {method} ran, and waits on it: answer it with \
         browser_dialog (accept or dismiss), then go on.{landed}"
    ))
    .retryable(false);
    match dialog {
        Some(dialog) => error.with_details(serde_json::json!({ "dialog": dialog })),
        None => error,
    }
}

/// Why an agent's answer to pane `pane`'s question didn't go.
pub(super) fn unanswered(why: Unanswered, pane: u64) -> ControlError {
    match why {
        Unanswered::Refused(why) => refused(why),
        Unanswered::NothingAsked => ControlError::not_found(format!(
            "Pane {pane}'s page isn't asking anything; browser_list shows a page's dialog."
        )),
        Unanswered::PersonAnswers(kind) => ControlError::new(
            ErrorCode::PersonHasControl,
            format!(
                "Pane {pane}'s page asks the person a {}, theirs to answer. Ask them, or wait \
                 for them to.",
                noun(kind)
            ),
        ),
    }
}

/// Pane `pane`'s page loads only once Koyo's window draws its tab: a tab brought back at
/// launch, or the person's own never shown. The tools don't load it themselves
/// (`docs/browser.md` 6), and what the window shows is the person's to decide
/// (`docs/app-control.md` 5), so the agent asks them, or opens a page of its own.
pub(super) fn unshown(pane: u64) -> ControlError {
    ControlError::browser_unavailable(format!(
        "Pane {pane}'s page isn't loaded: a tab brought back at launch, or one never shown, \
         loads its page only once Koyo's window shows its tab, which is the person's to decide. \
         Open a page of your own with tab_open, or ask the person to show this one; show it \
         yourself (tab_select, pane_focus) only if they asked to see it."
    ))
    .retryable(false)
}

/// The page didn't answer `method` within `limit`. An act may have landed all the same, so
/// trying it again blind could do it twice; a read can simply be tried again.
pub(super) fn timed_out(method: &str, limit: Duration, acts: bool) -> ControlError {
    let waited = format!(
        "The page didn't answer {method} within {} s, so Koyo stopped waiting for it.",
        limit.as_secs()
    );
    if acts {
        return ControlError::timeout(format!(
            "{waited} It may have acted all the same: look at the page (browser_snapshot, \
             browser_read) before doing it again."
        ))
        .retryable(false);
    }
    ControlError::timeout(format!(
        "{waited} The page may be busy or stuck: try again, or reload it with browser_go."
    ))
}

/// The page, or the host for it, didn't carry out `method`, for the reason `why` gives. A
/// command Chromium takes for malformed is Koyo's to fix, and a page that closed stays
/// closed, so trying again helps neither; a page between documents, say, answers later.
pub(super) fn command_failed(method: &str, why: String) -> ControlError {
    if why.starts_with("Invalid parameters") || why.ends_with("wasn't found") {
        return ControlError::internal(format!(
            "Chromium refused the {method} command Koyo sent it: {why}"
        ));
    }
    let closed = why == "The page is closed.";
    ControlError::browser_unavailable(why).retryable(!closed)
}

#[cfg(test)]
mod tests {
    use koyo_app_control::ErrorCode;

    use super::*;

    #[test]
    fn a_page_never_shown_says_to_ask_the_person_and_a_starting_engine_to_try_again() {
        let restored = unshown(7);
        assert_eq!(restored.code, ErrorCode::BrowserUnavailable);
        assert!(!restored.retryable, "trying again never loads it");
        assert!(restored.message.contains("Pane 7"));
        assert!(restored.message.contains("tab_open"));
        assert!(restored.message.contains("ask the person"));
        assert!(
            !restored.message.contains("app_focus"),
            "the window is the person's"
        );
        assert!(!restored.message.contains("Try again"));
        assert_eq!(
            failed(AgentError::NotOpen(NotOpen::Unshown), 7),
            restored,
            "a page never shown says the same"
        );

        let starting = failed(AgentError::NotOpen(NotOpen::Starting), 7);
        assert_eq!(starting.code, ErrorCode::BrowserUnavailable);
        assert!(starting.retryable);
        assert!(starting.message.contains("Try again shortly"));
        assert!(!starting.message.contains("tab_select"));
    }

    #[test]
    fn an_act_that_times_out_may_have_landed_and_a_read_can_be_tried_again() {
        let click = timed_out("Input.dispatchMouseEvent", Duration::from_secs(30), true);
        assert_eq!(click.code, ErrorCode::Timeout);
        assert!(!click.retryable, "it may have acted");
        assert!(click.message.contains("within 30 s"));
        assert!(click.message.contains("may have acted"));
        let shot = timed_out("Page.captureScreenshot", Duration::from_secs(30), false);
        assert!(shot.retryable);
        assert!(shot.message.contains("Page.captureScreenshot within 30 s"));
        assert!(shot.message.contains("try again"));
    }

    #[test]
    fn a_command_chromium_refuses_or_a_closed_page_isnt_retried() {
        let malformed = command_failed("Input.dispatchKeyEvent", "Invalid parameters".into());
        assert_eq!(
            (malformed.code, malformed.retryable),
            (ErrorCode::Internal, false)
        );
        let unknown = command_failed("Page.nope", "'Page.nope' wasn't found".into());
        assert_eq!(
            (unknown.code, unknown.retryable),
            (ErrorCode::Internal, false)
        );
        let closed = command_failed("Runtime.evaluate", "The page is closed.".into());
        assert_eq!(
            (closed.code, closed.retryable),
            (ErrorCode::BrowserUnavailable, false)
        );
        let between = command_failed(
            "Runtime.evaluate",
            "Cannot find context with specified id".into(),
        );
        assert_eq!(
            (between.code, between.retryable),
            (ErrorCode::BrowserUnavailable, true)
        );
    }
}
