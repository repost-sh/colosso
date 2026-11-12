//! The host of one device in Koyo (`docs/devices.md` 3): a child process of the app, one per open
//! device, so a crash in a private framework takes one device's tab and nothing else. This build
//! drives Koyo's own simulators; a real iPhone or iPad comes in milestone 6.

#![cfg(target_os = "macos")]

mod acting;
mod device;
mod gpu;
mod host;
mod jobs;
mod link;
mod look;
mod phone;
pub mod places;
mod plan;
mod ring;
mod run;
mod sim;
mod start;
mod sys;
mod text;
mod xpc;

pub use run::{ENV_SOCKET_FD, ENV_TOKEN, NEEDS_XCODE};
pub use sim::access::ENV_RESTS_MS;

/// The host's entry: `args` without the program's name. Exits 0 when the app said quit or went
/// away, 1 when it couldn't run, 64 when started wrong.
pub fn main(args: &[String]) -> std::process::ExitCode {
    // The watcher is a program of its own: `--watch` alone.
    if let [only] = args
        && only == "--watch"
    {
        return match phone::watch::watch() {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(why) => {
                log(&why);
                std::process::ExitCode::FAILURE
            }
        };
    }
    let args = match run::Args::parse(args) {
        Ok(args) => args,
        Err(why) => {
            log(&why);
            return std::process::ExitCode::from(64);
        }
    };
    match run::run(&args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(why) => {
            log(&why);
            std::process::ExitCode::FAILURE
        }
    }
}

/// What the host can't say on the link goes to its standard error, which the app keeps in the
/// host's log.
fn log(text: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "koyo-device-host: {text}");
}
