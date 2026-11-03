//! The terminals brought back as tabs when the app starts: those the daemon keeps, and the
//! person's shells a daemon restart ended. A daemon of another build lists nothing, so the
//! shells the registry remembers get tabs that say so (`docs/terminals.md` 5.1).

use std::path::PathBuf;

use koyo_term::{Source, is_other_build};
use koyo_term_control::{ActorKind, RequestBody, ResponseBody, TerminalSummary};
use gpui_kit::{AppContext as _, Context};

use super::open_request;
use crate::shell::Shell;

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
                Ok(_) => return,
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
            this.update(cx, |this, cx| {
                for summary in terminals {
                    let record = summary.record;
                    let Some(worktree) = record
                        .worktree
                        .as_ref()
                        .map(|worktree| worktree.path.clone())
                    else {
                        continue;
                    };
                    // An agent's terminal may have got its tab as it opened.
                    if !fresh.contains(&worktree) || this.terminals.tab_of(&record.handle).is_some()
                    {
                        continue;
                    }
                    let name = record.name.clone();
                    // A shell the person opened that a daemon restart ended runs again, from
                    // the screen it had; other ended terminals stay closed.
                    let source = match record.exit {
                        None => Source::Existing {
                            handle: record.handle,
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
                    let id = this.terminals.add(source);
                    this.workspace
                        .add_terminal_tab(&worktree, id, name.into(), false);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Tabs for the person's shells the registry remembers in the `fresh` checkouts, read as
    /// written, since another build wrote it: what replacing its daemon brings back.
    fn add_remembered_shells(&mut self, fresh: &[PathBuf], cx: &mut Context<Self>) {
        let Some(client) = self.terminals.client() else {
            return;
        };
        for record in client.dir().remembered() {
            let worktree = record["worktree"]["path"].as_str().map(PathBuf::from);
            let Some(worktree) = worktree.filter(|path| fresh.contains(path)) else {
                continue;
            };
            let shell = record["command"].as_array().is_some_and(Vec::is_empty)
                && record["created_by"]["kind"] == "human"
                && (record["exit"].is_null() || record["exit"]["daemon_restart"] == true);
            let Some(name) = record["name"].as_str().filter(|_| shell) else {
                continue;
            };
            let request = open_request(&worktree, name.to_owned());
            let id = self.terminals.add(Source::Open(Box::new(request)));
            self.workspace
                .add_terminal_tab(&worktree, id, name.to_owned().into(), false);
        }
        cx.notify();
    }
}
