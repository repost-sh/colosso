//! Why a browser tool's command didn't reach its page or came back empty, in app control's
//! words: the page's lease, a page the engine hasn't open (which says whether trying again
//! helps), and a page that didn't answer in time.

use std::time::Duration;

use koyo_app_control::ControlError;

use super::page::refused;
use crate::browser::agent::{AgentError, NotOpen};

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
    }
}

/// Pane `pane`'s page loads only once Koyo's window draws its tab: a tab brought back at
/// launch, or one never drawn, as a shown tab isn't while the window is minimized or on
/// another desktop. The tools don't load it themselves (`docs/browser.md` 6), so trying
/// again alone never helps.
pub(super) fn unshown(pane: u64) -> ControlError {
    ControlError::browser_unavailable(format!(
        "Pane {pane}'s page isn't loaded: a tab brought back at launch, or one never shown, \
         loads its page only once Koyo's window shows its tab. Show it with tab_select or \
         pane_focus (and app_focus when the window is minimized or on another desktop), or \
         open a new page with tab_open."
    ))
    .retryable(false)
}

/// The page didn't answer `method` within `limit`.
pub(super) fn timed_out(method: &str, limit: Duration) -> ControlError {
    ControlError::timeout(format!(
        "The page didn't answer {method} within {} s, so Koyo stopped waiting for it. The page \
         may be busy, or stuck on a script or a dialog: try again, or reload it with \
         browser_go.",
        limit.as_secs()
    ))
}

#[cfg(test)]
mod tests {
    use koyo_app_control::ErrorCode;

    use super::*;

    #[test]
    fn a_page_never_shown_says_to_show_its_tab_and_a_starting_engine_to_try_again() {
        let restored = unshown(7);
        assert_eq!(restored.code, ErrorCode::BrowserUnavailable);
        assert!(!restored.retryable, "trying again never loads it");
        assert!(restored.message.contains("Pane 7"));
        assert!(restored.message.contains("tab_select or pane_focus"));
        assert!(restored.message.contains("tab_open"));
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
    fn a_page_that_does_not_answer_times_out_saying_so() {
        let late = timed_out("Page.captureScreenshot", Duration::from_secs(30));
        assert_eq!(late.code, ErrorCode::Timeout);
        assert!(late.retryable);
        assert!(late.message.contains("Page.captureScreenshot within 30 s"));
    }
}
