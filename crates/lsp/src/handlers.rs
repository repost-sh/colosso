//! The server's requests to us: answered by a handler the owner registered, or else by a
//! default that keeps the server going.
//!
//! Capability registrations are always recorded here, whatever else is registered, since the
//! owner reads them back rather than answering them.

use std::marker::PhantomData;
use std::sync::{Arc, mpsc};

use lsp_types::error_codes::REQUEST_FAILED;
use lsp_types::notification::{DidChangeWatchedFiles, Notification};
use lsp_types::request::{
    ApplyWorkspaceEdit, RegisterCapability, Request, ShowDocument, ShowMessageRequest,
    UnregisterCapability, WorkDoneProgressCreate, WorkspaceConfiguration,
};
use lsp_types::{
    ConfigurationParams, DidChangeWatchedFilesRegistrationOptions, FileSystemWatcher, Registration,
    RegistrationParams, UnregistrationParams,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use serde_json::{Value, json};

use crate::client::Event;
use crate::message::{self, Payload, ResponseError};
use crate::outgoing::Peer;
use crate::shared::{Handler, Shared, lock};

/// Sends the answer to one server request, exactly once: a responder dropped unanswered
/// answers with the method's default, so the server is never left waiting.
pub(crate) struct Responder {
    id: Box<RawValue>,
    method: String,
    out: mpsc::Sender<Vec<u8>>,
    /// Whether an owner's handler took the request, which changes what a silence means.
    handled: bool,
    answered: bool,
}

impl Responder {
    pub(crate) fn new(id: &RawValue, method: &str, out: mpsc::Sender<Vec<u8>>) -> Responder {
        Responder {
            id: id.to_owned(),
            method: method.to_owned(),
            out,
            handled: false,
            answered: false,
        }
    }

    pub(crate) fn send<T: Serialize>(mut self, outcome: Result<&T, &ResponseError>) {
        self.answered = true;
        self.write(outcome);
    }

    fn write<T: Serialize>(&self, outcome: Result<&T, &ResponseError>) {
        match message::response(&self.id, outcome) {
            // A send fails only once the writer is gone with the server; nobody is waiting.
            Ok(body) => drop(self.out.send(body)),
            Err(error) => log::error!("couldn't write the answer to {}: {error}", self.method),
        }
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        if !self.answered {
            self.write(fallback(&self.method, self.handled).as_ref());
        }
    }
}

/// The owner's way to answer one request from the server, now or later, from any thread.
/// Dropped without an answer, it answers with the request's default ("not applied", "not
/// shown"), or an error where there is none.
pub struct Reply<T> {
    responder: Responder,
    result: PhantomData<fn(T)>,
}

impl<T: Serialize> Reply<T> {
    pub fn send(self, result: Result<T, ResponseError>) {
        match result {
            Ok(result) => self.responder.send(Ok(&result)),
            Err(error) => self.responder.send::<T>(Err(&error)),
        }
    }
}

/// Wraps a typed handler for requests of kind `R`.
pub(crate) fn typed<R: Request>(
    handler: impl Fn(R::Params, Reply<R::Result>, &Peer) + Send + Sync + 'static,
) -> Handler {
    Arc::new(
        move |params: &Payload, mut responder: Responder, peer: &Peer| {
            responder.handled = true;
            match params.parse::<R::Params>() {
                Ok(params) => {
                    let reply = Reply {
                        responder,
                        result: PhantomData,
                    };
                    handler(params, reply, peer);
                }
                Err(error) => responder.send::<()>(Err(&invalid(&error))),
            }
        },
    )
}

/// Answers the server's request, on the reader thread.
pub(crate) fn answer(shared: &Arc<Shared>, params: &Payload, responder: Responder) {
    match responder.method.as_str() {
        RegisterCapability::METHOD => {
            let result = parse(params).map(|params| register(shared, params));
            responder.send(result.as_ref());
        }
        UnregisterCapability::METHOD => {
            let result = parse(params).map(|params| unregister(shared, &params));
            responder.send(result.as_ref());
        }
        method => match shared.handler(method) {
            Some(handler) => handler(params, responder, &Peer::new(shared)),
            None if method == WorkspaceConfiguration::METHOD => {
                let result = parse(params).map(|params| configuration(&shared.settings, &params));
                responder.send(result.as_ref());
            }
            // The rest answer with their defaults as the responder drops.
            None => drop(responder),
        },
    }
}

/// Each requested section of `settings`, found by its dotted path, or `null`.
pub(crate) fn configuration(settings: &Value, params: &ConfigurationParams) -> Vec<Value> {
    params
        .items
        .iter()
        .map(|item| {
            let Some(section) = &item.section else {
                return settings.clone();
            };
            section
                .split('.')
                .try_fold(settings, |value, key| value.get(key))
                .cloned()
                .unwrap_or(Value::Null)
        })
        .collect()
}

fn register(shared: &Shared, params: RegistrationParams) {
    {
        let mut registrations = lock(&shared.registrations);
        for registration in params.registrations {
            registrations.retain(|existing| existing.id != registration.id);
            registrations.push(registration);
        }
    }
    shared.emit(&Event::RegistrationsChanged);
}

fn unregister(shared: &Shared, params: &UnregistrationParams) {
    lock(&shared.registrations).retain(|registration| {
        !params
            .unregisterations
            .iter()
            .any(|gone| gone.id == registration.id)
    });
    shared.emit(&Event::RegistrationsChanged);
}

/// The files every `workspace/didChangeWatchedFiles` registration asks to be told about.
pub(crate) fn watched_files(registrations: &[Registration]) -> Vec<FileSystemWatcher> {
    registrations
        .iter()
        .filter(|registration| registration.method == DidChangeWatchedFiles::METHOD)
        .filter_map(|registration| registration.register_options.clone())
        .filter_map(|options| {
            serde_json::from_value::<DidChangeWatchedFilesRegistrationOptions>(options).ok()
        })
        .flat_map(|options| options.watchers)
        .collect()
}

/// What a request nobody answered gets.
fn fallback(method: &str, handled: bool) -> Result<Value, ResponseError> {
    match method {
        ApplyWorkspaceEdit::METHOD => Ok(json!({
            "applied": false,
            "failureReason": "No editor took the edit.",
        })),
        ShowDocument::METHOD => Ok(json!({"success": false})),
        // Progress is shown however it's reported; a dismissed message chose no action.
        WorkDoneProgressCreate::METHOD | ShowMessageRequest::METHOD => Ok(Value::Null),
        _ if handled => Err(ResponseError::new(
            REQUEST_FAILED,
            format!("The client dropped {method} without answering."),
        )),
        _ => Err(ResponseError::new(
            ResponseError::METHOD_NOT_FOUND,
            format!("The client doesn't handle {method}."),
        )),
    }
}

fn parse<T: DeserializeOwned>(params: &Payload) -> Result<T, ResponseError> {
    params.parse().map_err(|error| invalid(&error))
}

fn invalid(error: &serde_json::Error) -> ResponseError {
    ResponseError::new(ResponseError::INVALID_PARAMS, error.to_string())
}

#[cfg(test)]
mod tests {
    use lsp_types::ConfigurationItem;
    use serde_json::json;

    use super::*;

    #[test]
    fn configuration_sections_are_found_by_dotted_path() {
        let settings = json!({"rust-analyzer": {"cargo": {"targetDir": true}}});
        let item = |section: Option<&str>| ConfigurationItem {
            scope_uri: None,
            section: section.map(str::to_owned),
        };
        let params = ConfigurationParams {
            items: vec![
                item(Some("rust-analyzer")),
                item(Some("rust-analyzer.cargo.targetDir")),
                item(Some("typescript")),
                item(None),
            ],
        };
        assert_eq!(
            configuration(&settings, &params),
            vec![
                json!({"cargo": {"targetDir": true}}),
                json!(true),
                Value::Null,
                settings.clone(),
            ]
        );
    }
}
