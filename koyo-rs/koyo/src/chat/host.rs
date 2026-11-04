//! Finding the agent host, and a client for it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use koyo_agent::agent_client::AgentClient;
use koyo_agent::client;

use crate::settings;

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
    let host = Path::new(env!("KOYO_CHECKOUT")).join("koyo-js/koyo-agent-host/dist/koyo-agent-host");
    host.is_file().then_some(host)
}

#[cfg(not(debug_assertions))]
fn host_in_checkout() -> Option<PathBuf> {
    None
}
