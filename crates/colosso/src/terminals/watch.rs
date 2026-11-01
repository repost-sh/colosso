//! The app follows what the daemon says of every terminal (`docs/terminals.md` 7.4, 9): its
//! list once, and again after it comes back, then its events, into `Terminals::states`. A
//! terminal an agent opens gets a tab of its checkout, which doesn't take the focus, and a
//! terminal closed in the daemon takes its tab with it, drawn or not.

use std::sync::Arc;
use std::time::Duration;

use colosso_term::{Notice, Source, TermClient};
use colosso_term_control::{
    ActorKind, Event, EventKind, RequestBody, ResponseBody, TerminalRecord,
};
use futures::StreamExt as _;
use gpui_kit::{AppContext as _, AsyncApp, Context, WeakEntity};

use super::states::NOTICE_SECS;
use crate::shell::Shell;

impl Shell {
    /// Starts following the daemon's terminals, once.
    pub(super) fn watch_terminals(&mut self, cx: &mut Context<Self>) {
        if self.terminals.watching.is_some() {
            return;
        }
        let Some(client) = self.terminals.client() else {
            return;
        };
        let mut inbox = client.subscribe();
        let task = cx.spawn(async move |this, cx| {
            relist(&this, &client, cx).await;
            while let Some(notice) = inbox.next().await {
                match notice {
                    Notice::Event(event) => {
                        let heard = this.update(cx, |this, cx| this.terminal_notice(&event, cx));
                        if heard.is_err() {
                            break;
                        }
                    }
                    // The views attach again, which starts it again; then it lists anew.
                    Notice::Disconnected => {
                        cx.background_executor().timer(Duration::from_secs(1)).await;
                        relist(&this, &client, cx).await;
                    }
                }
            }
        });
        self.terminals.watching = Some(task);
    }

    fn terminal_notice(&mut self, event: &Event, cx: &mut Context<Self>) {
        match &event.kind {
            EventKind::Opened { terminal } if terminal.created_by.kind == ActorKind::Agent => {
                self.reveal(terminal, cx);
            }
            EventKind::Closed => {
                if let Some(id) = self.terminals.tab_of(&event.handle) {
                    self.close_terminal_tab(id, cx);
                }
            }
            _ => {}
        }
        let before = self.terminals.states.takeover.clone();
        if self.terminals.states.apply(event) {
            cx.notify();
        }
        let after = &self.terminals.states.takeover;
        if after.is_some() && *after != before {
            // The notice of a takeover goes by itself: draw again when it's due to.
            cx.spawn(async move |this, cx| {
                let due = Duration::from_secs(NOTICE_SECS) + Duration::from_millis(100);
                cx.background_executor().timer(due).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            })
            .detach();
        }
    }
}

impl Shell {
    /// Adds a tab for `record` on its checkout, unless one shows it already.
    fn reveal(&mut self, record: &TerminalRecord, cx: &mut Context<Self>) {
        let Some(worktree) = record
            .worktree
            .as_ref()
            .map(|worktree| worktree.path.clone())
        else {
            return;
        };
        if self.terminals.tab_of(&record.handle).is_some() {
            return;
        }
        let id = self.terminals.add(Source::Existing {
            handle: record.handle.clone(),
            name: record.name.clone(),
        });
        let title = record.name.clone().into();
        self.workspace.add_terminal_tab(&worktree, id, title, false);
        cx.notify();
    }
}

/// Starts the states over from the daemon's list, when a daemon runs.
async fn relist(this: &WeakEntity<Shell>, client: &Arc<TermClient>, cx: &mut AsyncApp) {
    let client = Arc::clone(client);
    let listed = cx
        .background_spawn(async move {
            client.request_if_running(RequestBody::ListTerminals {
                worktree: None,
                status: None,
            })
        })
        .await;
    match listed {
        Ok(Some(ResponseBody::Terminals { terminals })) => {
            this.update(cx, |this, cx| {
                let records = terminals.into_iter().map(|summary| summary.record);
                this.terminals.states.reset(records);
                cx.notify();
            })
            .ok();
        }
        Ok(_) => {}
        Err(error) => log::warn!("couldn't list the terminals: {error}"),
    }
}
