//! A program on a terminal of its own, answered as a person would answer it: `sysadminctl
//! -password -` and `dscl -authonly` read the password from their terminal, which keeps it off
//! every command line (argv is readable by every user through `ps`). Rocuronium's
//! `provision-agent-account.sh` does the same with `expect`.

use std::fs::File;
use std::io::{self, Read as _, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};
use std::os::unix::process::CommandExt as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::mac::Ran;

/// `_IO('t', 97)`: make this terminal the caller's controlling one (`sys/ttycom.h`), which
/// the libc crate doesn't carry for Apple targets.
const TIOCSCTTY: libc::c_ulong = 0x2000_7461;

/// How long a program on a terminal may take.
const LIMIT: Duration = Duration::from_secs(120);

/// Runs `program` on a new terminal, writing `secret` and a return each time its output shows
/// `prompt` anew. What it printed comes back as `out`, with the secret taken out should it
/// ever be echoed.
pub fn run_answering(program: &str, args: &[&str], prompt: &str, secret: &str) -> io::Result<Ran> {
    let (master, slave) = open()?;
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::from(slave.try_clone()?))
        .stdout(Stdio::from(slave.try_clone()?))
        .stderr(Stdio::from(slave.try_clone()?));
    // SAFETY: setsid and ioctl are async-signal-safe; the terminal becomes the child's
    // controlling one, where `readpassphrase` looks for it.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(0, TIOCSCTTY, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    drop(slave);
    let mut terminal = File::from(master);
    let mut said = String::new();
    let mut answered = 0;
    let started = Instant::now();
    let mut buffer = [0u8; 4096];
    loop {
        if started.elapsed() > LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{program} didn't finish within {} s", LIMIT.as_secs()),
            ));
        }
        if !readable(&terminal, Duration::from_millis(200))? {
            if child.try_wait()?.is_some() && !readable(&terminal, Duration::ZERO)? {
                break;
            }
            continue;
        }
        let read = match terminal.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            // The other side closed: the program and all it started are gone.
            Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        said.push_str(&String::from_utf8_lossy(&buffer[..read]));
        if said[answered..].contains(prompt) {
            terminal.write_all(secret.as_bytes())?;
            terminal.write_all(b"\r")?;
            answered = said.len();
        }
    }
    let status = child.wait()?;
    Ok(Ran {
        code: status.code().unwrap_or(-1),
        out: said.replace(secret, "…"),
        err: String::new(),
    })
}

fn open() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: both out-pointers are valid; no name, settings or size asked for.
    let opened = unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if opened != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openpty made both descriptors for us, and nothing else owns them.
    Ok(unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) })
}

fn readable(file: &File, wait: Duration) -> io::Result<bool> {
    let mut poll = libc::pollfd {
        fd: file.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let millis = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
    // SAFETY: one valid pollfd.
    let ready = unsafe { libc::poll(&raw mut poll, 1, millis) };
    if ready < 0 {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::Interrupted {
            Ok(false)
        } else {
            Err(error)
        };
    }
    Ok(ready > 0 && poll.revents & (libc::POLLIN | libc::POLLHUP) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prompt_on_the_terminal_is_answered_and_the_secret_stays_out_of_what_it_said() {
        // A stand-in for sysadminctl: asks twice on its terminal, says what it read.
        let script = r#"printf 'User password:'; read -s a; printf '\nUser password:'; read -s b; [ "$a" = "$b" ] && echo "got $a""#;
        let ran = run_answering("/bin/zsh", &["-c", script], "User password:", "s3cret").unwrap();
        assert!(ran.ok(), "{ran:?}");
        assert!(ran.out.contains("got …"), "{}", ran.out);
        assert!(!ran.out.contains("s3cret"));
    }

    #[test]
    fn a_failing_program_says_so() {
        let ran = run_answering("/bin/sh", &["-c", "echo nope; exit 3"], "Password:", "x").unwrap();
        assert_eq!(ran.code, 3);
        assert!(ran.out.contains("nope"));
    }
}
