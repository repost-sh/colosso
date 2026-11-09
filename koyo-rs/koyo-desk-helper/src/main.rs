//! `koyo-desk-helper`, Koyo's root helper for agent desktops (`docs/agent-desktop.md` 5).
//!
//!   koyo-desk-helper setup --person-uid UID [--host BUNDLE] [--accept-screensharing-risk]
//!   koyo-desk-helper serve            the daemon launchd keeps
//!   koyo-desk-helper remove           all but the user record (a broken install's way out)
//!   koyo-desk-helper status
//!
//! `setup` and `remove` print one JSON line, a [`HelperAnswer`] (`done` or `failed`), for the
//! app to show.

mod account;
mod console;
mod files;
mod firewall;
mod mac;
mod plist;
mod pty;
mod remove;
#[cfg(test)]
mod script;
mod serve;
mod session;
mod setup;
mod sharing;
mod work;

use std::io::Write as _;
use std::process::ExitCode;

use koyo_desk_wire::helper::HelperAnswer;
use koyo_desk_wire::holder::line;
use mac::Real;
use work::{Outcome, Work};

/// A line in the helper's log (launchd sends stderr to the shared folder's `helper.log`).
pub fn note(text: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default();
    let _ = writeln!(std::io::stderr(), "[{now}] {text}");
}

/// The outcome as one answer line on stdout. The app's administrator dialog runs the helper so
/// that its stdout comes back whatever its exit code.
fn say(result: Outcome<()>, steps: Vec<String>) -> ExitCode {
    let (answer, code) = match result {
        Ok(()) => (HelperAnswer::Done { steps }, ExitCode::SUCCESS),
        Err(message) => (HelperAnswer::Failed { message, steps }, ExitCode::FAILURE),
    };
    let _ = write!(std::io::stdout(), "{}", line(&answer).unwrap_or_default());
    code
}

fn is_root() -> bool {
    // SAFETY: always safe.
    unsafe { libc::geteuid() == 0 }
}

fn setup_from(mut args: impl Iterator<Item = String>) -> Outcome<setup::Setup> {
    let helper = std::env::current_exe()
        .map_err(|error| format!("the helper's own path: {error}"))?
        .to_string_lossy()
        .into_owned();
    let mut setup = setup::Setup {
        person_uid: 0,
        helper,
        host: None,
        accept_risk: false,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--person-uid" => {
                setup.person_uid = args
                    .next()
                    .and_then(|uid| uid.parse().ok())
                    .ok_or("--person-uid takes a user id")?;
            }
            "--host" => setup.host = Some(args.next().ok_or("--host takes a bundle")?),
            "--accept-screensharing-risk" => setup.accept_risk = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(setup)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    if command != "status" && !is_root() {
        return say(Err("koyo-desk-helper runs as root".into()), Vec::new());
    }
    let mut mac = Real;
    let (result, steps) = match command.as_str() {
        "setup" => {
            let mut work = Work::new(&mut mac);
            let result = setup_from(args).and_then(|setup| setup::run(&mut work, &setup));
            (result, work.steps)
        }
        "remove" => {
            let mut work = Work::new(&mut mac);
            let result = files::load_state(&mut work)
                .ok_or_else(|| "not set up".to_owned())
                .and_then(|state| remove::run(&mut work, &state, None));
            (result, work.steps)
        }
        "serve" => {
            return match serve::main() {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    note(&message);
                    ExitCode::FAILURE
                }
            };
        }
        "status" => {
            let mut work = Work::new(&mut mac);
            let status = files::load_state(&mut work).map(|state| serve::status(&mut work, &state));
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                serde_json::to_string(&status).unwrap_or_default()
            );
            return ExitCode::SUCCESS;
        }
        _ => (
            Err("commands: setup, serve, remove, status".to_owned()),
            Vec::new(),
        ),
    };
    say(result, steps)
}
