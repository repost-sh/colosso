//! Setting up (`docs/agent-desktop.md` 5.1): the helper's `setup`, run as root through the
//! system's administrator dialog. `osascript … with administrator privileges` is the one way
//! that works for an ad-hoc build; a signed Koyo registers an `SMAppService` daemon instead
//! (5.6, not built).

use std::path::{Path, PathBuf};
use std::process::Command;

use koyo_desk_wire::helper::HelperAnswer;

use super::Outcome;

/// The helper's executable: where `KOYO_DESK_HELPER` says, or beside the app's own, where
/// `script/bundle` puts it and `cargo build` builds it.
pub fn helper_program() -> Option<PathBuf> {
    if let Some(program) = std::env::var_os("KOYO_DESK_HELPER") {
        return Some(program.into());
    }
    let beside = std::env::current_exe()
        .ok()?
        .parent()?
        .join("koyo-desk-helper");
    beside.is_file().then_some(beside)
}

/// The script that asks for the administrator: the helper and the person's uid come in as
/// arguments, quoted by `AppleScript` itself, so nothing is spliced into a command line. The
/// helper's answer line is what it hands back, whatever the helper's exit code (`|| true`):
/// `do shell script` would otherwise hand back only its stderr.
const SCRIPT: [&str; 3] = [
    "on run argv",
    "do shell script (quoted form of item 1 of argv) & \" setup --person-uid \" & (item 2 of argv) & \" || true\" with prompt \"Koyo sets up its agent desktop: a hidden account for agents, Screen Sharing for that account on this Mac alone, and a helper.\" with administrator privileges",
    "end run",
];

pub fn run() -> Outcome {
    let Some(helper) = helper_program() else {
        return Outcome::failed("the helper isn't built: script/cargo build -p koyo-desk-helper");
    };
    // SAFETY: always safe; the app runs as the person.
    let uid = unsafe { libc::getuid() };
    run_with(&helper, uid)
}

fn run_with(helper: &Path, uid: u32) -> Outcome {
    let mut command = Command::new("/usr/bin/osascript");
    for line in SCRIPT {
        command.arg("-e").arg(line);
    }
    let output = match command.arg(helper).arg(uid.to_string()).output() {
        Ok(output) => output,
        Err(error) => return Outcome::failed(format!("osascript didn't start: {error}")),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The person's Cancel is AppleScript's error -128.
    if stderr.contains("-128") {
        return Outcome::failed("Setting up was cancelled.");
    }
    let answer = stdout
        .lines()
        .find_map(|line| serde_json::from_str::<HelperAnswer>(line.trim()).ok());
    match answer {
        Some(answer) => Outcome::from_helper(Ok(answer), "Set up."),
        None => Outcome::failed(format!(
            "the helper said nothing readable: {}",
            stderr.trim()
        )),
    }
}
