//! What every call goes through, on the UI thread: the gate, the receipt of a changing call
//! with a `requestId`, the person's confirmation of a destructive call, the executor, the
//! activity log, and the answer.

use colosso_app_control::{Command, ControlError};
use gpui_kit::{App, AsyncApp};
use serde_json::Value;

use super::activity::{ActivityKind, Decision};
use super::exec::{self, Exec};
use super::receipts::Lookup;
use super::{Answer, Caller, Control, confirm, gate};

/// A call on its way through.
pub(crate) struct Pending {
    pub caller: Caller,
    pub request_id: Option<String>,
    pub command: Command,
    pub reply: Answer<Value>,
}

/// Whether `command` changes anything, so it keeps a receipt and is logged.
fn changes(command: &Command) -> bool {
    !command.hints().read_only
}

/// Takes a call through to its answer.
pub(crate) fn call(pending: Pending, cx: &mut App) {
    if let Err(error) = gate::check(&pending.caller, &pending.command, cx) {
        return finish(pending, None, Err(error), cx);
    }
    let receipt = (pending.request_id.clone()).filter(|_| changes(&pending.command));
    if let Some(request_id) = &receipt {
        let key = pending.caller.key();
        match Control::get_mut(cx)
            .receipts
            .begin(&key, request_id, &pending.command)
        {
            Lookup::New => {}
            // A repeat acts no more, so it isn't logged again either.
            Lookup::Replay(result) => return (pending.reply)(result),
            Lookup::Conflict(error) => return finish(pending, None, Err(error), cx),
        }
    }
    if pending.command.confirmed() {
        confirm_then_run(pending, receipt, cx);
    } else {
        run(pending, receipt, cx);
    }
}

/// Asks the person to confirm the call, then runs it if they do and the caller may still
/// make it.
fn confirm_then_run(pending: Pending, receipt: Option<String>, cx: &mut App) {
    let questioning = exec::question(&pending.caller, &pending.command, cx);
    cx.spawn(async move |cx: &mut AsyncApp| {
        let question = match questioning.await {
            Ok(question) => question,
            Err(error) => return cx.update(|cx| finish(pending, receipt, Err(error), cx)),
        };
        let asking = cx.update(|cx| confirm::ask(&pending.caller, &question, cx));
        let decision = match asking {
            Ok(asking) => asking.await,
            Err(error) => return cx.update(|cx| finish(pending, receipt, Err(error), cx)),
        };
        cx.update(|cx| {
            let operation = pending.command.name().to_owned();
            let kind = ActivityKind::Confirmation {
                operation,
                question: question.message.clone(),
                decision,
            };
            Control::get_mut(cx)
                .activity
                .record(pending.caller.key(), kind);
            let declined = match decision {
                Decision::Allowed => match gate::check(&pending.caller, &pending.command, cx) {
                    Ok(()) => return run(pending, receipt, cx),
                    Err(error) => error,
                },
                Decision::Denied => ControlError::user_declined(format!(
                    "The person cancelled it: {}.",
                    question.message
                )),
                Decision::TimedOut => ControlError::user_declined(format!(
                    "The person didn't confirm it in time: {}.",
                    question.message
                )),
            };
            finish(pending, receipt, Err(declined), cx);
        });
    })
    .detach();
}

/// Runs the call's executor.
fn run(pending: Pending, receipt: Option<String>, cx: &mut App) {
    match exec::run(&pending.caller, pending.command.clone(), cx) {
        Exec::Ready(result) => finish(pending, receipt, result, cx),
        Exec::Later(running) => cx
            .spawn(async move |cx: &mut AsyncApp| {
                let result = running.await;
                cx.update(|cx| finish(pending, receipt, result, cx));
            })
            .detach(),
    }
}

/// Logs a changing call, settles its receipt, and answers it.
fn finish(
    pending: Pending,
    receipt: Option<String>,
    result: Result<Value, ControlError>,
    cx: &mut App,
) {
    let Pending {
        caller,
        command,
        reply,
        ..
    } = pending;
    let control = Control::get_mut(cx);
    let key = caller.key();
    if changes(&command) {
        let outcome = match &result {
            Ok(output) => Ok(command
                .summary(output)
                .unwrap_or_else(|| command.name().to_owned())),
            Err(error) => Err(error.clone()),
        };
        let operation = command.name().to_owned();
        control
            .activity
            .record(key.clone(), ActivityKind::Call { operation, outcome });
    }
    if let Some(request_id) = receipt {
        control.receipts.finish(&key, &request_id, &result);
    }
    reply(result);
}
