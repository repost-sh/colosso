//! `koyo app`: the app's operations from a shell (`docs/app-control.md` 6, CLI).
//!
//! `koyo app help` lists the operations from the catalog, and `koyo app help <op>` shows
//! one with its input schema. `koyo app <op> [--json '{…}'] [--request-id ID] [--client
//! NAME]` calls it on the running app's control socket and prints its result as JSON; a
//! failure prints the error envelope `{"error": {…}}`, also to stdout, and exits 1.

use std::fmt::Write as _;
use std::io::Write;
use std::process::ExitCode;

use serde_json::Value;

use crate::{
    ControlError, Endpoint, Info, catalog, info, infos, take_request_id, valid_request_id,
};

/// The name the CLI says hello with, unless `--client` gives another.
pub const CLIENT_NAME: &str = "koyo app";

/// What a command line asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Invocation {
    /// The list of operations, or one operation's help.
    Help(Option<String>),
    /// A call of an operation.
    Call {
        /// The operation's name.
        operation: String,
        /// Its input, an object.
        input: Value,
        /// The call's `requestId`, from `--request-id` or the input's own.
        request_id: Option<String>,
        /// The name to say hello with.
        client: String,
    },
}

/// Runs `koyo app` with the arguments after `app`, printing to stdout.
pub fn cli_main(args: impl IntoIterator<Item = String>) -> ExitCode {
    let mut out = std::io::stdout().lock();
    let code = match parse(args) {
        Ok(invocation) => run(&invocation, Endpoint::from_env, &mut out),
        Err(error) => fail(&mut out, &error),
    };
    ExitCode::from(code)
}

/// Reads the arguments after `app`.
///
/// # Errors
/// `invalid_input` for an unknown or repeated flag, a flag without its value, an argument
/// too many, `--json` that isn't a JSON object, or a malformed request ID.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Invocation, ControlError> {
    let mut args = args.into_iter();
    let operation = match args.next() {
        None => return Ok(Invocation::Help(None)),
        Some(word) if matches!(word.as_str(), "help" | "--help" | "-h") => {
            let topic = args.next();
            if let Some(extra) = args.next() {
                return Err(usage(&format!("`{extra}` is one argument too many.")));
            }
            return Ok(Invocation::Help(topic));
        }
        Some(flag) if flag.starts_with('-') => {
            return Err(usage(&format!("`{flag}` comes after an operation's name.")));
        }
        Some(operation) => operation,
    };
    let mut json = None;
    let mut request_id = None;
    let mut client = None;
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => {
                (flag.to_owned(), Some(value.to_owned()))
            }
            _ => (arg, None),
        };
        let slot = match flag.as_str() {
            "--help" | "-h" => return Ok(Invocation::Help(Some(operation))),
            "--json" => &mut json,
            "--request-id" => &mut request_id,
            "--client" => &mut client,
            _ => return Err(usage(&format!("`{flag}` isn't an option of koyo app."))),
        };
        if slot.is_some() {
            return Err(usage(&format!("`{flag}` is given twice.")));
        }
        let value = inline
            .or_else(|| args.next())
            .ok_or_else(|| usage(&format!("`{flag}` needs a value.")))?;
        *slot = Some(value);
    }
    let mut input = match json {
        None => Value::Object(serde_json::Map::new()),
        Some(text) => match serde_json::from_str::<Value>(&text) {
            Ok(value @ Value::Object(_)) => value,
            Ok(_) => return Err(ControlError::invalid_input("--json must be a JSON object.")),
            Err(error) => {
                return Err(ControlError::invalid_input(format!(
                    "--json isn't valid JSON: {error}"
                )));
            }
        },
    };
    let inner = take_request_id(&mut input)?;
    let request_id = match (request_id, inner) {
        (Some(_), Some(_)) => {
            return Err(usage(
                "The request ID is given twice, in --request-id and in --json.",
            ));
        }
        (Some(id), None) if !valid_request_id(&id) => {
            return Err(ControlError::invalid_input(
                "--request-id must be 1 to 128 letters, digits, underscores or hyphens.",
            ));
        }
        (id, inner) => id.or(inner),
    };
    let client = client.unwrap_or_else(|| CLIENT_NAME.to_owned());
    if client.trim().is_empty() {
        return Err(usage("--client needs a name."));
    }
    Ok(Invocation::Call {
        operation,
        input,
        request_id,
        client,
    })
}

/// Carries out `invocation`, on the socket `endpoint` finds, printing to `out`. Answers the
/// exit code: 0, or 1 after printing an error envelope.
pub fn run(
    invocation: &Invocation,
    endpoint: impl FnOnce() -> Result<Endpoint, ControlError>,
    out: &mut impl Write,
) -> u8 {
    match invocation {
        Invocation::Help(None) => {
            let _ = out.write_all(help().as_bytes());
            0
        }
        Invocation::Help(Some(name)) => match operation_help(name) {
            Ok(text) => {
                let _ = out.write_all(text.as_bytes());
                0
            }
            Err(error) => fail(out, &error),
        },
        Invocation::Call {
            operation,
            input,
            request_id,
            client,
        } => match call(operation, input, request_id.as_deref(), client, endpoint) {
            Ok(output) => {
                let text = serde_json::to_string_pretty(&output).unwrap_or_default();
                let _ = writeln!(out, "{text}");
                0
            }
            Err(error) => fail(out, &error),
        },
    }
}

#[cfg(unix)]
fn call(
    operation: &str,
    input: &Value,
    request_id: Option<&str>,
    client: &str,
    endpoint: impl FnOnce() -> Result<Endpoint, ControlError>,
) -> Result<Value, ControlError> {
    known(operation)?;
    let hello = crate::Hello::from_env(crate::ClientInfo {
        name: client.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    });
    let client = crate::Client::connect(&endpoint()?, &hello)?;
    client.call(operation, input.clone(), request_id)
}

#[cfg(not(unix))]
fn call(
    operation: &str,
    _: &Value,
    _: Option<&str>,
    _: &str,
    _: impl FnOnce() -> Result<Endpoint, ControlError>,
) -> Result<Value, ControlError> {
    known(operation)?;
    Err(ControlError::internal("koyo app needs a Unix system."))
}

/// The operation named `name`.
fn known(name: &str) -> Result<Info, ControlError> {
    info(name).ok_or_else(|| {
        let hint = if name == crate::APP_LAUNCH {
            " app_launch is the MCP relay's own tool; open Koyo instead."
        } else {
            " `koyo app help` lists the operations."
        };
        ControlError::invalid_input(format!("Koyo has no operation named `{name}`.{hint}"))
    })
}

/// Prints `error`'s envelope and answers exit code 1.
fn fail(out: &mut impl Write, error: &ControlError) -> u8 {
    let text = serde_json::to_string_pretty(&error.envelope()).unwrap_or_default();
    let _ = writeln!(out, "{text}");
    1
}

fn usage(message: &str) -> ControlError {
    ControlError::invalid_input(format!("{message} `koyo app help` shows how it's used."))
}

/// The list of operations: each one's name, scope, title and first sentence.
pub fn help() -> String {
    let mut text = String::from(
        "Drives the running Koyo window: projects, worktrees, tabs, panes, files, browser\n\
         pages and settings. Each call prints its result as JSON; a failure prints\n\
         {\"error\": {…}} and exits 1.\n\n\
         Usage: koyo app <operation> [--json '{…}'] [--request-id ID] [--client NAME]\n\
         \x20      koyo app help [<operation>]\n\nOperations:\n",
    );
    for info in infos() {
        let _ = writeln!(
            text,
            "  {:<20} {:<12} {}. {}",
            info.name,
            info.scope.id(),
            info.title,
            info.short_description()
        );
    }
    text
}

/// One operation's help: its title, scope, behaviour, description and input schema.
///
/// # Errors
/// `invalid_input` when no operation has that name.
pub fn operation_help(name: &str) -> Result<String, ControlError> {
    known(name)?;
    let Some(descriptor) = catalog().into_iter().find(|d| d.name == name) else {
        return Err(ControlError::invalid_input(format!(
            "Koyo has no operation named `{name}`."
        )));
    };
    let mut text = format!(
        "{}: {}\n\nScope: {}",
        descriptor.name,
        descriptor.title,
        descriptor.scope.id()
    );
    for scope in descriptor.also {
        let _ = write!(text, ", and {} for some inputs", scope.id());
    }
    let hints = descriptor.hints;
    let behaviour = match (hints.read_only, hints.destructive, hints.idempotent) {
        (true, _, _) => "reads only",
        (false, true, _) => "destructive",
        (false, false, true) => "changes, idempotent",
        (false, false, false) => "changes; --request-id makes a repeat safe",
    };
    let _ = write!(text, "\nBehaviour: {behaviour}");
    if descriptor.confirm {
        text.push_str("; the person confirms each call in the window");
    }
    let schema = serde_json::to_string_pretty(&descriptor.input_schema).unwrap_or_default();
    let input = if hints.read_only {
        "Input (--json):"
    } else {
        "Input (--json; its requestId may go in --request-id instead):"
    };
    let _ = write!(
        text,
        "\n\n{}\n\n{input}\n{schema}\n",
        descriptor.description
    );
    Ok(text)
}
