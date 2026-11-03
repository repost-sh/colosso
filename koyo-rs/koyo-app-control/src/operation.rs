//! The contract every app operation implements, and what the catalog says about it.

use std::fmt::Debug;
use std::ops::Deref;
use std::time::Duration;

use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::{ControlError, Scope, object_schema};

/// How long the window's confirmation prompt waits for the person before the call fails
/// `user_declined`.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(120);

/// The argument a client adds to a changing operation's input so a repeat returns the first
/// result. It travels as the call frame's `requestId`, not in the operation's input.
pub const REQUEST_ID: &str = "requestId";

/// What a `requestId` must match.
pub const REQUEST_ID_PATTERN: &str = "^[A-Za-z0-9_-]{1,128}$";

/// What an operation does to the world, published as the MCP tool's annotations. Clients
/// decide what to approve on their own by them, so they are honest. `openWorld` is always
/// false: every operation acts on this app only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hints {
    /// The operation changes nothing.
    pub read_only: bool,
    /// The operation may delete or discard something.
    pub destructive: bool,
    /// Repeating the call with the same input has no further effect.
    pub idempotent: bool,
}

impl Hints {
    /// A read (R).
    pub const READ: Hints = Hints {
        read_only: true,
        destructive: false,
        idempotent: true,
    };
    /// A change that repeating has no further effect on (I).
    pub const IDEMPOTENT: Hints = Hints {
        read_only: false,
        destructive: false,
        idempotent: true,
    };
    /// A change that does more each time, such as opening a tab (C). A `requestId` makes a
    /// retry safe.
    pub const CHANGE: Hints = Hints {
        read_only: false,
        destructive: false,
        idempotent: false,
    };
    /// A change that deletes or discards something; doing it again has no further effect (D).
    pub const DESTRUCTIVE: Hints = Hints {
        read_only: false,
        destructive: true,
        idempotent: true,
    };
}

/// One app operation: a typed input and output, the scope it needs and how it behaves.
/// Implemented in a domain module under `ops`, then named in the registry (`command.rs`).
pub trait Operation {
    /// The tool name: `snake_case` `noun_verb` of `[a-z0-9_]`, at most 64 characters. Also
    /// the [`Command`](crate::Command) variant's tag.
    const NAME: &'static str;
    /// What a person calls it, in sentence case ("Delete worktree"). Relays publish it as
    /// the tool's title.
    const TITLE: &'static str;
    /// The input fields that say what a call acts on, in priority order, for a transcript
    /// to show after the title.
    const SUBJECT: &'static [&'static str] = &[];
    /// The scope every call needs.
    const SCOPE: Scope;
    /// The scopes a call needs as well when [`Operation::needs_also`] says so. Most
    /// operations need [`Operation::SCOPE`] alone.
    const ALSO: &'static [Scope] = &[];
    /// What it does, for a model choosing a tool: concise, with the rules that refuse it.
    const DESCRIPTION: &'static str;
    /// How it behaves.
    const HINTS: Hints;
    /// Whether the person confirms every call in the window before it runs. Calls that need
    /// `destructive` through [`Operation::ALSO`] are confirmed as well.
    const CONFIRM: bool = false;

    /// The arguments.
    type Input: Serialize + DeserializeOwned + JsonSchema + Debug + Clone + PartialEq + Send;
    /// The result.
    type Output: Serialize + DeserializeOwned + JsonSchema + Debug + Clone + PartialEq + Send;

    /// A one-line summary of a result, the text beside the structured content.
    fn summary(output: &Self::Output) -> String;

    /// Whether this call needs [`Operation::ALSO`] too, given its input.
    fn needs_also(input: &Self::Input) -> bool {
        let _ = input;
        false
    }
}

/// What the catalog says about an operation, without its schemas: cheap to look up by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    /// The tool name ([`Operation::NAME`]).
    pub name: &'static str,
    /// What a person calls it ([`Operation::TITLE`]).
    pub title: &'static str,
    /// The input fields a transcript shows ([`Operation::SUBJECT`]).
    pub subject: &'static [&'static str],
    /// The scope every call needs.
    pub scope: Scope,
    /// The scopes some calls need as well, depending on their input; the description says
    /// when.
    pub also: &'static [Scope],
    /// What it does.
    pub description: &'static str,
    /// How it behaves.
    pub hints: Hints,
    /// Whether the person confirms every call ([`Operation::CONFIRM`]).
    pub confirm: bool,
}

impl Info {
    /// The entry for `Op`.
    pub fn of<Op: Operation>() -> Info {
        Info {
            name: Op::NAME,
            title: Op::TITLE,
            subject: Op::SUBJECT,
            scope: Op::SCOPE,
            also: Op::ALSO,
            description: Op::DESCRIPTION,
            hints: Op::HINTS,
            confirm: Op::CONFIRM,
        }
    }

    /// Whether some call of it may wait for the person's confirmation: it's confirmed, or
    /// some input makes it need `destructive`.
    pub fn may_confirm(&self) -> bool {
        self.confirm || self.also.contains(&Scope::Destructive)
    }

    /// The description's first sentence, for one-line listings.
    pub fn short_description(&self) -> &'static str {
        let text = self.description;
        text.find(". ").map_or(text, |end| &text[..=end])
    }
}

/// An operation's catalog entry: its [`Info`] (reachable as fields of the descriptor) and
/// the schemas a relay publishes.
#[derive(Clone, Debug, PartialEq)]
pub struct Descriptor {
    /// What the catalog says about it.
    pub info: Info,
    /// The input's JSON Schema: an object taking no other properties. A changing operation
    /// also takes [`REQUEST_ID`].
    pub input_schema: Map<String, Value>,
    /// The output's JSON Schema, an object.
    pub output_schema: Map<String, Value>,
}

impl Descriptor {
    /// The entry for `Op`.
    pub fn of<Op: Operation>() -> Descriptor {
        let mut input_schema = object_schema::<Op::Input>();
        if !Op::HINTS.read_only
            && let Some(Value::Object(properties)) = input_schema.get_mut("properties")
        {
            properties.insert(
                REQUEST_ID.into(),
                serde_json::json!({
                    "type": "string",
                    "pattern": REQUEST_ID_PATTERN,
                    "description": "Makes a repeat return the first result instead of acting again. Reusing it with other input fails request_conflict."
                }),
            );
        }
        Descriptor {
            info: Info::of::<Op>(),
            input_schema,
            output_schema: object_schema::<Op::Output>(),
        }
    }
}

impl Deref for Descriptor {
    type Target = Info;

    fn deref(&self) -> &Info {
        &self.info
    }
}

/// Whether `name` is a valid tool name: 1 to 64 of `[a-z0-9_]`, starting with a letter.
pub fn valid_tool_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Whether `id` is a valid request ID: 1 to 128 of `[A-Za-z0-9_-]`.
pub fn valid_request_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Refuses a request id that isn't 1 to 128 letters, digits, underscores or hyphens.
///
/// # Errors
/// `invalid_input` for a malformed id.
pub fn check_request_id(id: Option<&str>) -> Result<(), ControlError> {
    match id {
        Some(id) if !valid_request_id(id) => Err(malformed_request_id()),
        _ => Ok(()),
    }
}

fn malformed_request_id() -> ControlError {
    ControlError::invalid_input(
        "requestId must be 1 to 128 letters, digits, underscores or hyphens",
    )
}

/// Takes [`REQUEST_ID`] out of a tool call's arguments, as a relay does before forwarding
/// the rest as the operation's input.
///
/// # Errors
/// `invalid_input` when it isn't a string matching [`REQUEST_ID_PATTERN`].
pub fn take_request_id(input: &mut Value) -> Result<Option<String>, ControlError> {
    let Some(arguments) = input.as_object_mut() else {
        return Ok(None);
    };
    match arguments.remove(REQUEST_ID) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) => check_request_id(Some(&id)).map(|()| Some(id)),
        Some(_) => Err(malformed_request_id()),
    }
}
