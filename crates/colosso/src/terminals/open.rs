//! Terminals opened in tabs: a shell named after itself and the next free number, or a
//! program under a name of the caller's choosing; and a terminal the daemon runs already,
//! shown where its tab is or in a new tab of its checkout.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use colosso_term::{Source, TerminalView};
use colosso_term_control::{Handle, OpenRequest, RequestBody, ResponseBody, Selector};
use gpui_kit::{AppContext as _, Context, Entity, Task};

use super::{Terminals, free_name, open_request, shell_name};
use crate::shell::Shell;

impl Terminals {
    /// The view of terminal tab `id`, once it's made.
    pub fn view(&self, id: u64) -> Option<Entity<TerminalView>> {
        self.views.get(&id).map(|made| made.view.clone())
    }

    /// The terminal tab `id` shows, by its daemon handle, once it's known.
    pub fn handle(&self, id: u64) -> Option<&Handle> {
        self.handle_of(id)
    }

    /// How many terminals the daemon has said run in the checkout at `worktree`.
    pub fn running_in(&self, worktree: &Path) -> usize {
        self.states.of_worktree(worktree).len()
    }
}

impl Shell {
    /// Opens a shell in the selected checkout, in a new tab, named after the shell and the
    /// next free number.
    pub fn new_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(worktree) = self
            .workspace
            .branch()
            .map(|unit| unit.worktree.path.clone())
        else {
            return;
        };
        self.open_terminal(worktree, None, Vec::new(), cx).detach();
    }

    /// Opens a terminal in the checkout at `worktree`, in a new tab that's shown and takes the
    /// keyboard: named `name`, or after the shell and the next free number, and running
    /// `command`, or the login shell when it's empty. A name the checkout has already shows
    /// that terminal. Done with the tab's pane once it's added; with `None` when the daemon
    /// can't be reached or the checkout isn't listed.
    pub(crate) fn open_terminal(
        &mut self,
        worktree: PathBuf,
        name: Option<String>,
        command: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Task<Option<u64>> {
        let Some(client) = self.terminals.client() else {
            return Task::ready(None);
        };
        self.watch_terminals(cx);
        cx.spawn(async move |this, cx| {
            // Only a name to pick needs the names taken.
            let listed = match &name {
                Some(_) => None,
                None => Some(
                    cx.background_spawn({
                        let worktree = worktree.clone();
                        async move {
                            client.request(RequestBody::ListTerminals {
                                worktree: Some(Selector::Path(worktree)),
                                status: None,
                            })
                        }
                    })
                    .await,
                ),
            };
            let taken: BTreeSet<String> = match listed {
                Some(Ok(ResponseBody::Terminals { terminals })) => terminals
                    .into_iter()
                    .map(|summary| summary.record.name)
                    .collect(),
                None | Some(Ok(_)) => BTreeSet::new(),
                Some(Err(error)) => {
                    log::warn!(
                        "couldn't list the terminals of {}: {error}",
                        worktree.display()
                    );
                    BTreeSet::new()
                }
            };
            this.update(cx, |this, cx| {
                let name = name.unwrap_or_else(|| {
                    // Names the open tabs hold count too: their terminals may not be open yet.
                    let mut taken = taken;
                    taken.extend(
                        this.terminals
                            .views
                            .values()
                            .map(|made| made.view.read(cx).name().to_owned()),
                    );
                    free_name(&shell_name(), &taken)
                });
                let request = OpenRequest {
                    command,
                    ..open_request(&worktree, name.clone())
                };
                let id = this.terminals.add(Source::Open(Box::new(request)));
                this.terminals.focus_next = Some(id);
                let pane = this
                    .workspace
                    .add_terminal_tab(&worktree, id, name.into(), true);
                cx.notify();
                pane
            })
            .ok()
            .flatten()
        })
    }

    /// Shows terminal `handle`: the tab showing it already, or a new tab on its checkout.
    /// The pane showing it; `None` when the daemon hasn't said it runs such a terminal.
    pub(crate) fn reveal_terminal(
        &mut self,
        handle: &Handle,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        if let Some(id) = self.terminals.tab_of(handle) {
            let (at, pane) = self.workspace.find_pane(|pane| pane.terminal == Some(id))?;
            self.workspace.select_tab(at.0, at.1, at.2);
            cx.notify();
            return Some(pane);
        }
        let state = self.terminals.states.get(handle)?;
        let (worktree, name) = (state.worktree.clone()?, state.name.clone());
        let id = self.terminals.add(Source::Existing {
            handle: handle.clone(),
            name: name.clone(),
        });
        let pane = self
            .workspace
            .add_terminal_tab(&worktree, id, name.into(), true);
        cx.notify();
        pane
    }

    /// Whether the daemon has said it runs terminal `handle`.
    pub(crate) fn terminal_known(&self, handle: &Handle) -> bool {
        self.terminals.states.get(handle).is_some()
    }
}
