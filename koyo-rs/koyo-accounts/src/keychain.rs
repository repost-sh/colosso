//! Port of claude-swap's `macos_keychain.py` (0fe22a8): the Keychain through the `security`
//! tool, shaped as Claude Code's own `macOsKeychainStorage.ts` does it.
//!
//! - A write hex-encodes the value (`-X`) and pipes the command through `security -i`, so the
//!   secret never appears in a process's arguments.
//! - A read is `find-generic-password … -w`; exit 44 is "not found", any other failure raises,
//!   so a missing item is never confused with a locked or denied Keychain.
//! - `/usr/bin/security` by absolute path, so nothing earlier on `PATH` sees a secret; and the
//!   item's creator and reader are the same stable binary, so reads stay silent while Koyo's
//!   own signature changes with every build (pitfall 12).
//! - Every call is bounded by 5 s, so a wedged Keychain can't hang the caller.
//!
//! One rule differs from cswap's: a command longer than `security -i`'s 4096-byte line isn't
//! retried with the secret in the arguments, as cswap and Claude Code do; it fails, because
//! Koyo puts no token in a process's arguments (`docs/claude-accounts.md` 3).
//!
//! `security` itself is behind [`Security`], so tests run on a fake (`testing::FakeSecurity`).
//! Left out: `item_exists`, which nothing calls.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use crate::oauth::hex;

/// `security -i` reads stdin with a 4096-byte `fgets` buffer; a longer line is cut mid-argument
/// and fails to write (Claude Code #30337). 64 bytes of headroom for line-ending accounting.
pub const SECURITY_STDIN_LINE_LIMIT: usize = 4096 - 64;
/// `errSecItemNotFound`, as `find-` and `delete-generic-password` exit with it.
pub const NOT_FOUND_RC: i32 = 44;
/// The most a `security` call may take: a healthy Keychain answers in well under 100 ms.
pub const TIMEOUT: Duration = Duration::from_secs(5);
pub const SECURITY: &str = "/usr/bin/security";

/// What `security` answered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// A `security` call that failed for a reason other than "not found": a non-zero exit, a
/// timeout, or no binary to run. cswap's `KeychainError` and `KEYCHAIN_ERRORS` in one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeychainError(pub String);

impl std::fmt::Display for KeychainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for KeychainError {}

/// The `security` tool: `args` after the program's name, `input` on stdin, within [`TIMEOUT`].
pub trait Security: Send + Sync {
    /// # Errors
    /// A timeout, or a binary that couldn't be run; an exit code is an [`Output`].
    fn run(&self, args: &[&str], input: Option<&str>) -> Result<Output, KeychainError>;

    /// The keychain every call names, for a test on a temporary keychain; with none, the
    /// person's search list, as cswap's calls.
    fn keychain(&self) -> Option<&Path> {
        None
    }
}

/// `/usr/bin/security`, or a temporary keychain through it.
#[derive(Default)]
pub struct SystemSecurity {
    pub keychain: Option<PathBuf>,
}

impl Security for SystemSecurity {
    fn run(&self, args: &[&str], input: Option<&str>) -> Result<Output, KeychainError> {
        let mut child = Command::new(SECURITY)
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| KeychainError(format!("couldn't run {SECURITY}: {error}")))?;
        if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
            // A command is one line, well within what the pipe holds.
            stdin
                .write_all(input.as_bytes())
                .map_err(|error| KeychainError(format!("couldn't talk to {SECURITY}: {error}")))?;
        }
        let pid = child.id();
        let (sent, answer) = mpsc::channel();
        std::thread::spawn(move || {
            let mut stdout = String::new();
            let mut stderr = String::new();
            if let Some(mut out) = child.stdout.take() {
                out.read_to_string(&mut stdout).ok();
            }
            if let Some(mut err) = child.stderr.take() {
                err.read_to_string(&mut stderr).ok();
            }
            let status = child.wait().ok().and_then(|status| status.code());
            sent.send((status, stdout, stderr)).ok();
        });
        if let Ok((status, stdout, stderr)) = answer.recv_timeout(TIMEOUT) {
            return Ok(Output {
                status: status.unwrap_or(-1),
                stdout,
                stderr,
            });
        }
        // SAFETY: signalling the child this call started and still waits on.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
        Err(KeychainError(format!(
            "{} timed out after {}s",
            args.first().copied().unwrap_or("security"),
            TIMEOUT.as_secs()
        )))
    }

    fn keychain(&self) -> Option<&Path> {
        self.keychain.as_deref()
    }
}

/// The active credential's account, as Claude Code's `getUsername()`: `$USER`, then the OS
/// user, then `claude-code-user`. Matching it exactly matters where `$USER` is unset (launchd):
/// another default would key an item Claude Code never reads.
pub fn keychain_account_name(user: Option<&str>) -> String {
    if let Some(user) = user.filter(|user| !user.is_empty()) {
        return user.to_owned();
    }
    os_user().unwrap_or_else(|| "claude-code-user".to_owned())
}

fn os_user() -> Option<String> {
    // SAFETY: `getpwuid` returns a pointer into static storage or null; the name is copied out
    // before anything else could call it on this thread.
    unsafe {
        let entry = libc::getpwuid(libc::geteuid());
        if entry.is_null() || (*entry).pw_name.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr((*entry).pw_name)
            .to_str()
            .ok()
            .map(str::to_owned)
    }
}

/// A value quoted for a `security -i` line, which is re-parsed shell-style: in double quotes,
/// `"` and `\` escaped (the active credential's service has a space).
pub fn quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn with_keychain<'a>(security: &'a dyn Security, mut args: Vec<&'a str>) -> Vec<&'a str> {
    if let Some(keychain) = security.keychain().and_then(Path::to_str) {
        args.push(keychain);
    }
    args
}

fn failed(verb: &str, output: &Output) -> KeychainError {
    KeychainError(format!(
        "security {verb} failed (rc={}): {}",
        output.status,
        output.stderr.trim()
    ))
}

/// The stored value, or `None` when there's no such item (exit 44).
///
/// # Errors
/// Any other exit (locked, denied, unavailable) or a timeout, so a miss isn't confused with a
/// failure.
pub fn get_password(
    security: &dyn Security,
    service: &str,
    account: &str,
) -> Result<Option<String>, KeychainError> {
    let args = with_keychain(
        security,
        vec!["find-generic-password", "-a", account, "-w", "-s", service],
    );
    let output = security.run(&args, None)?;
    match output.status {
        // `-w` prints the value and one newline: exactly that is taken off.
        0 => Ok(Some(
            output
                .stdout
                .strip_suffix('\n')
                .unwrap_or(&output.stdout)
                .to_owned(),
        )),
        NOT_FOUND_RC => Ok(None),
        _ => Err(failed("find-generic-password", &output)),
    }
}

/// Creates or updates the item (`-U`), through `security -i` so the secret stays out of the
/// arguments.
///
/// # Errors
/// A non-zero exit, a timeout, or a command too long for `security -i`'s line.
pub fn set_password(
    security: &dyn Security,
    service: &str,
    account: &str,
    password: &str,
) -> Result<(), KeychainError> {
    let hex = hex(password.as_bytes());
    let keychain = security
        .keychain()
        .and_then(Path::to_str)
        .map(|keychain| format!(" {}", quote(keychain)))
        .unwrap_or_default();
    let command = format!(
        "add-generic-password -U -a {} -s {} -X {hex}{keychain}\n",
        quote(account),
        quote(service)
    );
    if command.len() > SECURITY_STDIN_LINE_LIMIT {
        return Err(KeychainError(format!(
            "the credential is {} bytes, too long for security -i's line, and Koyo puts no \
             token in a command's arguments",
            password.len()
        )));
    }
    let output = security.run(&["-i"], Some(&command))?;
    if output.status == 0 {
        Ok(())
    } else {
        Err(failed("add-generic-password", &output))
    }
}

/// Deletes the item; one already absent (exit 44) counts as deleted.
///
/// # Errors
/// Any other exit, or a timeout.
pub fn delete_password(
    security: &dyn Security,
    service: &str,
    account: &str,
) -> Result<(), KeychainError> {
    let args = with_keychain(
        security,
        vec!["delete-generic-password", "-a", account, "-s", service],
    );
    let output = security.run(&args, None)?;
    if matches!(output.status, 0 | NOT_FOUND_RC) {
        Ok(())
    } else {
        Err(failed("delete-generic-password", &output))
    }
}

#[cfg(test)]
mod tests;
