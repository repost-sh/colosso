//! A command run for at most a while (`docs/devices.md` 4): `simctl` and `devicectl` calls, the
//! host's and the app's, each bounded so one that hangs is ended rather than waited on.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Runs `command` for at most `limit`, killing it after: its output, or why not.
pub fn output(mut command: Command, limit: Duration) -> Result<String, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    // The pipes are read beside the wait, so a child that writes a lot never blocks on them.
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(out) = &mut out {
            let _ = out.read_to_string(&mut text);
        }
        let mut errors = String::new();
        if let Some(err) = &mut err {
            let _ = err.read_to_string(&mut errors);
        }
        let _ = said.send((text, errors));
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < limit => {
                std::thread::park_timeout(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("no answer in {} s", limit.as_secs()));
            }
            Err(error) => return Err(error.to_string()),
        }
    };
    let (text, errors) = heard
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or_default();
    if status.success() {
        Ok(text)
    } else {
        Err(errors.trim().to_owned())
    }
}
