//! What the daemon says of every terminal, kept from its list and its events: the status
//! the tabs' dots and the sidebar's counts show, and who holds each terminal, which the
//! terminal tab's controller chip shows (`docs/terminals.md` 7.4, 9).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use colosso_term_control::{
    Actor, Control, ControlChange, Controller, Event, EventKind, Handle, Status, TerminalRecord,
};

/// How long the notice of a takeover by typing stays up.
pub const NOTICE_SECS: u64 = 12;

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub name: String,
    pub worktree: Option<PathBuf>,
    pub status: Status,
    pub reason: String,
    pub control: Control,
}

impl State {
    fn of(record: &TerminalRecord) -> State {
        State {
            name: record.name.clone(),
            worktree: record
                .worktree
                .as_ref()
                .map(|worktree| worktree.path.clone()),
            status: record.status,
            reason: record.status_reason.clone(),
            control: record.control.clone(),
        }
    }

    /// The person holds it: agents are locked out until they give it back.
    pub fn yours(&self) -> bool {
        matches!(self.control.holder, Some(Controller::Human { .. }))
    }

    /// The agent that holds it.
    pub fn agent(&self) -> Option<&Actor> {
        match &self.control.holder {
            Some(Controller::Agent { actor, .. }) => Some(actor),
            _ => None,
        }
    }
}

/// The person took a terminal from an agent by typing in it: shown until it's given back,
/// something else changes control, or it's been up a while.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Takeover {
    pub handle: Handle,
    pub from: String,
    pub at: Instant,
}

#[derive(Default)]
pub struct States {
    by_handle: HashMap<Handle, State>,
    pub takeover: Option<Takeover>,
}

impl States {
    pub fn get(&self, handle: &Handle) -> Option<&State> {
        self.by_handle.get(handle)
    }

    /// What a view learned of its terminal opening or attaching.
    pub fn upsert(&mut self, record: &TerminalRecord) {
        self.by_handle
            .insert(record.handle.clone(), State::of(record));
    }

    /// Starts over from the daemon's list.
    pub fn reset(&mut self, records: impl IntoIterator<Item = TerminalRecord>) {
        self.by_handle = (records.into_iter())
            .map(|record| (record.handle.clone(), State::of(&record)))
            .collect();
    }

    /// Takes in what the daemon said; whether anything shown changed.
    pub fn apply(&mut self, event: &Event) -> bool {
        if let EventKind::Opened { terminal } = &event.kind {
            self.by_handle
                .insert(event.handle.clone(), State::of(terminal));
            return true;
        }
        if matches!(event.kind, EventKind::Closed) {
            return self.by_handle.remove(&event.handle).is_some();
        }
        let Some(state) = self.by_handle.get_mut(&event.handle) else {
            return false;
        };
        match &event.kind {
            EventKind::Status { status, reason, .. } => {
                state.status = *status;
                reason.clone_into(&mut state.reason);
            }
            EventKind::Renamed { name, .. } => name.clone_into(&mut state.name),
            EventKind::Control { change, control } => {
                let before = state.agent().map(|agent| agent.label.clone());
                state.control = control.clone();
                self.takeover = match change {
                    ControlChange::HumanTook { implicit: true, .. } => Some(Takeover {
                        handle: event.handle.clone(),
                        from: before.unwrap_or_else(|| "an agent".to_owned()),
                        at: Instant::now(),
                    }),
                    _ => self
                        .takeover
                        .take()
                        .filter(|shown| shown.handle != event.handle),
                };
            }
            _ => return false,
        }
        true
    }

    /// The takeover notice for `handle`, while it's up.
    pub fn takeover(&self, handle: &Handle) -> Option<&Takeover> {
        (self.takeover.as_ref())
            .filter(|shown| shown.handle == *handle && shown.at.elapsed().as_secs() < NOTICE_SECS)
    }

    /// How many terminals of the checkout at `worktree` wait for the person.
    /// The terminals of the checkout at `worktree`, by name.
    pub fn of_worktree(&self, worktree: &Path) -> Vec<(Handle, &State)> {
        let mut terminals: Vec<(Handle, &State)> = (self.by_handle.iter())
            .filter(|(_, state)| state.worktree.as_deref() == Some(worktree))
            .map(|(handle, state)| (handle.clone(), state))
            .collect();
        terminals.sort_by(|a, b| a.1.name.cmp(&b.1.name));
        terminals
    }

    pub fn needing_input(&self, worktree: &Path) -> usize {
        (self.by_handle.values())
            .filter(|state| {
                state.status == Status::NeedsInput && state.worktree.as_deref() == Some(worktree)
            })
            .count()
    }
}

/// Of several statuses, the one a tab's dot shows: what needs the person first, then what's
/// busy, then what's waiting, then what ended.
pub fn most_urgent(statuses: impl IntoIterator<Item = Status>) -> Option<Status> {
    let rank = |status: &Status| match status {
        Status::NeedsInput => 4,
        Status::Running => 3,
        Status::Idle => 2,
        Status::Exited => 1,
        Status::Unknown => 0,
    };
    statuses.into_iter().max_by_key(rank)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_shows_what_needs_the_person_before_what_runs() {
        use Status::{Exited, Idle, NeedsInput, Running};
        assert_eq!(most_urgent([Idle, NeedsInput, Running]), Some(NeedsInput));
        assert_eq!(most_urgent([Exited, Idle]), Some(Idle));
        assert_eq!(most_urgent([Exited, Running]), Some(Running));
        assert_eq!(most_urgent([]), None);
    }

    #[test]
    fn events_of_terminals_not_listed_change_nothing() {
        let mut states = States::default();
        let event = Event {
            handle: Handle("term_unknown".into()),
            kind: EventKind::Bell,
        };
        assert!(!states.apply(&event));
    }

    #[test]
    fn a_checkouts_terminals_are_its_own_by_name_and_go_as_they_close() {
        use std::collections::{BTreeMap, BTreeSet};

        use colosso_term_control::{Actor, Size, WorktreeRef};

        let record = |name: &str, worktree: &str| TerminalRecord {
            handle: Handle::generate(),
            worktree: Some(WorktreeRef {
                path: worktree.into(),
                repo_main: "/w/main".into(),
                branch: None,
            }),
            name: name.into(),
            title: None,
            cwd: worktree.into(),
            command: vec![],
            env: BTreeMap::new(),
            size: Size::default(),
            labels: BTreeSet::new(),
            created_ms: 1,
            created_by: Actor::system(),
            incarnation: 1,
            pid: Some(42),
            exit: None,
            status: Status::Idle,
            status_reason: "prompt".into(),
            last_exit: None,
            progress: None,
            control: Control::default(),
            attached_clients: 1,
            last_output_ms: 1,
            pinned: None,
        };
        let (server, shell) = (record("server", "/w/feat"), record("shell", "/w/feat"));
        let mut states = States::default();
        states.reset([shell, server.clone(), record("other", "/w/main")]);
        let names = |states: &States| -> Vec<String> {
            (states.of_worktree(Path::new("/w/feat")).into_iter())
                .map(|(_, state)| state.name.clone())
                .collect()
        };
        assert_eq!(names(&states), ["server", "shell"]);
        let closed = Event {
            handle: server.handle,
            kind: EventKind::Closed,
        };
        assert!(states.apply(&closed));
        assert_eq!(names(&states), ["shell"]);
        assert!(states.of_worktree(Path::new("/w/none")).is_empty());
    }
}
