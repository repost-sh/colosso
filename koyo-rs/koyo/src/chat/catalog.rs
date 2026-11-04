//! What a checkout's chats can choose from (`docs/chat.md` 4.5.9): the host's catalog for it, asked
//! for each time a tab shows that checkout, and again each time the host is reached, as what it
//! offers may have changed; the host answers from what it keeps. Held for as long as a tab shows
//! it, and read as it's drawn, as it seldom changes.

use std::path::Path;
use std::sync::Arc;

use gpui_kit::{AppContext as _, Context, Entity, WeakEntity};
use koyo_agent::agent_client::AgentClient;
use koyo_agent::protocol::methods::{Catalog, CatalogParams, GetCatalog};

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
        if let Some(client) = self.client.as_ref().filter(|_| self.connection == Connection::Connected) {
            ask_for_catalog(client, cwd, held.clone(), cx);
        }
        held
    }

    /// The host was reached, and says what it offers now to the checkouts tabs still show.
    pub(super) fn refresh_catalogs(&mut self, cx: &mut Context<Self>) {
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
