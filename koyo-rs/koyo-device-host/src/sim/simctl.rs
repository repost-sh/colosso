//! `xcrun simctl --set <Koyo's set>`, one call at a time, each bounded (`docs/devices.md` 4):
//! booting, shutting down, apps, links. Never in an unbounded loop; never on the host's loop,
//! which a call this long would stop: each runs on a thread of its own and its answer comes back.

use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

pub use koyo_device_wire::process::output;

/// Runs `simctl --set set args…` for at most `limit`: its output, or what went wrong in words.
pub fn run(set: &str, args: &[&str], limit: Duration) -> Result<String, String> {
    let mut command = Command::new("/usr/bin/xcrun");
    command.arg("simctl").arg("--set").arg(set).args(args);
    output(command, limit).map_err(|why| format!("simctl {}: {why}", args.first().unwrap_or(&"")))
}

/// Runs `job` on a thread of its own; its answer comes on the receiver.
pub fn later<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> mpsc::Receiver<T> {
    let (done, answer) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(job());
        crate::link::wake();
    });
    answer
}
