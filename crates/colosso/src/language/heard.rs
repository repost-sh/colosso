//! Everything a server says that the app follows, as one stream: its progress, its status,
//! the diagnostics it publishes, and what happens to it.

use std::sync::Arc;

use colosso_lsp::lsp_types::notification::{Progress, PublishDiagnostics};
use colosso_lsp::lsp_types::{ProgressParams, PublishDiagnosticsParams};
use colosso_lsp::{Client, Event, ServerStatus, ServerStatusParams};
use futures::StreamExt as _;
use futures::stream::{self, LocalBoxStream};
use gpui_kit::{AsyncApp, WeakEntity};

use super::server::Server;

/// Something the server sent, or that happened to it.
pub(super) enum Heard {
    Progress(Arc<ProgressParams>),
    Status(Arc<ServerStatusParams>),
    Diagnostics(Arc<PublishDiagnosticsParams>),
    Event(Event),
}

/// Everything the server says that the app follows, as one stream.
pub(super) fn hear(client: &Client) -> LocalBoxStream<'static, Heard> {
    stream::select_all([
        (client.subscribe::<Progress>().map(Heard::Progress)).boxed_local(),
        (client.subscribe::<ServerStatus>().map(Heard::Status)).boxed_local(),
        (client.subscribe::<PublishDiagnostics>())
            .map(Heard::Diagnostics)
            .boxed_local(),
        client.events().map(Heard::Event).boxed_local(),
    ])
    .boxed_local()
}

/// Passes on what the server says until it ends.
pub(super) async fn follow(
    this: WeakEntity<Server>,
    mut heard: LocalBoxStream<'static, Heard>,
    cx: &mut AsyncApp,
) {
    while let Some(heard) = heard.next().await {
        let ended = matches!(heard, Heard::Event(Event::Ended(_)));
        if this.update(cx, |this, cx| this.heard(heard, cx)).is_err() || ended {
            return;
        }
    }
}
