//! The agent desktop's host (`docs/agent-desktop.md`): a process of the person's, in their
//! session, that makes one extra desktop, starts apps on it, captures and drives their
//! windows, and answers the Koyo app over the link. It is loaded by a small loader app, so
//! the privacy grants given to the loader (Accessibility, Screen Recording) survive every
//! build of this library.

#![cfg(target_os = "macos")]

mod acts;
mod app;
mod apps;
mod asks;
mod ax;
mod capture;
mod clipboard;
mod compare;
mod desktop;
mod focus;
mod front;
mod grants;
mod host;
mod input;
mod keyboard;
mod layout;
mod link;
mod mission;
mod moving;
mod ns;
mod opening;
mod order;
mod run;
mod shot;
mod skylight;
mod snapshot;
mod stats;
mod sys;
mod windows;

use std::ffi::{CStr, c_char, c_int};
use std::path::PathBuf;

/// The host's entry, called by its loader with the process's own arguments:
/// `--link <socket>` (the app's socket) and `--state <folder>` (what the host keeps: the
/// record of the desktop it made, the data folders of apps that need one of their own).
/// With `--stats` it also says, every few seconds, how many frames it sent. With
/// `--grants <file>` instead, it only writes there which privacy grants it holds, and goes;
/// with `--ask <grant>`, it only has macOS list it for that grant, and goes.
/// Returns the process's exit code.
///
/// # Safety
/// `arguments` must hold `count` zero-terminated strings, as a C `main`'s `argv` does.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn koyo_desk_host_main(
    count: c_int,
    arguments: *const *const c_char,
) -> c_int {
    let count = usize::try_from(count).unwrap_or(0);
    let given: Vec<String> = (0..count)
        .filter_map(|index| {
            // SAFETY: the caller's promise: `count` strings, each zero-terminated.
            let text = unsafe { *arguments.add(index) };
            (!text.is_null())
                // SAFETY: as above.
                .then(|| {
                    unsafe { CStr::from_ptr(text) }
                        .to_string_lossy()
                        .into_owned()
                })
        })
        .collect();
    let value = |name: &str| {
        // The last one given: what Koyo passes comes last, after anything put before it.
        let at = given.iter().rposition(|one| one == name)?;
        given.get(at + 1).map(PathBuf::from)
    };
    // A probe: what this process holds now, written to the file named, and nothing else. A
    // host that runs can't tell that Screen Recording was given since it started; a process
    // started afresh can.
    if let Some(file) = value("--grants") {
        let wrote = std::fs::write(file, grants::Grants::read().said());
        return i32::from(wrote.is_err());
    }
    if let Some(grant) = value("--ask") {
        grants::ask(&grant.to_string_lossy());
        return 0;
    }
    let (Some(link), Some(state)) = (value("--link"), value("--state")) else {
        return 64;
    };
    let counting = given.iter().any(|one| one == "--stats");
    match run::run(&link, state, counting) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}
