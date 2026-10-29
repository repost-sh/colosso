//! A server's life: initialized with what the client can do, shut down when done with, and
//! killed when it won't go.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use lsp_types::notification::{Exit as ExitNotification, Initialized};
use lsp_types::request::{Initialize, Shutdown};
use lsp_types::{
    ClientInfo, InitializeParams, InitializeResult, InitializedParams, PositionEncodingKind,
    WorkspaceFolder,
};

use crate::capabilities::client_capabilities;
use crate::client::{Client, Exit};
use crate::error::LspError;
use crate::process::Signal;
use crate::timer::within;
use crate::uri;

impl Client {
    /// Initializes the server for the workspace at `root`, and tells it it's initialized.
    /// Afterwards [`Client::encoding`] is the position encoding the server chose.
    pub async fn initialize(&self, root: &Path) -> Result<InitializeResult, LspError> {
        let root_uri = uri::from_path(root).ok_or_else(|| LspError::BadRoot(root.to_owned()))?;
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        #[expect(
            deprecated,
            reason = "older servers still find the workspace by rootUri alone"
        )]
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_uri: Some(root_uri.clone()),
            initialization_options: self.inner.initialization_options.clone(),
            capabilities: client_capabilities(),
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: root_uri,
                name,
            }]),
            client_info: Some(ClientInfo {
                name: "Colosso".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
            ..InitializeParams::default()
        };
        let result = self.request::<Initialize>(&params).await?;
        let shared = &self.inner.shared;
        if let Some(chosen) = &result.capabilities.position_encoding {
            let offered = [PositionEncodingKind::UTF8, PositionEncodingKind::UTF16];
            if !offered.contains(chosen) {
                log::warn!(
                    "{} chose {chosen:?}, which wasn't offered; using UTF-16",
                    shared.name
                );
            }
        }
        // Set once: a second initialize is the protocol's error, and the server's to answer.
        let _ = shared
            .capabilities
            .set(Arc::new(result.capabilities.clone()));
        self.notify::<Initialized>(&InitializedParams {})?;
        Ok(result)
    }

    /// Asks the server to shut down and exit, and kills it if it hasn't within `timeout`
    /// for each step. Resolves once the process has ended.
    pub async fn shutdown(&self, timeout: Duration) -> Exit {
        let shared = &self.inner.shared;
        shared.stopping();
        let exited = self.exited();
        if !shared.has_exited() {
            // The answer doesn't matter: the server exits next either way.
            let _ = within(timeout, self.request::<Shutdown>(&())).await;
            let _ = self.notify::<ExitNotification>(&());
        }
        let mut exited = std::pin::pin!(exited);
        if let Some(exit) = within(timeout, exited.as_mut()).await {
            return exit;
        }
        log::warn!("{} didn't exit within {timeout:?}; killing it", shared.name);
        self.kill();
        exited.await
    }

    /// Kills the server at once. [`Client::exited`] says when it's gone.
    pub(crate) fn kill(&self) {
        self.inner.shared.stopping();
        let _ = self.inner.signal.send(Signal::Kill(None));
    }
}
