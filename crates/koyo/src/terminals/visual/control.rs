//! The terminal's state as the person sees it (`docs/terminals.md` 5.6, 5.7, 7.4): the
//! status following the shell's marks, an agent holding the terminal, the person taking it
//! by typing with the notice that says so, the agent refused from then on, and "Give back";
//! a terminal the agent opens appearing as a tab without taking the focus, and going with
//! its tab when the agent closes it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use koyo_term_control::{
    AttachMode, ClientError, ClientKind, Connection, ErrorCode, Handle, Hello, IfExists,
    OpenRequest, RequestBody, ResponseBody, Selector, Status,
};

use super::drive::{type_line, wait, wait_until};
use crate::terminals::states::State;
use crate::visual::{Drawn, Result};

/// What the app knows of the shown terminal.
fn state(drawn: &mut Drawn) -> Option<State> {
    let shell = drawn.shell.clone();
    drawn.cx.read(|cx| {
        let shell = shell.read(cx);
        let id = shell.shown()?.0;
        let handle = shell.terminals.handle_of(id)?;
        shell.terminals.states.get(handle).cloned()
    })
}

pub(super) fn holds(drawn: &mut Drawn, what: &str, test: impl Fn(&State) -> bool) -> Result<()> {
    wait_until(drawn, what, |drawn| {
        Ok(state(drawn).is_some_and(|state| test(&state)))
    })
}

pub fn control(drawn: &mut Drawn, said: &mut Vec<String>) -> Result<()> {
    type_line(drawn, "sleep 1")?;
    holds(drawn, "the status to say a command runs", |state| {
        state.status == Status::Running && state.reason == "command running"
    })?;
    holds(drawn, "the status to say the prompt is back", |state| {
        state.status == Status::Idle && state.reason == "prompt"
    })?;
    said.push("the status followed the shell: a command running, then the prompt".into());

    let shell = drawn.shell.clone();
    let found = drawn.cx.read(|cx| {
        let shell = shell.read(cx);
        let id = shell.shown()?.0;
        Some((
            shell.terminals.client.clone()?,
            shell.terminals.handle_of(id)?.clone(),
        ))
    });
    let (client, handle) = found.ok_or("the shown terminal isn't known yet")?;
    // The agent's inbox lives as long as it does, or its connection's reader stops.
    let (agent, _inbox) = connect_agent(client.dir())?;
    // Attached to write, as a relay is, with no size of its own to impose.
    let attach = RequestBody::Attach {
        handle: handle.clone(),
        size: None,
        mode: AttachMode::Control,
        since_offset: None,
    };
    agent
        .request(attach)
        .map_err(|error| format!("the agent's attach: {error}"))?;
    let claim = RequestBody::Claim {
        handle: handle.clone(),
        takeover: false,
        reason: None,
    };
    agent
        .request(claim)
        .map_err(|error| format!("the agent's claim: {error}"))?;
    holds(drawn, "the bar to show the agent", |state| {
        state
            .agent()
            .is_some_and(|agent| agent.label == "visual-agent")
    })?;
    said.push(format!("{}", drawn.save("agent.png")?.display()));

    type_line(drawn, "echo mine")?;
    wait(drawn, "the person's command", |text| {
        text.lines().any(|line| line == "mine")
    })?;
    holds(drawn, "the terminal to be the person's", State::yours)?;
    let noticed = drawn
        .cx
        .read(|cx| shell.read(cx).terminals.states.takeover(&handle).is_some());
    if !noticed {
        return Err("typing took the terminal but no notice says so".into());
    }
    said.push(format!("{}", drawn.save("takeover.png")?.display()));
    match refused(&agent, &handle) {
        Some(ErrorCode::PersonHasControl) => {
            said.push("the agent's write was refused: person_has_control".into());
        }
        other => {
            return Err(format!(
                "the agent's write after the takeover got {other:?}"
            ));
        }
    }

    // As the bar's "Give back" does.
    let release = RequestBody::Release {
        handle: handle.clone(),
    };
    client
        .request(release)
        .map_err(|error| format!("giving back: {error}"))?;
    holds(drawn, "the terminal to be no one's", |state| {
        state.control.holder.is_none()
    })?;
    said.push(format!("{}", drawn.save("given-back.png")?.display()));
    agent_opens(drawn, &agent, &handle, said)?;
    agent.close();

    // A program asking for the person: the dot and the branch's count say so until they type.
    type_line(drawn, "printf '\\e]9;the check needs you\\a'")?;
    holds(drawn, "the terminal to need the person", |state| {
        state.status == Status::NeedsInput && state.reason.starts_with("notification")
    })?;
    let counted = drawn.cx.read(|cx| {
        let shell = shell.read(cx);
        let branch = shell
            .workspace
            .branch()
            .map(|unit| unit.worktree.path.clone());
        branch.map_or(0, |path| shell.terminals_needing_input(&path))
    });
    if counted != 1 {
        return Err(format!(
            "the branch counts {counted} terminals needing the person, not 1"
        ));
    }
    said.push(format!("{}", drawn.save("needs-input.png")?.display()));
    type_line(drawn, "true")?;
    holds(drawn, "typing to answer it", |state| {
        state.status != Status::NeedsInput
    })?;
    said.push("a notification made the terminal need the person, and typing answered it".into());
    Ok(())
}

/// The agent opens a terminal in the branch: it gets a tab, the person's terminal still
/// shown; the agent closes it, and the tab goes.
fn agent_opens(
    drawn: &mut Drawn,
    agent: &Connection,
    persons: &Handle,
    said: &mut Vec<String>,
) -> Result<()> {
    let shell = drawn.shell.clone();
    let branch: Option<PathBuf> = drawn.cx.read(|cx| {
        let shell = shell.read(cx);
        shell
            .workspace
            .branch()
            .map(|unit| unit.worktree.path.clone())
    });
    let open = OpenRequest {
        worktree: Some(Selector::Path(branch.ok_or("no branch is selected")?)),
        name: "agent-made".into(),
        command: ["/bin/sh", "-c", "sleep 60"].map(str::to_owned).to_vec(),
        cwd: None,
        env: BTreeMap::new(),
        size: None,
        labels: BTreeSet::new(),
        if_exists: IfExists::Attach,
        respawn: false,
    };
    let handle = match agent.request(RequestBody::Open(open)) {
        Ok(ResponseBody::Opened { terminal, .. }) => terminal.handle,
        other => return Err(format!("the agent's open: {other:?}")),
    };
    let tabbed = |drawn: &mut Drawn, handle: &Handle| {
        drawn.cx.read(|cx| {
            let shell = shell.read(cx);
            let id = shell.terminals.tab_of(handle);
            id.is_some_and(|id| shell.workspace.terminal_tabs().any(|(_, tab)| tab == id))
        })
    };
    wait_until(drawn, "the agent's terminal to get a tab", |drawn| {
        Ok(tabbed(drawn, &handle))
    })?;
    let shown = drawn.cx.read(|cx| {
        let shell = shell.read(cx);
        let id = shell.shown()?.0;
        shell.terminals.handle_of(id).cloned()
    });
    if shown.as_ref() != Some(persons) {
        return Err("the agent's terminal took the focus from the person's".into());
    }
    said.push(format!("{}", drawn.save("agent-tab.png")?.display()));
    let claim = RequestBody::Claim {
        handle: handle.clone(),
        takeover: false,
        reason: None,
    };
    let close = RequestBody::Close {
        handle: handle.clone(),
        force: true,
    };
    (agent.request(claim).and_then(|_| agent.request(close)))
        .map_err(|error| format!("the agent's close: {error}"))?;
    wait_until(drawn, "the agent's tab to go with its terminal", |drawn| {
        Ok(!tabbed(drawn, &handle))
    })?;
    said.push(
        "a terminal the agent opened got a tab, left the focus alone, and went with it".into(),
    );
    Ok(())
}

/// A connection as an agent's relay makes one, and its inbox.
fn connect_agent(dir: &koyo_term_control::TermDir) -> Result<(Arc<Connection>, Box<dyn Send>)> {
    let hello = Hello::new(ClientKind::Relay, "visual-agent", "0");
    let (tx, inbox) = koyo_term_control::client::inbox();
    let (connection, _) = Connection::connect(&dir.socket(), &hello, tx)
        .map_err(|error| format!("the agent couldn't connect: {error}"))?;
    Ok((connection, Box::new(inbox)))
}

/// What the daemon says to an agent's paste.
fn refused(agent: &Connection, handle: &Handle) -> Option<ErrorCode> {
    let paste = RequestBody::Paste {
        handle: handle.clone(),
        text: "echo agent\n".into(),
    };
    match agent.request(paste) {
        Err(ClientError::Protocol(error)) => Some(error.code),
        _ => None,
    }
}
