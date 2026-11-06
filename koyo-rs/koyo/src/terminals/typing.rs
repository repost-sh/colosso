//! A command typed into a new shell, as the launcher runs one (`docs/new-tab.md` 2): the shell
//! opens as any does, and once the daemon says it's idle the command is typed into it with Enter,
//! so it shows at the prompt and goes into the shell's own history, and the shell stays when it's
//! done. Typed before the prompt, the terminal would echo it above the prompt.

use gpui_kit::{AppContext as _, Context};
use koyo_term_control::{Handle, RequestBody, SendRequest, Status, terminal};

use super::open::Naming;
use super::shell_name;
use crate::shell::Shell;

impl Shell {
    /// Opens a shell in the selected checkout, in a new tab named after `line`, and types `line`
    /// into it once it's at its prompt.
    pub(crate) fn run_in_new_terminal(&mut self, line: String, cx: &mut Context<Self>) {
        let Some(worktree) = self
            .workspace
            .branch()
            .map(|unit| unit.worktree.path.clone())
        else {
            return;
        };
        let name = Naming::After(terminal::name_after(&line).unwrap_or_else(shell_name));
        self.open_terminal(worktree, name, Vec::new(), Some(line), cx)
            .detach();
    }

    /// What waits to be typed goes into each terminal the daemon now says is idle: at its prompt,
    /// as a shell's marks tell, or quiet, the most a shell without them says. Asking for input is
    /// a bell or a notice, not a prompt. Called as a terminal opens and as its status changes.
    pub(super) fn type_waiting(&mut self, cx: &mut Context<Self>) {
        if self.terminals.typing.is_empty() {
            return;
        }
        let terminals = &self.terminals;
        let ready: Vec<(u64, Handle)> = (terminals.typing.keys())
            .filter_map(|id| Some((*id, terminals.handle_of(*id)?.clone())))
            .filter(|(_, handle)| {
                terminals
                    .states
                    .get(handle)
                    .is_some_and(|state| state.status == Status::Idle)
            })
            .collect();
        for (id, handle) in ready {
            if let Some(line) = self.terminals.typing.remove(&id) {
                self.type_line(handle, line, cx);
            }
        }
    }

    /// Types `line` into terminal `handle`, then Enter.
    fn type_line(&mut self, handle: Handle, line: String, cx: &mut Context<Self>) {
        let Some(client) = self.terminals.client() else {
            return;
        };
        let send = SendRequest {
            handle,
            text: Some(line),
            paste: None,
            keys: Vec::new(),
            enter: true,
            interrupt: false,
        };
        cx.background_spawn(async move {
            if let Err(error) = client.request(RequestBody::Send(send)) {
                log::warn!("couldn't type the command into its terminal: {error}");
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use gpui_kit::TestAppContext;
    use koyo_term::Source;
    use koyo_term_control::{Actor, Control, Event, EventKind, Size, TerminalRecord};

    use super::*;
    use crate::test_support::{repo, shell_with};

    /// Terminal `handle`, a login shell at `status`.
    fn record(handle: &Handle, status: Status) -> TerminalRecord {
        TerminalRecord {
            handle: handle.clone(),
            worktree: None,
            name: "zsh".into(),
            title: None,
            cwd: "/w".into(),
            command: vec![],
            env: BTreeMap::new(),
            size: Size::default(),
            labels: BTreeSet::new(),
            created_ms: 1,
            created_by: Actor::system(),
            incarnation: 1,
            pid: Some(42),
            exit: None,
            status,
            status_reason: String::new(),
            last_exit: None,
            progress: None,
            control: Control::default(),
            attached_clients: 1,
            last_output_ms: 1,
            pinned: None,
        }
    }

    #[gpui_kit::test]
    fn a_command_waits_while_its_shell_starts_or_rings_and_goes_once_it_is_idle(
        cx: &mut TestAppContext,
    ) {
        let root = repo("typing-waits", &[("README.md", "hi\n")]);
        let (_, shell) = shell_with(&[&root], cx);
        shell.update(cx, |shell, cx| {
            let handle = Handle::generate();
            let id = shell.terminals.add(Source::Existing {
                handle: handle.clone(),
                name: "zsh".into(),
            });
            shell.terminals.typing.insert(id, "echo hi".into());
            shell
                .terminals
                .states
                .upsert(&record(&handle, Status::Running));
            shell.type_waiting(cx);
            assert!(
                shell.terminals.typing.contains_key(&id),
                "still starting up"
            );
            let now = |status: Status, reason: &str| Event {
                handle: handle.clone(),
                kind: EventKind::Status {
                    status,
                    reason: reason.into(),
                    progress: None,
                },
            };
            shell
                .terminals
                .states
                .apply(&now(Status::NeedsInput, "bell"));
            shell.type_waiting(cx);
            assert!(
                shell.terminals.typing.contains_key(&id),
                "a bell isn't a prompt"
            );
            shell.terminals.states.apply(&now(Status::Idle, "prompt"));
            shell.type_waiting(cx);
            assert!(shell.terminals.typing.is_empty(), "typed at the prompt");
        });
    }
}
