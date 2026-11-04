//! `Open`: find the terminal called `name` in a worktree, or make it, or run it again.
//! `docs/terminals.md` 5.2.

use std::sync::Arc;

use koyo_term_control::{
    Control, ErrorCode, ErrorDetails, EventKind, Handle, IfExists, OpenRequest, ProtocolError,
    ResponseBody, Status, TerminalRecord, now_ms,
};

use crate::connection::Client;
use crate::server::Daemon;
use crate::session::{self, Cmd, Reply};
use crate::worktree;

/// `Some(body)` to answer now; `None` when a session was asked and will answer itself.
pub fn open(
    daemon: &Arc<Daemon>,
    client: &Arc<Client>,
    req: u32,
    open: OpenRequest,
) -> Result<Option<ResponseBody>, ProtocolError> {
    valid_name(&open.name)?;
    let known = daemon.registry.lock().known_worktrees();
    let cwd = open.cwd.as_deref().or(client.cwd.as_deref());
    let worktree = match worktree::resolve(open.worktree.as_ref(), cwd, &known) {
        Ok(worktree) => Some(worktree),
        // A terminal outside any checkout is allowed when the caller said where to start it.
        Err(error) if error.code == ErrorCode::WorktreeRequired && open.cwd.is_some() => None,
        Err(error) => return Err(error),
    };
    let worktree_path = worktree.as_ref().map(|worktree| worktree.path.clone());
    let existing = daemon
        .registry
        .lock()
        .by_name(worktree_path.as_deref(), &open.name)
        .cloned();
    if let Some(existing) = existing {
        if open.if_exists == IfExists::Error {
            return Err(ProtocolError::new(
                ErrorCode::NameTaken,
                format!(
                    "{} already has a terminal called {}",
                    place(&existing),
                    open.name
                ),
            )
            .with_details(ErrorDetails::Existing {
                handle: existing.handle,
            }));
        }
        return reopen(daemon, client, req, existing, open.respawn);
    }
    let cwd = open
        .cwd
        .clone()
        .or_else(|| worktree_path.clone())
        .ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::WorktreeRequired,
                "say where to start the terminal: a worktree or a cwd",
            )
        })?;
    let record = TerminalRecord {
        handle: Handle::generate(),
        worktree,
        name: open.name,
        title: None,
        cwd,
        command: open.command,
        env: open.env,
        size: open.size.unwrap_or_default(),
        labels: open.labels,
        created_ms: now_ms(),
        created_by: client.actor.clone(),
        incarnation: 1,
        pid: None,
        exit: None,
        status: Status::Unknown,
        status_reason: String::new(),
        last_exit: None,
        progress: None,
        control: Control::default(),
        attached_clients: 0,
        last_output_ms: 0,
        pinned: None,
    };
    let handle = record.handle.clone();
    let session = session::start(daemon, record)?;
    let record = session
        .live
        .record
        .lock()
        .clone()
        .ok_or_else(|| ProtocolError::new(ErrorCode::Internal, "the session kept no record"))?;
    daemon.registry.lock().put(record.clone());
    daemon.add_session(handle.clone(), session);
    daemon.persist();
    daemon.broadcast(
        &handle,
        &EventKind::Opened {
            terminal: Box::new(record.clone()),
        },
    );
    log::info!("{handle} opened: {} in {}", record.name, place(&record));
    Ok(Some(ResponseBody::Opened {
        terminal: Box::new(record),
        existed: false,
    }))
}

/// A terminal that exists: as it is, or run again when asked and it has exited.
fn reopen(
    daemon: &Arc<Daemon>,
    client: &Arc<Client>,
    req: u32,
    existing: TerminalRecord,
    respawn: bool,
) -> Result<Option<ResponseBody>, ProtocolError> {
    let handle = existing.handle.clone();
    if let Some(session) = daemon.session(&handle) {
        let live = session.live.record.lock().clone().unwrap_or(existing);
        if respawn && live.pid.is_none() {
            session
                .tx
                .send(Cmd::Respawn {
                    reply: Reply::new(Arc::clone(client), req),
                })
                .map_err(|_gone| {
                    ProtocolError::new(ErrorCode::Internal, "the terminal's thread is gone")
                })?;
            return Ok(None);
        }
        return Ok(Some(ResponseBody::Opened {
            terminal: Box::new(live),
            existed: true,
        }));
    }
    // Remembered from before the daemon restarted: no process until asked for one.
    if !respawn {
        return Ok(Some(ResponseBody::Opened {
            terminal: Box::new(existing),
            existed: true,
        }));
    }
    let record = TerminalRecord {
        incarnation: existing.incarnation + 1,
        exit: None,
        ..existing
    };
    let session = session::start(daemon, record)?;
    let record = session
        .live
        .record
        .lock()
        .clone()
        .ok_or_else(|| ProtocolError::new(ErrorCode::Internal, "the session kept no record"))?;
    daemon.registry.lock().put(record.clone());
    daemon.add_session(handle, session);
    daemon.persist();
    Ok(Some(ResponseBody::Opened {
        terminal: Box::new(record),
        existed: true,
    }))
}

/// `[a-z0-9][a-z0-9-]{0,31}`.
pub fn valid_name(name: &str) -> Result<(), ProtocolError> {
    if koyo_term_control::terminal::is_terminal_name(name) {
        Ok(())
    } else {
        Err(ProtocolError::new(
            ErrorCode::InvalidArgument,
            format!(
                "{name:?} isn't a terminal name: lowercase letters, digits and dashes, up to 32, starting with a letter or digit"
            ),
        ))
    }
}

fn place(record: &TerminalRecord) -> String {
    record.worktree.as_ref().map_or_else(
        || record.cwd.display().to_string(),
        koyo_term_control::WorktreeRef::display_name,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_rule() {
        assert!(valid_name("tests").is_ok());
        assert!(valid_name("dev-server-2").is_ok());
        assert!(valid_name("1").is_ok());
        assert!(valid_name("").is_err());
        assert!(valid_name("-x").is_err());
        assert!(valid_name("Tests").is_err());
        assert!(valid_name("a b").is_err());
        assert!(valid_name(&"a".repeat(33)).is_err());
    }
}
