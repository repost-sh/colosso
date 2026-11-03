//! The app was called Colosso before it was Koyo, and kept its folders under that name: its
//! data in `dev.colosso.desktop` in Application Support, its log in `~/Library/Logs/Colosso`.
//! The first Koyo to start moves them to Koyo's names, so settings, projects, terminals and
//! browser sign-ins carry over. Nothing moves once Koyo has a folder of its own, or while
//! `KOYO_DATA_DIR` or `KOYO_CONFIG_DIR` points the app somewhere else, as tests do.

use std::fs;
use std::io;
use std::path::Path;

use koyo_term_control::{ClientKind, Hello, TermDir};

use crate::settings::APP_ID;

/// The app's folder under the platform's data folder, as Colosso named it.
const FORMER_APP_ID: &str = "dev.colosso.desktop";
/// The log file's name as Colosso named it, with `.log`; Koyo's is `Koyo.log`.
const FORMER_LOG: &str = "Colosso";

/// Moves Colosso's log folder to Koyo's, its log files renamed, before the log opens.
pub fn move_logs() {
    if pointed_elsewhere() {
        return;
    }
    let former = if cfg!(target_os = "macos") {
        dirs::home_dir().map(|home| home.join("Library/Logs/Colosso"))
    } else {
        dirs::data_local_dir().map(|dir| dir.join("Colosso/logs"))
    };
    if let (Some(former), Some(logs)) = (former, crate::logs::logs_dir()) {
        // Nowhere to say it went wrong yet; a log that didn't move is only an old log.
        let _ = move_log_folder(&former, &logs);
    }
}

/// Moves Colosso's data folder to Koyo's, once its terminal daemon has stopped.
pub fn move_data() {
    if pointed_elsewhere() {
        return;
    }
    let Some(data) = dirs::data_dir() else {
        return;
    };
    let stop = |dir: &TermDir| {
        let hello = Hello::new(ClientKind::Cli, "koyo", env!("CARGO_PKG_VERSION"));
        koyo_term_control::client::stop_daemon(dir, &hello).map_err(io::Error::other)
    };
    match move_data_folder(&data.join(FORMER_APP_ID), &data.join(APP_ID), stop) {
        Ok(true) => log::info!("moved Colosso's data to {}", data.join(APP_ID).display()),
        Ok(false) => {}
        Err(error) => log::warn!("couldn't move Colosso's data to Koyo's folder: {error}"),
    }
}

fn pointed_elsewhere() -> bool {
    ["KOYO_DATA_DIR", "KOYO_CONFIG_DIR"]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
}

/// Moves `former` to `data` when only `former` is there, saying whether it did. Colosso's
/// terminal daemon writes to its folder for as long as it runs, so `stop` stops it first:
/// moved from under it, it would make the folder again. Its terminals end, as they would
/// for any daemon of an older build.
fn move_data_folder(
    former: &Path,
    data: &Path,
    stop: impl FnOnce(&TermDir) -> io::Result<()>,
) -> io::Result<bool> {
    if !former.is_dir() || data.exists() {
        return Ok(false);
    }
    stop(&TermDir::at(former.join("term")))?;
    fs::rename(former, data)?;
    // The socket Colosso listened on; Koyo's has its own name, and nothing listens on it.
    let _ = fs::remove_file(data.join("control/colosso-control.sock"));
    Ok(true)
}

/// Moves `former` to `logs` when only `former` is there, `Colosso.log` and
/// `Colosso.log.old` renamed to Koyo's names.
fn move_log_folder(former: &Path, logs: &Path) -> io::Result<()> {
    if !former.is_dir() || logs.exists() {
        return Ok(());
    }
    if let Some(parent) = logs.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(former, logs)?;
    for entry in fs::read_dir(logs)? {
        let name = entry?.file_name();
        if let Some(rest) = name.to_str().and_then(|name| name.strip_prefix(FORMER_LOG)) {
            fs::rename(logs.join(&name), logs.join(format!("Koyo{rest}")))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
