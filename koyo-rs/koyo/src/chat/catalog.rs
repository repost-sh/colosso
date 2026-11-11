//! What a checkout's chats can choose from (`docs/chat.md` 4.5.9): the host's catalog for it, asked
//! for each time a tab shows that checkout, and again each time the host is reached, as what it
//! offers may have changed; the host answers from what it keeps. Held for as long as a tab shows
//! it, and read as it's drawn, as it seldom changes.

use std::path::Path;
use std::sync::Arc;

use gpui_kit::{AppContext as _, Context, Entity, Task, WeakEntity};
use koyo_agent::agent_client::AgentClient;
use koyo_agent::client::ClientError;
use koyo_agent::protocol::methods::{
    Catalog, CatalogParams, GetCatalog, ProviderState, ProvidersStatus, StatusParams,
};

use super::{Chats, Connection};

impl Chats {
    /// The catalog for the checkout at `cwd`, empty until the host says.
    pub(super) fn catalog(&mut self, cwd: &Path, cx: &mut Context<Self>) -> Entity<Catalog> {
        let held = self.catalogs.get(cwd).and_then(WeakEntity::upgrade);
        let held = held.unwrap_or_else(|| {
            let held = cx.new(|_| Catalog::default());
            self.catalogs.insert(cwd.to_owned(), held.downgrade());
            held
        });
        if let Some(client) = self
            .client
            .as_ref()
            .filter(|_| self.connection == Connection::Connected)
        {
            ask_for_catalog(client, cwd, held.clone(), cx);
        }
        held
    }

    /// The host heard Koyo's settings, and says what it offers now to the checkouts tabs still
    /// show.
    pub(super) fn refresh_catalogs(&mut self, cx: &mut Context<Self>) {
        self.ask_for_provider(false, cx);
        let Some(client) = &self.client else {
            return;
        };
        self.catalogs.retain(|cwd, held| {
            let held = held.upgrade();
            if let Some(held) = &held {
                ask_for_catalog(client, cwd, held.clone(), cx);
            }
            held.is_some()
        });
    }
}

impl Chats {
    /// What a chat in the checkout at `cwd` can run with: the catalog a tab showing it holds,
    /// which its composer goes by, or else the host's models and defaults, asked for now on a
    /// thread of its own, beside what the chats ask in order, and without the folder's commands,
    /// which may take starting the agent there.
    pub(crate) fn ask_catalog(
        &mut self,
        cwd: &Path,
        cx: &mut Context<Self>,
    ) -> Task<Result<Catalog, ClientError>> {
        let held = self.catalogs.get(cwd).and_then(WeakEntity::upgrade);
        let held = held.map(|held| held.read(cx).clone());
        if let Some(held) = held.filter(|held| !held.models.is_empty()) {
            return Task::ready(Ok(held));
        }
        let Some(client) = self.client.clone() else {
            return Task::ready(Err(ClientError::Closed));
        };
        self.connect(cx);
        let params = CatalogParams {
            provider: None,
            cwd: cwd.to_string_lossy().into_owned(),
            commands: Some(false),
        };
        cx.background_spawn(async move { client.request::<GetCatalog>(&params) })
    }

    /// Asks the host whether Claude Code is there, which it looks for each time. Once it turns up
    /// there's something to choose from.
    pub(super) fn ask_for_provider(&self, refresh: bool, cx: &mut Context<Self>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let held = self.claude_missing.clone();
        let params = StatusParams { refresh };
        let asked = cx.background_spawn(async move { client.request::<ProvidersStatus>(&params) });
        cx.spawn(async move |chats, cx| match asked.await {
            Ok(statuses) => {
                let missing = statuses
                    .iter()
                    .any(|status| status.state == ProviderState::Missing);
                let turned_up = held.update(cx, |held, cx| {
                    let was = std::mem::replace(held, missing);
                    if was != missing {
                        cx.notify();
                    }
                    was && !missing
                });
                if turned_up {
                    chats.update(cx, Chats::refresh_catalogs).ok();
                }
            }
            Err(error) => log::warn!("whether Claude Code is there: {error}"),
        })
        .detach();
    }
}

/// Asks the host for the catalog of `cwd`, on its own thread as a first answer takes a while. `held`
/// only hears of an answer that says something new, as a menu open on it closes.
fn ask_for_catalog(
    client: &Arc<AgentClient>,
    cwd: &Path,
    held: Entity<Catalog>,
    cx: &mut Context<Chats>,
) {
    let client = Arc::clone(client);
    let params = CatalogParams {
        provider: None,
        cwd: cwd.to_string_lossy().into_owned(),
        commands: None,
    };
    let asked = cx.background_spawn(async move { client.request::<GetCatalog>(&params) });
    cx.spawn(async move |_, cx| match asked.await {
        Ok(catalog) => held.update(cx, |held, cx| {
            if *held != catalog {
                *held = catalog;
                cx.notify();
            }
        }),
        Err(error) => log::warn!("what a chat can choose from: {error}"),
    })
    .detach();
}
