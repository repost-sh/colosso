//! What a run says: its rows, printed and appended where the environment names a file, the time,
//! and a simulator's picture saved for a look.

use std::process::Command;
use std::time::Duration;

use koyo_device_wire::{Answer, Ask};

use super::Driver;

/// The time of day, for a log of acts.
pub fn now() -> String {
    let out = Command::new("/bin/date").arg("+%Y-%m-%d %H:%M:%S").output();
    out.map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned()).unwrap_or_default()
}

/// Saves the picture as `<name>.png` in `$KOYO_REAL_SIM_SHOTS`, when it is set: a simulator's,
/// never a real device's.
pub fn shot(host: &mut Driver, name: &str) {
    let Ok(folder) = std::env::var("KOYO_REAL_SIM_SHOTS") else {
        return;
    };
    if let Ok(Answer::Screenshot { png, .. }) =
        host.ask(Ask::Screenshot { scale: 1 }, Duration::from_secs(15))
    {
        let _ = std::fs::create_dir_all(&folder);
        let _ = std::fs::write(format!("{folder}/{name}.png"), png.as_slice());
    }
}

/// A row's result, printed and appended to `$KOYO_REAL_SIM_OUT` when it is set.
pub fn row(id: &str, what: &str, got: &str, pass: bool) {
    let line = format!(
        "{id} | {what} | {got} | {}",
        if pass { "PASS" } else { "FAIL" }
    );
    eprintln!("{line}");
    append("KOYO_REAL_SIM_OUT", &line);
}

/// Appends `line` to the file the environment's `variable` names, when it's set.
pub fn append(variable: &str, line: &str) {
    use std::io::Write as _;
    if let Ok(path) = std::env::var(variable)
        && let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}
