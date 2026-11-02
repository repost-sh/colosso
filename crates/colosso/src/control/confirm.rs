//! The person confirming a destructive call in the window, for every caller: a prompt naming
//! the client and exactly what the call would do, Cancel or two minutes without an answer
//! declining it. A client has one such prompt open at a time.

use std::time::Duration;

use colosso_app_control::ControlError;
use futures::channel::oneshot;
use futures::future::{Either, select};
use gpui_kit::{App, AsyncApp, PromptLevel, Task};

use super::activity::Decision;
use super::exec::{main_shell, window_closed};
use super::{Caller, Control};

/// What the person is asked: the question, what the call would do in detail, and the button
/// that lets it go ahead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Question {
    pub message: String,
    pub detail: Option<String>,
    pub action: &'static str,
}

/// A prompt open for a client, by its number.
pub(crate) struct Waiting {
    number: u64,
}

/// Asks the person `question` for `caller`'s call; done with their decision. Refused while
/// another call of the caller's waits on them, or with the window closed.
pub(crate) fn ask(
    caller: &Caller,
    question: &Question,
    cx: &mut App,
) -> Result<Task<Decision>, ControlError> {
    let key = caller.key();
    if Control::get(cx).confirmations.contains_key(&key) {
        return Err(ControlError::confirmation_pending());
    }
    let (window, _) = main_shell(cx).ok_or_else(window_closed)?;
    let answer = window
        .update(cx, |_, window, cx| {
            window.prompt(
                PromptLevel::Warning,
                &question.message,
                question.detail.as_deref(),
                &[question.action, "Cancel"],
                cx,
            )
        })
        .map_err(|error| {
            ControlError::internal(format!("Colosso couldn't ask the person: {error}"))
        })?;
    let number = Control::number(cx);
    Control::get_mut(cx)
        .confirmations
        .insert(key.clone(), Waiting { number });
    let timeout = Control::get(cx).timeouts.confirm;
    Ok(cx.spawn(async move |cx: &mut AsyncApp| {
        let decision = answered(answer, timeout, cx).await;
        cx.update(|cx| {
            let waiting = &mut Control::get_mut(cx).confirmations;
            if waiting
                .get(&key)
                .is_some_and(|waiting| waiting.number == number)
            {
                waiting.remove(&key);
            }
        });
        decision
    }))
}

/// How the person answered a prompt whose first button allows: denied by any other button,
/// timed out after `timeout` or when the prompt went without an answer.
pub(crate) async fn answered(
    answer: oneshot::Receiver<usize>,
    timeout: Duration,
    cx: &AsyncApp,
) -> Decision {
    let timer = Box::pin(cx.background_executor().timer(timeout));
    match select(answer, timer).await {
        Either::Left((Ok(0), _)) => Decision::Allowed,
        Either::Left((Ok(_), _)) => Decision::Denied,
        Either::Left((Err(_), _)) | Either::Right(_) => Decision::TimedOut,
    }
}
