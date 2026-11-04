//! Carrying out the catalog's operations (`docs/terminals.md` 8.3, 8.5) over a connection to
//! the daemon, as the caller that connection is: the terminal it means when it names none,
//! what it was granted, and the answer a repeated request id gets back.

mod log;
mod read;
mod resize;
mod shape;

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::catalog::args::{
    ClaimArgs, CloseArgs, CommandArg, IfExistsArg, ListArgs, OpenArgs, RenameArgs, SignalArgs,
    TerminalArgs,
};
use crate::catalog::{Scope, operation};
use crate::{
    Actor, ClientError, Connection, Control, ErrorCode, ErrorDetails, Handle, IfExists, LogKind,
    OpenRequest, ProtocolError, RequestBody, ResponseBody, Selector, Size, Status, TermDir,
    TerminalSummary,
};

/// An operation's answer: what it found, and a line saying so.
#[derive(Clone, Debug, PartialEq)]
pub struct Done {
    pub structured: Value,
    pub summary: String,
}

/// How many answers are kept for repeated request ids.
const RECEIPTS: usize = 256;

pub struct Executor {
    connection: Arc<Connection>,
    dir: TermDir,
    /// Who the daemon takes the connection to be.
    actor: Actor,
    grants: Vec<Scope>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// The terminal last acted on: what a call naming none means, while it's in the worktree.
    last: Option<Handle>,
    /// Answers by request id, with the arguments that got them.
    receipts: VecDeque<(String, Value, Done)>,
}

impl Executor {
    pub fn new(connection: Arc<Connection>, dir: TermDir, actor: Actor, grants: &[Scope]) -> Self {
        Executor {
            connection,
            dir,
            actor,
            grants: grants.to_vec(),
            state: Mutex::default(),
        }
    }

    /// The terminal `terminal` names, with who holds it and its size.
    pub fn describe(&self, terminal: &str) -> Result<Value, ProtocolError> {
        let summary = self.terminal(Some(terminal))?;
        let size = summary.record.size;
        Ok(json!({
            "terminal": shape::summary(&summary),
            "control": shape::control(&summary.record.control, &self.actor),
            "size": { "cols": size.cols, "rows": size.rows },
        }))
    }

    pub fn grants(&self) -> &[Scope] {
        &self.grants
    }

    /// Whether the daemon connection ended, so a new one is needed.
    pub fn is_closed(&self) -> bool {
        self.connection.is_closed()
    }

    /// Ends the daemon connection, though others still hold it: the daemon ends this
    /// client's pins and leases, and whoever reads it learns it's closed.
    pub fn disconnect(&self) {
        self.connection.close();
    }

    /// Carries out operation `name` with `args`, as the MCP's `tools/call` asks.
    pub fn call(&self, name: &str, args: Value) -> Result<Done, ProtocolError> {
        let operation = operation(name)
            .ok_or_else(|| invalid(format!("there is no operation called {name}")))?;
        if !self.grants.contains(&operation.scope) {
            return Err(ProtocolError::new(
                ErrorCode::ScopeDenied,
                format!(
                    "{name} needs {}, which this client wasn't granted",
                    operation.scope.name()
                ),
            ));
        }
        let args = if args.is_null() { json!({}) } else { args };
        let request_id = args
            .get("requestId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if let Some(id) = &request_id
            && let Some(done) = self.receipt(id, &args)?
        {
            return Ok(done);
        }
        let done = match name {
            "terminal_list" => self.list(parse(&args)?),
            "terminal_open" => self.open(parse(&args)?),
            "terminal_read" => self.read(&parse(&args)?),
            "terminal_send" => self.send(parse(&args)?),
            "terminal_wait" => self.wait(&parse(&args)?),
            "terminal_claim" => self.claim(parse(&args)?),
            "terminal_release" => self.release(&parse(&args)?),
            "terminal_control" => self.control(&parse(&args)?),
            "terminal_resize" => self.resize(parse(&args)?),
            "terminal_rename" => self.rename(parse(&args)?),
            "terminal_signal" => self.signal(&parse(&args)?),
            "terminal_close" => self.close(&parse(&args)?),
            "terminal_log" => self.log(&parse(&args)?),
            "worktree_list" => self.worktrees(),
            _ => Err(invalid(format!("{name} isn't carried out here yet"))),
        }?;
        if let Some(id) = request_id {
            let mut state = self.state.lock();
            if state.receipts.len() == RECEIPTS {
                state.receipts.pop_front();
            }
            state.receipts.push_back((id, args, done.clone()));
        }
        Ok(done)
    }

    /// The answer `id` got before, if the same arguments came with it.
    fn receipt(&self, id: &str, args: &Value) -> Result<Option<Done>, ProtocolError> {
        let valid = (1..=128).contains(&id.len())
            && (id.bytes()).all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if !valid {
            return Err(invalid(
                "requestId is 1 to 128 letters, digits, `_` and `-`",
            ));
        }
        let state = self.state.lock();
        match state.receipts.iter().find(|(seen, ..)| seen == id) {
            Some((_, given, done)) if given == args => Ok(Some(done.clone())),
            Some(_) => Err(invalid("that requestId came with other arguments before")),
            None => Ok(None),
        }
    }

    fn request(&self, body: RequestBody) -> Result<ResponseBody, ProtocolError> {
        self.connection.request(body).map_err(|error| match error {
            ClientError::Protocol(error) => error,
            ClientError::Timeout => ProtocolError::new(
                ErrorCode::Timeout,
                "the terminal daemon didn't answer in time",
            )
            .retryable(),
            other => ProtocolError::new(
                ErrorCode::Internal,
                format!("the terminal daemon couldn't be reached: {other}"),
            )
            .retryable(),
        })
    }

    fn terminals(&self, worktree: Option<Selector>) -> Result<Vec<TerminalSummary>, ProtocolError> {
        let status = None;
        match self.request(RequestBody::ListTerminals { worktree, status })? {
            ResponseBody::Terminals { terminals } => Ok(terminals),
            other => Err(unexpected(&other)),
        }
    }

    /// The terminal `given` names, or the one a call naming none means: the one last acted
    /// on in the caller's worktree, else its only one.
    fn terminal(&self, given: Option<&str>) -> Result<TerminalSummary, ProtocolError> {
        if let Some(handle) = given.filter(|given| given.starts_with("term_")) {
            return (self.terminals(None)?.into_iter())
                .find(|terminal| terminal.record.handle.as_str() == handle)
                .ok_or_else(|| not_found(format!("no terminal is {handle}")));
        }
        let mut here = self.terminals(Some(Selector::Current))?;
        if let Some(name) = given {
            return (here.into_iter())
                .find(|terminal| terminal.record.name == name)
                .ok_or_else(|| {
                    not_found(format!(
                        "your worktree has no terminal named {name:?}; terminal_list shows them"
                    ))
                });
        }
        let last = self.state.lock().last.clone();
        if let Some(found) = last.and_then(|last| here.iter().find(|t| t.record.handle == last)) {
            return Ok(found.clone());
        }
        if here.len() == 1
            && let Some(only) = here.pop()
        {
            return Ok(only);
        }
        if here.is_empty() {
            return Err(not_found(
                "your worktree has no terminals; open one with terminal_open",
            ));
        }
        let names: Vec<String> = (here.iter())
            .map(|t| format!("{} ({})", t.record.name, t.record.handle))
            .collect();
        let handles = here.iter().map(|t| t.record.handle.clone()).collect();
        Err(ProtocolError::new(
            ErrorCode::AmbiguousTerminal,
            format!("say which terminal: {}", names.join(", ")),
        )
        .with_details(ErrorDetails::Terminals { handles }))
    }

    /// The terminal `handle` now.
    fn record(&self, handle: &Handle) -> Result<TerminalSummary, ProtocolError> {
        (self.terminals(None)?.into_iter())
            .find(|terminal| &terminal.record.handle == handle)
            .ok_or_else(|| not_found(format!("{handle} is gone")))
    }

    fn remember(&self, handle: &Handle) {
        self.state.lock().last = Some(handle.clone());
    }

    fn list(&self, args: ListArgs) -> Result<Done, ProtocolError> {
        let worktree = Some(
            args.worktree
                .as_deref()
                .map_or(Selector::Current, Selector::parse),
        );
        let status = args.status;
        let mut terminals = match self.request(RequestBody::ListTerminals { worktree, status })? {
            ResponseBody::Terminals { terminals } => terminals,
            other => return Err(unexpected(&other)),
        };
        if let Some(labels) = &args.labels {
            terminals.retain(|t| labels.iter().all(|label| t.record.labels.contains(label)));
        }
        terminals.sort_by_key(|t| t.record.created_ms);
        let start = match args.cursor.as_deref() {
            Some(cursor) => cursor
                .parse()
                .map_err(|error| invalid(format!("cursor is a nextCursor: {error}")))?,
            None => 0,
        };
        let limit = args.limit.unwrap_or(50).clamp(1, 500) as usize;
        let page: Vec<&TerminalSummary> = terminals.iter().skip(start).take(limit).collect();
        let next = (start + limit < terminals.len()).then(|| (start + limit).to_string());
        let named: Vec<String> = (page.iter())
            .map(|t| format!("{} ({})", t.record.name, t.record.status.words()))
            .collect();
        let summary = match named.len() {
            0 => "no terminals".to_owned(),
            n => format!(
                "{n} terminal{}: {}",
                if n == 1 { "" } else { "s" },
                named.join(", ")
            ),
        };
        let page: Vec<Value> = page.into_iter().map(shape::summary).collect();
        Ok(Done {
            structured: json!({ "terminals": page, "nextCursor": next }),
            summary,
        })
    }

    fn open(&self, args: OpenArgs) -> Result<Done, ProtocolError> {
        let command = match args.command {
            None => Vec::new(),
            Some(CommandArg::Argv(program)) => program,
            Some(CommandArg::Line(line)) => crate::terminal::login_shell_line(line),
        };
        let request = OpenRequest {
            worktree: Some(
                args.worktree
                    .as_deref()
                    .map_or(Selector::Current, Selector::parse),
            ),
            name: args.name,
            command,
            cwd: args.cwd,
            env: args.env.unwrap_or_default(),
            size: args.size.map(|size| Size::cells(size.cols, size.rows)),
            labels: args.labels.unwrap_or_default().into_iter().collect(),
            if_exists: match args.if_exists {
                Some(IfExistsArg::Error) => IfExists::Error,
                Some(IfExistsArg::Attach) | None => IfExists::Attach,
            },
            respawn: args.respawn.unwrap_or(false),
        };
        let (record, existed) = match self.request(RequestBody::Open(request))? {
            ResponseBody::Opened { terminal, existed } => (*terminal, existed),
            other => return Err(unexpected(&other)),
        };
        self.remember(&record.handle);
        let mut control = record.control.clone();
        let claiming = args.claim.unwrap_or(true);
        if claiming && control.holder.is_none() {
            let claim = RequestBody::Claim {
                handle: record.handle.clone(),
                takeover: false,
                reason: None,
            };
            if let Ok(ResponseBody::Control { control: now }) = self.request(claim) {
                control = now;
            }
        }
        let verb = if existed { "found" } else { "opened" };
        let mut summary = format!(
            "{verb} {} ({}); {}",
            record.name,
            record.handle,
            shape::holder(&control, &self.actor)
        );
        let mut record = record;
        // Without the claim the size is only the one it starts at: a pin needs the hold.
        let pinned =
            (args.size.filter(|_| claiming)).map(|size| self.pin_opened(&mut record, size));
        let mut structured = json!({
            "terminal": shape::record(&record, ""),
            "existed": existed,
            "control": shape::control(&control, &self.actor),
        });
        if let Some(pinned) = pinned {
            summary.push_str(&resize::opened(&mut structured, &record, pinned));
        }
        Ok(Done {
            structured,
            summary,
        })
    }

    fn claim(&self, args: ClaimArgs) -> Result<Done, ProtocolError> {
        let terminal = self.terminal(args.terminal.as_deref())?;
        let handle = terminal.record.handle;
        let claim = RequestBody::Claim {
            handle: handle.clone(),
            takeover: args.takeover.unwrap_or(false),
            reason: args.reason,
        };
        let control = control_of(self.request(claim)?)?;
        self.remember(&handle);
        Ok(self.control_done(&terminal.record.name, &control, Value::Null))
    }

    fn release(&self, args: &TerminalArgs) -> Result<Done, ProtocolError> {
        let terminal = self.terminal(args.terminal.as_deref())?;
        let release = RequestBody::Release {
            handle: terminal.record.handle,
        };
        let control = control_of(self.request(release)?)?;
        Ok(self.control_done(&terminal.record.name, &control, Value::Null))
    }

    fn control(&self, args: &TerminalArgs) -> Result<Done, ProtocolError> {
        let terminal = self.terminal(args.terminal.as_deref())?;
        let tail = self.log_tail(&terminal.record.handle, LogKind::Activity, 5)?;
        let control = terminal.record.control;
        Ok(self.control_done(&terminal.record.name, &control, Value::Array(tail)))
    }

    fn control_done(&self, name: &str, control: &Control, tail: Value) -> Done {
        let mut structured = json!({ "control": shape::control(control, &self.actor) });
        if !tail.is_null() {
            structured["activityTail"] = tail;
        }
        Done {
            structured,
            summary: format!("{name}: {}", shape::holder(control, &self.actor)),
        }
    }

    fn rename(&self, args: RenameArgs) -> Result<Done, ProtocolError> {
        let terminal = self.terminal(args.terminal.as_deref())?;
        let rename = RequestBody::Rename {
            handle: terminal.record.handle.clone(),
            name: args.name,
            title: args.title,
            labels: args.labels.map(|labels| labels.into_iter().collect()),
        };
        self.request(rename)?;
        let now = self.record(&terminal.record.handle)?;
        Ok(Done {
            summary: format!("renamed {} to {}", terminal.record.name, now.record.name),
            structured: json!({ "terminal": shape::summary(&now) }),
        })
    }

    fn signal(&self, args: &SignalArgs) -> Result<Done, ProtocolError> {
        let terminal = self.terminal(args.terminal.as_deref())?;
        let signal = RequestBody::Signal {
            handle: terminal.record.handle.clone(),
            signal: args.signal,
        };
        self.request(signal)?;
        self.remember(&terminal.record.handle);
        Ok(Done {
            summary: format!("sent {} to {}", args.signal.name(), terminal.record.name),
            structured: json!({ "sent": args.signal.name() }),
        })
    }

    /// Closes a terminal; one whose program runs needs the person, unless the client was
    /// granted destruction.
    fn close(&self, args: &CloseArgs) -> Result<Done, ProtocolError> {
        let terminal = self.terminal(args.terminal.as_deref())?;
        let record = &terminal.record;
        let live = record.status != Status::Exited && record.pid.is_some();
        if live && !self.grants.contains(&Scope::Destructive) {
            return Err(ProtocolError::new(
                ErrorCode::ConfirmationRequired,
                format!(
                    "{}'s program still runs: stop it (terminal_signal or terminal_send with \
                     interrupt), then close it, or ask the person to close it",
                    record.name
                ),
            ));
        }
        let close = RequestBody::Close {
            handle: record.handle.clone(),
            force: true,
        };
        self.request(close)?;
        Ok(Done {
            summary: format!("closed {}", record.name),
            structured: json!({ "closed": [record.handle], "confirmed": false }),
        })
    }

    fn worktrees(&self) -> Result<Done, ProtocolError> {
        let worktrees = match self.request(RequestBody::ListWorktrees)? {
            ResponseBody::Worktrees { worktrees } => worktrees,
            other => return Err(unexpected(&other)),
        };
        let summary = format!("{} worktrees with terminals", worktrees.len());
        let worktrees: Vec<Value> = worktrees.iter().map(shape::worktree).collect();
        Ok(Done {
            structured: json!({ "worktrees": worktrees }),
            summary,
        })
    }
}

fn parse<T: DeserializeOwned>(args: &Value) -> Result<T, ProtocolError> {
    serde_json::from_value(args.clone()).map_err(|error| invalid(error.to_string()))
}

fn control_of(body: ResponseBody) -> Result<Control, ProtocolError> {
    match body {
        ResponseBody::Control { control } => Ok(control),
        other => Err(unexpected(&other)),
    }
}

fn invalid(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidArgument, message)
}

fn not_found(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::TerminalNotFound, message)
}

fn unexpected(body: &ResponseBody) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::Internal,
        format!("the daemon answered with something else: {body:?}"),
    )
}
