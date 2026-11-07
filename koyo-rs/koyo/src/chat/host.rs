//! Finding the agent host, a client for it, what it's told of Koyo's settings, and the loop over
//! what it says.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use gpui_kit::{Context, Task};
use koyo_agent::agent_client::AgentClient;
use koyo_agent::client;
use koyo_agent::protocol::methods::Config;

use super::Chats;
use crate::settings;

/// How long the loop waits between batches: what arrives meanwhile is applied together.
const FRAME: Duration = Duration::from_millis(16);

/// The loop over what the host says: it takes one notice, then all that wait, applies them
/// together and waits out the frame, so that nothing is applied more than once a frame.
pub(super) fn follow(client: &AgentClient, cx: &mut Context<Chats>) -> Task<()> {
    let mut inbox = client.subscribe();
    cx.spawn(async move |this, cx| {
        while let Some(first) = inbox.next().await {
            let mut notices = vec![first];
            while let Ok(notice) = inbox.try_recv() {
                notices.push(notice);
            }
            if this
                .update(cx, |this, cx| this.take_in(notices, cx))
                .is_err()
            {
                return;
            }
            cx.background_executor().timer(FRAME).await;
        }
    })
}

impl Chats {
    /// Gives the client what Koyo's settings say, if that's news: every connection tells the host
    /// first, so the next session it opens follows them and a new chat starts as they say. A host
    /// reached now is told at once.
    pub(super) fn tell_config(&mut self, cx: &mut Context<Self>) {
        let settings = settings::get(cx);
        let config = Config {
            claude_path: settings.claude_path.clone(),
            default_mode: settings.chat_mode,
            // This executable, so a debug build's chats reach this build's `koyo mcp`.
            koyo_tools: (settings.koyo_tools)
                .then(std::env::current_exe)
                .and_then(Result::ok)
                .map(|koyo| koyo.to_string_lossy().into_owned()),
        };
        if self.told_config.as_ref() == Some(&config) {
            return;
        }
        self.told_config = Some(config.clone());
        let Some(told) = (self.client.as_ref()).and_then(|client| client.set_config(config)) else {
            return;
        };
        // What the host offers has Koyo's settings in it, so it's asked for once it's heard them.
        cx.spawn(async move |chats, cx| match told.await {
            Ok(_) => {
                let _ = chats.update(cx, Chats::refresh_catalogs);
            }
            Err(error) => log::warn!("telling the host its settings: {error}"),
        })
        .detach();
    }
}

/// A client of the host in the app's data folder. Tests start no host and read none, as the
/// person's chats aren't theirs.
pub(super) fn agent_client() -> Option<Arc<AgentClient>> {
    let data = settings::data_dir().filter(|_| !cfg!(test))?;
    let program = host_program()?;
    let dir = client::host_dir(&data);
    let folder = dir.clone();
    let made = AgentClient::new(dir, move || client::host_command(&program, &folder));
    made.inspect_err(|error| log::error!("can't make the agent client: {error}"))
        .ok()
}

/// The host's executable: where `KOYO_AGENT_HOST` says, beside the app's own in a bundle, or
/// where `script/agent-host` builds it in the checkout.
fn host_program() -> Option<PathBuf> {
    if let Some(program) = std::env::var_os("KOYO_AGENT_HOST") {
        return Some(program.into());
    }
    (std::env::current_exe().ok())
        .and_then(|executable| host_beside(&executable))
        .or_else(host_in_checkout)
}

fn host_beside(executable: &Path) -> Option<PathBuf> {
    let host = executable.parent()?.join("koyo-agent-host");
    host.is_file().then_some(host)
}

#[cfg(debug_assertions)]
fn host_in_checkout() -> Option<PathBuf> {
    let host =
        Path::new(env!("KOYO_CHECKOUT")).join("koyo-js/koyo-agent-host/dist/koyo-agent-host");
    host.is_file().then_some(host)
}

#[cfg(not(debug_assertions))]
fn host_in_checkout() -> Option<PathBuf> {
    None
}
