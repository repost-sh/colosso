//! Reaching the agent host: the first time a chat needs it, again after a connection ended, each
//! wait longer while it can't be reached, and in place of another build's host once the person
//! chose that. Once it answers, the threads it lists are what the chats hold.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{AppContext as _, Context, WeakEntity};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::methods::{Empty, ThreadsWatch};
use koyo_agent::protocol::model::ThreadSummary;

use super::{Chats, Connection, FIRST_WAIT};

/// The longest wait between attempts to reach the host.
const LONGEST_WAIT: Duration = Duration::from_secs(30);

impl Chats {
    /// Reaches the host unless it's reached, or being reached.
    pub fn connect(&mut self, cx: &mut Context<Self>) {
        if matches!(self.connection, Connection::Idle) {
            self.reach(Duration::ZERO, cx);
        }
    }

    /// Stops a host of another build and starts this one's, as the person chose: its running
    /// turns stop.
    pub fn replace_host(&mut self, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.connection = Connection::Connecting;
        cx.notify();
        let stopping = cx.background_spawn(async move { client.replace_host() });
        self.connecting = Some(cx.spawn(async move |this, cx| {
            let stopped = stopping.await;
            this.update(cx, |this, cx| match stopped {
                Ok(()) => this.reach(Duration::ZERO, cx),
                Err(error) => this.failed(&error, cx),
            })
            .ok();
        }));
    }

    /// Asks the host for its threads after `wait`, over and over until it answers, then opens
    /// what was open before: the way back from any connection that ended.
    pub(super) fn reach(&mut self, wait: Duration, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.connection = Connection::Connecting;
        cx.notify();
        self.connecting = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let mut wait = FIRST_WAIT;
            loop {
                let asking = Arc::clone(&client);
                let listed = cx
                    .background_spawn(async move { asking.request::<ThreadsWatch>(&Empty {}) })
                    .await;
                let again = this.update(cx, |this, cx| this.listed(listed, cx));
                if !again.unwrap_or(false) {
                    return;
                }
                cx.background_executor().timer(wait).await;
                wait = (wait * 2).min(LONGEST_WAIT);
            }
        }));
    }

    /// Takes in the host's list of threads; whether to ask again.
    pub(super) fn listed(
        &mut self,
        listed: Result<Vec<ThreadSummary>, ClientError>,
        cx: &mut Context<Self>,
    ) -> bool {
        let summaries = match listed {
            Ok(summaries) => summaries,
            Err(ClientError::OtherBuild) => {
                self.connection = Connection::OtherBuild;
                cx.notify();
                return false;
            }
            Err(error) => {
                self.failed(&error, cx);
                return true;
            }
        };
        self.to_restore.get_or_insert_with(|| {
            let mut tabs: Vec<ThreadId> = (summaries.iter())
                .filter(|summary| summary.tab_open)
                .map(|summary| summary.id.clone())
                .collect();
            // In the order they were made, as the host's ids sort, so they come back the same.
            tabs.sort();
            tabs
        });
        self.summaries = (summaries.into_iter())
            .map(|summary| (summary.id.clone(), summary))
            .collect();
        self.connection = Connection::Connected;
        // What the host offers has Koyo's settings in it, which the connection told it first.
        self.refresh_catalogs(cx);
        for thread in self.threads.values().filter_map(WeakEntity::upgrade) {
            self.open(thread, cx);
        }
        self.summaries_changed(cx);
        cx.notify();
        false
    }

    fn failed(&mut self, error: &ClientError, cx: &mut Context<Self>) {
        log::warn!("can't reach the agent host: {error}");
        self.connection = Connection::Failed(error.to_string().into());
        cx.notify();
    }
}
