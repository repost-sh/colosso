//! Receipts of changing calls made with a `requestId`, kept per client, the latest 256, for
//! the app's run: a repeat with the same input gets the first result instead of acting again,
//! and with other input fails `request_conflict`.

use std::collections::{HashMap, VecDeque};

use colosso_app_control::{Command, ControlError};
use serde_json::Value;

/// How many receipts each client keeps.
pub(crate) const KEPT: usize = 256;

/// One call's receipt.
struct Receipt {
    request_id: String,
    command: Command,
    /// The result once the call is done; `None` while it runs.
    result: Option<Result<Value, ControlError>>,
}

/// What a receipt says about a call about to be made.
#[derive(Debug, PartialEq)]
pub(crate) enum Lookup {
    /// No call had this `requestId`: make it, its receipt is kept as running.
    New,
    /// The same call was made: here is its result.
    Replay(Result<Value, ControlError>),
    /// Another call had this `requestId`, or this one still runs.
    Conflict(ControlError),
}

/// Every client's receipts, oldest first.
#[derive(Default)]
pub(crate) struct Receipts {
    clients: HashMap<String, VecDeque<Receipt>>,
}

impl Receipts {
    /// Looks up `request_id` among `client`'s receipts for `command`, keeping a running
    /// receipt for a call not made before.
    pub fn begin(&mut self, client: &str, request_id: &str, command: &Command) -> Lookup {
        let receipts = self.clients.entry(client.to_owned()).or_default();
        if let Some(receipt) = receipts
            .iter()
            .find(|receipt| receipt.request_id == request_id)
        {
            if receipt.command != *command {
                return Lookup::Conflict(ControlError::request_conflict(format!(
                    "The requestId {request_id} was used for another call; use a new one."
                )));
            }
            return match &receipt.result {
                Some(result) => Lookup::Replay(result.clone()),
                None => Lookup::Conflict(
                    ControlError::request_conflict(format!(
                        "The call with requestId {request_id} is still running."
                    ))
                    .retryable(true),
                ),
            };
        }
        if receipts.len() == KEPT {
            receipts.pop_front();
        }
        receipts.push_back(Receipt {
            request_id: request_id.to_owned(),
            command: command.clone(),
            result: None,
        });
        Lookup::New
    }

    /// Notes how `client`'s call `request_id` ended: its receipt keeps `result`, or goes when
    /// the call should be made again rather than repeated.
    pub fn finish(&mut self, client: &str, request_id: &str, result: &Result<Value, ControlError>) {
        let Some(receipts) = self.clients.get_mut(client) else {
            return;
        };
        let Some(at) = (receipts.iter()).position(|receipt| receipt.request_id == request_id)
        else {
            return;
        };
        if kept(result) {
            receipts[at].result = Some(result.clone());
        } else {
            receipts.remove(at);
        }
    }
}

/// Whether a call's result is its receipt's: everything but a refusal or a failure worth
/// trying again, which leave the call to be made afresh.
fn kept(result: &Result<Value, ControlError>) -> bool {
    use colosso_app_control::ErrorCode;
    match result {
        Ok(_) => true,
        Err(error) => {
            !error.retryable
                && !matches!(
                    error.code,
                    ErrorCode::UserDeclined
                        | ErrorCode::RequestConflict
                        | ErrorCode::ScopeDenied
                        | ErrorCode::ControlDisabled
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use colosso_app_control::ops::project::ProjectInput;

    use super::*;

    fn select(project: &str) -> Command {
        Command::ProjectSelect(ProjectInput {
            project: project.into(),
        })
    }

    #[test]
    fn a_repeat_gets_the_first_result_and_other_input_conflicts() {
        let mut receipts = Receipts::default();
        assert_eq!(receipts.begin("a", "r1", &select("x")), Lookup::New);
        let running = receipts.begin("a", "r1", &select("x"));
        assert!(matches!(running, Lookup::Conflict(error) if error.retryable));
        receipts.finish("a", "r1", &Ok(Value::from(1)));
        assert_eq!(
            receipts.begin("a", "r1", &select("x")),
            Lookup::Replay(Ok(Value::from(1)))
        );
        assert!(matches!(
            receipts.begin("a", "r1", &select("y")),
            Lookup::Conflict(error) if !error.retryable
        ));
        // Another client's requestIds are its own.
        assert_eq!(receipts.begin("b", "r1", &select("y")), Lookup::New);
    }

    #[test]
    fn a_declined_call_keeps_no_receipt() {
        let mut receipts = Receipts::default();
        assert_eq!(receipts.begin("a", "r1", &select("x")), Lookup::New);
        receipts.finish("a", "r1", &Err(ControlError::user_declined("no")));
        assert_eq!(receipts.begin("a", "r1", &select("y")), Lookup::New);
    }

    #[test]
    fn keeps_the_latest_receipts() {
        let mut receipts = Receipts::default();
        for n in 0..=KEPT {
            let id = format!("r{n}");
            assert_eq!(receipts.begin("a", &id, &select("x")), Lookup::New);
            receipts.finish("a", &id, &Ok(Value::Null));
        }
        // The first went, so its requestId is new again.
        assert_eq!(receipts.begin("a", "r0", &select("y")), Lookup::New);
        assert_eq!(
            receipts.begin("a", "r2", &select("x")),
            Lookup::Replay(Ok(Value::Null))
        );
    }
}
