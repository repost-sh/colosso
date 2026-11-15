//! The app has kept its folders under other names. As Colosso it kept its data in
//! `dev.colosso.desktop` in Application Support and its log in `~/Library/Logs/Colosso`; as
//! Koyo while the project's domain was koyo.dev, its data in `dev.koyo.desktop`. The first
//! Koyo to start under the koyo.codes id moves the newest of those data folders to its own,
//! `codes.koyo.desktop`, and Colosso's log to Koyo's, so settings, projects, terminals, chats
//! and browser sign-ins carry over. Nothing moves once Koyo has a folder of its own, while an
//! older Koyo still runs on its folder, or while `KOYO_DATA_DIR` or `KOYO_CONFIG_DIR` points
//! the app somewhere else, as tests do.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use koyo_agent::client::{host_dir, stop_host};
use koyo_app_control::Endpoint;
use koyo_daemon::{APP_ID, FORMER_APP_ID};
use koyo_term_control::{ClientKind, Hello, TermDir};

/// The app's folder under the platform's data folder, as Colosso named it.
const COLOSSO_APP_ID: &str = "dev.colosso.desktop";
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

/// Moves the newest former data folder to Koyo's, once what ran in it has stopped.
pub fn move_data() {
    if pointed_elsewhere() {
        return;
    }
    let Some(base) = dirs::data_dir() else {
        return;
    };
    match move_newest(&base, older_koyo_runs, stop_daemons) {
        Ok(Some(former)) => log::info!(
            "moved {} to {}",
            former.display(),
            base.join(APP_ID).display()
        ),
        Ok(None) => {}
        Err(error) => log::warn!("couldn't move the former data folder to Koyo's: {error}"),
    }
}

fn pointed_elsewhere() -> bool {
    ["KOYO_DATA_DIR", "KOYO_CONFIG_DIR"]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
}

/// Moves the newest former folder in `base` to Koyo's when Koyo has none, saying which it
/// moved. The koyo.dev folder is newer than Colosso's, which stays where it is beside it, as it
/// did beside Koyo's own. An older Koyo that `runs` on the koyo.dev folder keeps it in place:
/// moved from under it, its terminals and chats would stop mid-use and it would make the folder
/// again. This build uses that folder meanwhile (`app_folder`), and a start after that Koyo has
/// quit moves it.
fn move_newest(
    base: &Path,
    runs: impl FnOnce(&Path) -> bool,
    stop: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<Option<PathBuf>> {
    let data = base.join(APP_ID);
    if data.exists() {
        return Ok(None);
    }
    let koyo = base.join(FORMER_APP_ID);
    let former = if koyo.is_dir() {
        if runs(&koyo) {
            log::info!(
                "an older Koyo runs on {}: it moves once that Koyo has quit",
                koyo.display()
            );
            return Ok(None);
        }
        koyo
    } else {
        base.join(COLOSSO_APP_ID)
    };
    Ok(move_data_folder(&former, &data, stop)?.then_some(former))
}

/// Whether a Koyo answers on the control socket of the folder `former`, as every running Koyo
/// serves its folder's (`control::start`).
fn older_koyo_runs(former: &Path) -> bool {
    Endpoint::for_data_dir(former).is_ok_and(|endpoint| answers(endpoint.path()))
}

/// Whether something listens on the socket `path`.
fn answers(path: &Path) -> bool {
    koyo_daemon::connect(path).is_ok()
}

/// Stops what an older build may still run in its folder `former` and write to: the terminal
/// daemon and the agent host, which both outlive the app. Their terminals end and their running
/// turns stop, as "Restart terminals" and "Restart chats" would end them; what they kept moves
/// with the folder.
fn stop_daemons(former: &Path) -> io::Result<()> {
    let hello = Hello::new(ClientKind::Cli, "koyo", env!("CARGO_PKG_VERSION"));
    let term = TermDir::at(former.join("term"));
    koyo_term_control::client::stop_daemon(&term, &hello).map_err(io::Error::other)?;
    stop_host(&host_dir(former)).map_err(io::Error::other)
}

/// Moves `former` to `data` when only `former` is there, saying whether it did. What an older
/// build runs in `former` writes to it for as long as it runs, so `stop` stops it first: moved
/// from under it, it would make the folder again.
fn move_data_folder(
    former: &Path,
    data: &Path,
    stop: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<bool> {
    if !former.is_dir() || data.exists() {
        return Ok(false);
    }
    stop(former)?;
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
