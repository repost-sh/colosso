//! Resource subscriptions, both as `resources/subscribe` (2025-11-25) and as
//! `subscriptions/listen` streams (2026-07-28). A terminal resource is watched on the daemon
//! connection; an app resource by following its topic on the app. Either way the watch says
//! what changed, and each subscription passes on what it asked for.

use std::collections::BTreeSet;
use std::sync::Arc;

use colosso_app_control::Topic;
use rmcp::ErrorData;
use rmcp::model::ResourceUpdatedNotificationParam;
use rmcp::service::{Peer, RequestContext, RoleServer, SubscriptionContext};
use tokio::sync::broadcast::error::RecvError;

use crate::resources::{self, Uri};
use crate::server::{Server, joined};
use crate::watch::Update;

impl Server {
    /// Watches `uris`, and the terminal list when `list`. A terminal resource or the list
    /// needs the daemon connection, whose events say what changed; an app resource needs its
    /// topic followed on the app. Nothing stays watched when either fails.
    pub(crate) async fn watch(
        &self,
        uris: Vec<Uri>,
        list: bool,
        context: &RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let (app, terminals): (Vec<Uri>, Vec<Uri>) =
            uris.into_iter().partition(|uri| matches!(uri, Uri::App(_)));
        if list || !terminals.is_empty() {
            let executor = self.executor(context).await?;
            let (watch, adding) = (Arc::clone(&self.inner.watch), terminals.clone());
            tokio::task::spawn_blocking(move || {
                for uri in &adding {
                    watch.add(uri, Some(&executor));
                }
            })
            .await
            .map_err(joined)?;
        }
        if app.is_empty() {
            return Ok(());
        }
        for uri in &app {
            self.inner.watch.add(uri, None);
        }
        let topics: Vec<Topic> = topics(&app).into_iter().collect();
        let client = self.identity(context);
        let followed = (self.inner.app)
            .follow(client, topics, &self.inner.watch)
            .await;
        if let Err(error) = followed {
            for uri in app.iter().chain(&terminals) {
                self.inner.watch.remove(uri);
            }
            return Err(resources::app_error(&error));
        }
        Ok(())
    }

    /// Stops watching `uris`, and following the app topics no subscription needs any more.
    pub(crate) async fn unwatch(&self, uris: &[Uri]) {
        for uri in uris {
            self.inner.watch.remove(uri);
        }
        let wanted = self.inner.watch.app_topics();
        let unused: Vec<Topic> = (topics(uris).into_iter())
            .filter(|topic| !wanted.contains(topic))
            .collect();
        self.inner.app.unfollow(unused).await;
    }

    /// Forwards updates to the `resources/subscribe` subscriptions, from the first one on.
    pub(crate) fn forward(&self, peer: Peer<RoleServer>) {
        let mut forwarding = self.inner.forwarding.lock();
        if *forwarding {
            return;
        }
        *forwarding = true;
        let inner = Arc::clone(&self.inner);
        let mut updates = inner.watch.updates();
        tokio::spawn(async move {
            loop {
                let sent = match updates.recv().await {
                    Ok(Update::List) => peer.notify_resource_list_changed().await,
                    Ok(Update::Resource(uri)) => {
                        let wanted = inner.subscribed.lock().contains(&uri);
                        if !wanted {
                            continue;
                        }
                        let update = ResourceUpdatedNotificationParam::new(uri.text());
                        peer.notify_resource_updated(update).await
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return,
                };
                if sent.is_err() {
                    return;
                }
            }
        });
    }

    /// Serves one `subscriptions/listen` stream until the client cancels it.
    pub(crate) async fn listen_until_cancelled(
        &self,
        context: &SubscriptionContext,
    ) -> Result<(), ErrorData> {
        let accepted = context.accepted();
        let list = accepted.resources_list_changed == Some(true);
        let uris: Vec<Uri> = (accepted.resource_subscriptions.iter().flatten())
            .filter_map(|uri| Uri::parse(uri))
            .collect();
        let mut updates = self.inner.watch.updates();
        self.watch(uris.clone(), list, context.request_context())
            .await?;
        loop {
            let update = tokio::select! {
                () = context.cancelled() => break,
                update = updates.recv() => update,
            };
            let sent = match update {
                Ok(Update::List) if list => context.sink().notify_resource_list_changed().await,
                Ok(Update::Resource(uri)) if uris.contains(&uri) => {
                    context.sink().notify_resource_updated(uri.text()).await
                }
                Ok(_) | Err(RecvError::Lagged(_)) => Ok(()),
                Err(RecvError::Closed) => break,
            };
            if sent.is_err() {
                break;
            }
        }
        self.unwatch(&uris).await;
        Ok(())
    }
}

/// `uri` as a resource that can be subscribed to.
pub(crate) fn subscribable(uri: &str) -> Result<Uri, ErrorData> {
    (Uri::parse(uri).filter(Uri::subscribable))
        .ok_or_else(|| ErrorData::invalid_params(format!("{uri} can't be subscribed to"), None))
}

/// The app topics `uris` need.
fn topics(uris: &[Uri]) -> BTreeSet<Topic> {
    (uris.iter())
        .filter_map(|uri| match uri {
            Uri::App(resource) => Some(resource.topic()),
            _ => None,
        })
        .collect()
}
