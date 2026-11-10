//! The terminals brought back as tabs when the app starts: those the daemon keeps, and the
//! person's shells a daemon restart ended. A daemon of another build lists nothing, so the
//! shells the registry remembers get tabs that say so (`docs/terminals.md` 5.1).
//!
//! The window brings its terminal panes back first, where they were (`shell/kept.rs`), each
//! showing the terminal it showed, by its handle. The daemon's list then says which of those
//! still run: one that doesn't leaves its pane, and one the list has that no pane shows, opened
//! while the window was closed, gets a tab after the others.

use std::path::{Path, PathBuf};

use gpui_kit::{AppContext as _, Context};
use koyo_term::{Source, is_other_build};
use koyo_term_control::{ActorKind, Handle, RequestBody, ResponseBody, TerminalSummary};

use super::{Terminals, open_request};
use crate::shell::Shell;

impl Terminals {
    /// The terminal tab `id` shows as the window keeps it for the next one: its handle and its
    /// name, once the daemon runs it.
    pub(crate) fn kept(&self, id: u64) -> Option<(&Handle, &str)> {
        match self.sources.get(&id)? {
            Source::Existing { handle, name } => Some((handle, name)),
            Source::Open(_) => None,
        }
    }

    /// A tab for the terminal `handle`, called `name`, that the window showed when it was last
    /// open: it shows that terminal again, if the daemon's list says it still runs.
    pub(crate) fn bring_back(&mut self, handle: Handle, name: String) -> u64 {
        let id = self.add(Source::Existing { handle, name });
        self.unconfirmed.insert(id);
        id
    }

    /// The daemon's list has the terminal of tab `tab`, which the window brought back: from
    /// now on the tab shows `source`. A shell to run again leaves the view made for the
    /// terminal that ended, if the tab was drawn already, so the next draw makes one for it.
    fn confirm(&mut self, tab: u64, source: Source) {
        if !self.unconfirmed.remove(&tab) {
            return;
        }
        if matches!(source, Source::Open(_)) {
            self.views.remove(&tab);
        }
        self.sources.insert(tab, source);
    }
}

impl Shell {
    /// Brings back as tabs the terminals the daemon keeps for checkouts listed for the first
    /// time, and runs again the person's shells a daemon restart ended.
    pub fn restore_terminals(&mut self, cx: &mut Context<Self>) {
        let fresh: Vec<PathBuf> = (self.workspace.projects.iter())
            .flat_map(|project| {
                project
                    .branches
                    .iter()
                    .map(|unit| unit.worktree.path.clone())
            })
            .filter(|path| !self.terminals.restored.contains(path))
            .collect();
        if fresh.is_empty() {
            return;
        }
        let Some(client) = self.terminals.client() else {
            return;
        };
        self.watch_terminals(cx);
        self.terminals.restored.extend(fresh.iter().cloned());
        cx.spawn(async move |this, cx| {
            let listed = cx
                .background_spawn(async move {
                    let list = RequestBody::ListTerminals {
                        worktree: None,
                        status: None,
                    };
                    // Terminals remembered from before a restart need the daemon to come back.
                    if client.dir().remembers_terminals() {
                        client.request(list).map(Some)
                    } else {
                        client.request_if_running(list)
                    }
                })
                .await;
            let terminals: Vec<TerminalSummary> = match listed {
                Ok(Some(ResponseBody::Terminals { terminals })) => terminals,
                // No daemon runs and none is remembered: no terminal outlived the window.
                Ok(None) => Vec::new(),
                Ok(Some(_)) => return,
                // Another build's daemon: the shells it would bring back get tabs, which say
                // so and offer to replace it.
                Err(error) if is_other_build(&error) => {
                    log::warn!("couldn't list the running terminals: {error}");
                    this.update(cx, |this, cx| this.add_remembered_shells(&fresh, cx))
                        .ok();
                    return;
                }
                Err(error) => return log::warn!("couldn't list the running terminals: {error}"),
            };
            this.update(cx, |this, cx| this.take_terminals(&fresh, terminals, cx))
                .ok();
        })
        .detach();
    }

    /// Takes in the daemon's list of `terminals`: those of the `fresh` checkouts no tab shows
    /// get tabs, and the panes the window brought back keep the terminals that still run and
    /// leave for those that don't.
    pub(crate) fn take_terminals(
        &mut self,
        fresh: &[PathBuf],
        terminals: Vec<TerminalSummary>,
        cx: &mut Context<Self>,
    ) {
        for summary in terminals {
            let record = summary.record;
            let Some(worktree) = record
                .worktree
                .as_ref()
                .map(|worktree| worktree.path.clone())
            else {
                continue;
            };
            let name = record.name.clone();
            // A shell the person opened that a daemon restart ended runs again, from the
            // screen it had; other ended terminals stay closed.
            let source = match record.exit {
                None => Source::Existing {
                    handle: record.handle.clone(),
                    name: name.clone(),
                },
                Some(exit)
                    if exit.daemon_restart
                        && record.command.is_empty()
                        && record.created_by.kind == ActorKind::Human =>
                {
                    Source::Open(Box::new(open_request(&worktree, name.clone())))
                }
                Some(_) => continue,
            };
            self.place_terminal(fresh, &worktree, Some(&record.handle), (source, name));
        }
        // What's left the daemon doesn't run any more: ended, or closed, while the window was.
        for gone in std::mem::take(&mut self.terminals.unconfirmed) {
            self.workspace.close_terminal_pane(gone);
        }
        cx.notify();
    }

    /// Gives the terminal `handle` of the checkout at `worktree`, called `name`, its place: the
    /// pane the window brought back for it shows `source`, where it was; else, in one of the
    /// `fresh` checkouts, it gets a tab after the others. A terminal a tab shows already, as an
    /// agent's may have got one as it opened, keeps that tab.
    fn place_terminal(
        &mut self,
        fresh: &[PathBuf],
        worktree: &Path,
        handle: Option<&Handle>,
        (source, name): (Source, String),
    ) {
        if let Some(tab) = handle.and_then(|handle| self.terminals.tab_of(handle)) {
            self.terminals.confirm(tab, source);
        } else if fresh.iter().any(|fresh| fresh == worktree) {
            let id = self.terminals.add(source);
            self.workspace
                .add_terminal_tab(worktree, id, name.into(), false);
        }
    }

    /// Tabs for the person's shells the registry remembers in the `fresh` checkouts, read as
    /// written, since another build wrote it: what replacing its daemon brings back.
    fn add_remembered_shells(&mut self, fresh: &[PathBuf], cx: &mut Context<Self>) {
        let Some(client) = self.terminals.client() else {
            return;
        };
        for record in client.dir().remembered() {
            let worktree = record["worktree"]["path"].as_str().map(PathBuf::from);
            let Some(worktree) = worktree else {
                continue;
            };
            let shell = record["command"].as_array().is_some_and(Vec::is_empty)
                && record["created_by"]["kind"] == "human"
                && (record["exit"].is_null() || record["exit"]["daemon_restart"] == true);
            let Some(name) = record["name"].as_str().filter(|_| shell) else {
                continue;
            };
            let request = Source::Open(Box::new(open_request(&worktree, name.to_owned())));
            let handle = record["handle"]
                .as_str()
                .map(|handle| Handle(handle.to_owned()));
            self.place_terminal(
                fresh,
                &worktree,
                handle.as_ref(),
                (request, name.to_owned()),
            );
        }
        // The rest stay as they were brought back: the other build's daemon may run them yet.
        self.terminals.unconfirmed.clear();
        cx.notify();
    }
}
