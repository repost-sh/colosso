//! The operation catalog (`docs/terminals.md` 8.1, 8.2, 8.4, 8.5): what a terminal can be
//! asked to do, each operation with its name, title, description, scope, hints and argument
//! schema. The MCP lists its tools from here, with no code of its own per tool, and
//! [`crate::exec::Executor`] carries them out.

pub mod args;

use schemars::JsonSchema;
use serde_json::Value;

/// What a caller may do, granted per client.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// List, read, wait, logs.
    Read,
    /// Open, send, claim, release, rename.
    Write,
    /// Signal, close exited terminals.
    Manage,
    /// Close a terminal whose program still runs.
    Destructive,
}

impl Scope {
    pub fn name(self) -> &'static str {
        match self {
            Scope::Read => "terminal.read",
            Scope::Write => "terminal.write",
            Scope::Manage => "terminal.manage",
            Scope::Destructive => "terminal.destructive",
        }
    }

    /// What an agent the app launched may do without asking.
    pub const AGENT: [Scope; 3] = [Scope::Read, Scope::Write, Scope::Manage];
}

/// What an operation does to the world, as the MCP's tool annotations say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hints {
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
}

const READS: Hints = Hints {
    read_only: true,
    destructive: false,
    idempotent: true,
};
const WRITES: Hints = Hints {
    read_only: false,
    destructive: false,
    idempotent: false,
};
const SETS: Hints = Hints {
    read_only: false,
    destructive: false,
    idempotent: true,
};
const ENDS: Hints = Hints {
    read_only: false,
    destructive: true,
    idempotent: true,
};

pub struct Operation {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub scope: Scope,
    pub hints: Hints,
    schema: fn() -> Value,
}

impl Operation {
    /// The JSON Schema of the operation's arguments.
    pub fn input_schema(&self) -> Value {
        (self.schema)()
    }
}

fn schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).unwrap_or_default()
}

/// Every operation, in the order the MCP lists them.
pub const CATALOG: &[Operation] = &[
    Operation {
        name: "terminal_list",
        title: "List terminals",
        description: "The terminals of a worktree (yours unless told otherwise): each one's \
                      handle, name, status, who controls it and its last screen line.",
        scope: Scope::Read,
        hints: READS,
        schema: schema::<args::ListArgs>,
    },
    Operation {
        name: "terminal_open",
        title: "Open a terminal",
        description: "Opens a terminal named `name` in a worktree, or returns the one that \
                      has the name already, and takes control of it when it's free. It \
                      appears as a tab in Colosso. `size` pins it at that size for every \
                      viewer while you're connected (`pinned`, or `pinned: null` and \
                      `pinRefused` when that was refused); with `claim: false` it's only the \
                      size it starts at.",
        scope: Scope::Write,
        hints: WRITES,
        schema: schema::<args::OpenArgs>,
    },
    Operation {
        name: "terminal_read",
        title: "Read a terminal",
        description: "The screen as text with its size, cursor and status (mode `screen`, the \
                      default), or the output written since a cursor (mode `stream`). Prefer \
                      the screen when the answer depends on how output looks.",
        scope: Scope::Read,
        hints: READS,
        schema: schema::<args::ReadArgs>,
    },
    Operation {
        name: "terminal_send",
        title: "Send input to a terminal",
        description: "Types text, presses named keys, pastes, presses Enter or Ctrl-C, then \
                      optionally waits and reads in the same call. Claims a free terminal \
                      first. `person_has_control` means the person took this terminal: don't \
                      retry or wait for it; work in another terminal.",
        scope: Scope::Write,
        hints: WRITES,
        schema: schema::<args::SendArgs>,
    },
    Operation {
        name: "terminal_wait",
        title: "Wait for a terminal",
        description: "Waits until a condition holds: idle, running, prompt, exit, needs_input, \
                      text or gone (a regex on the screen), stable, control, status_change. \
                      Running out of time answers `met: false`. Prefer this to fixed sleeps.",
        scope: Scope::Read,
        hints: READS,
        schema: schema::<args::WaitArgs>,
    },
    Operation {
        name: "terminal_claim",
        title: "Take control of a terminal",
        description: "Takes control so you may write. `takeover` displaces another agent, \
                      never a person: `person_has_control` means leave this terminal alone.",
        scope: Scope::Write,
        hints: SETS,
        schema: schema::<args::ClaimArgs>,
    },
    Operation {
        name: "terminal_release",
        title: "Give up control of a terminal",
        description: "Gives control up, so the terminal is free for others.",
        scope: Scope::Write,
        hints: SETS,
        schema: schema::<args::TerminalArgs>,
    },
    Operation {
        name: "terminal_control",
        title: "Who controls a terminal",
        description: "Who holds the terminal and until when, who watches, and its last few \
                      activity entries.",
        scope: Scope::Read,
        hints: READS,
        schema: schema::<args::TerminalArgs>,
    },
    Operation {
        name: "terminal_resize",
        title: "Resize a terminal",
        description: "Pins the terminal at `cols` × `rows` (2 to 1000 each) for every viewer, \
                      or with `fill: true` lets it fill the person's pane again, then waits \
                      for the screen to settle and reads it: a program redraws after the \
                      resize. `then: {}` skips the wait and the read. The size lasts while \
                      you are connected; the person sees it and may change it.",
        scope: Scope::Write,
        hints: SETS,
        schema: schema::<args::ResizeArgs>,
    },
    Operation {
        name: "terminal_rename",
        title: "Rename a terminal",
        description: "Changes a terminal's name, title or labels.",
        scope: Scope::Write,
        hints: SETS,
        schema: schema::<args::RenameArgs>,
    },
    Operation {
        name: "terminal_signal",
        title: "Signal a terminal's program",
        description: "Sends a signal (int, term, hup, kill, tstp, cont) to the program's \
                      process group.",
        scope: Scope::Manage,
        hints: WRITES,
        schema: schema::<args::SignalArgs>,
    },
    Operation {
        name: "terminal_close",
        title: "Close a terminal",
        description: "Closes a terminal and forgets it. One whose program still runs needs \
                      the person's confirmation in Colosso: stop the program first.",
        scope: Scope::Manage,
        hints: ENDS,
        schema: schema::<args::CloseArgs>,
    },
    Operation {
        name: "terminal_log",
        title: "Read a terminal's logs",
        description: "What people and agents did in a terminal (`activity`), the commands its \
                      shell ran with exit codes (`commands`), or its screenshots, read on \
                      from a cursor.",
        scope: Scope::Read,
        hints: READS,
        schema: schema::<args::LogArgs>,
    },
    Operation {
        name: "worktree_list",
        title: "List worktrees",
        description: "The worktrees that have terminals, with how many each has and how many \
                      wait for the person.",
        scope: Scope::Read,
        hints: READS,
        schema: schema::<args::WorktreeListArgs>,
    },
];

pub fn operation(name: &str) -> Option<&'static Operation> {
    CATALOG.iter().find(|operation| operation.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operation_is_named_as_mcp_tools_are_and_has_an_object_schema() {
        for operation in CATALOG {
            let named = operation.name.len() <= 64
                && (operation.name.bytes()).all(|b| b.is_ascii_lowercase() || b == b'_');
            assert!(named, "{}", operation.name);
            let schema = operation.input_schema();
            assert_eq!(schema["type"], "object", "{}", operation.name);
            assert_eq!(schema["additionalProperties"], false, "{}", operation.name);
        }
        let send = operation("terminal_send").unwrap().input_schema();
        assert!(send["properties"]["keys"].is_object());
        assert!(send["properties"]["requestId"].is_object());
        let wait = operation("terminal_wait").unwrap().input_schema();
        assert_eq!(wait["required"], serde_json::json!(["for"]));
    }

    #[test]
    fn resize_takes_a_size_or_fill_a_then_and_a_request_id() {
        let resize = operation("terminal_resize").unwrap();
        assert_eq!(resize.scope, Scope::Write);
        assert!(!resize.hints.read_only && resize.hints.idempotent);
        let schema = resize.input_schema();
        for field in ["terminal", "cols", "rows", "fill", "then", "requestId"] {
            assert!(schema["properties"][field].is_object(), "{field}: {schema}");
        }
        assert!(schema.get("required").is_none(), "{schema}");
        let parse = |args: Value| serde_json::from_value::<args::ResizeArgs>(args);
        let sized = parse(serde_json::json!({ "cols": 120, "rows": 40 })).unwrap();
        assert_eq!(
            (sized.cols, sized.rows, sized.fill),
            (Some(120), Some(40), None)
        );
        let quick = parse(serde_json::json!({ "fill": true, "then": {} })).unwrap();
        assert!(
            quick
                .then
                .is_some_and(|then| then.wait.is_none() && then.read.is_none())
        );
        assert!(parse(serde_json::json!({ "cols": 80, "rows": 24, "size": 1 })).is_err());
        assert!(parse(serde_json::json!({ "cols": 70000, "rows": 24 })).is_err());
    }
}
