//! The pseudo-terminal a terminal's program runs in, opened and spawned as agent-terminal
//! does with portable-pty, with the environment `docs/terminals.md` 5.3 lists.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};
use std::path::Path;

use colosso_term_control::{Handle, Size, WorktreeRef};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

/// The program's terminal as the session holds it. Its reads and writes are non-blocking
/// (the reader spins on an empty queue, `docs/terminals.md` 5.3), so it's written to only
/// through `write_all`, which waits for room.
pub struct Pty {
    pub master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    pub pid: u32,
}

/// Writes all of `bytes` to the program, waiting for room while it doesn't read.
pub fn write_all(pty: &mut Pty, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        match pty.writer.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => bytes = &bytes[n..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait_for(descriptor(pty.master.as_ref())?, libc::POLLOUT)?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    pty.writer.flush()
}

/// Waits until `fd` is ready for `events` (`POLLIN`, `POLLOUT`), or its other side closed.
pub fn wait_for(fd: RawFd, events: libc::c_short) -> io::Result<()> {
    let mut fds = [libc::pollfd {
        fd,
        events,
        revents: 0,
    }];
    loop {
        // SAFETY: `poll` reads and writes the one pollfd given; its caller holds `fd` open.
        if unsafe { libc::poll(fds.as_mut_ptr(), 1, -1) } >= 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn descriptor(master: &dyn MasterPty) -> io::Result<RawFd> {
    master
        .as_raw_fd()
        .ok_or_else(|| io::Error::other("the PTY has no descriptor"))
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: `fcntl` reads and sets the flags of a descriptor its caller holds open.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    // SAFETY: as above.
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

impl super::Session {
    /// Hangs the program up and waits up to `grace` for it to end, then kills it.
    pub(super) fn end_program(&mut self, grace: std::time::Duration) {
        let Some(pty) = &self.pty else { return };
        let pid = pty.pid;
        let _ = signal_group(pid, libc::SIGHUP);
        if self.exit_rx.recv_timeout(grace).is_err() {
            let _ = signal_group(pid, libc::SIGKILL);
            let _ = self.exit_rx.recv_timeout(std::time::Duration::from_secs(1));
        }
        self.pty = None;
    }
}

pub struct Spawned {
    pub pty: Pty,
    /// The master's descriptor, the reader's own copy.
    pub reader: File,
    pub child: Box<dyn Child + Send + Sync>,
}

/// Starts `command` (or the login shell when it's empty) in `cwd` at `size`.
pub fn spawn(
    handle: &Handle,
    worktree: Option<&WorktreeRef>,
    command: &[String],
    cwd: &Path,
    extra_env: &BTreeMap<String, String>,
    size: Size,
) -> io::Result<Spawned> {
    if !cwd.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("the working directory {} doesn't exist", cwd.display()),
        ));
    }
    let pair = native_pty_system()
        .openpty(pty_size(size))
        .map_err(io::Error::other)?;
    let mut builder = match command {
        [] => {
            // portable-pty's default program is `$SHELL` as a login shell: run by its path,
            // its name with a leading dash as `argv[0]`. A `SHELL` the caller gave wins.
            let mut builder = CommandBuilder::new_default_prog();
            builder.env("SHELL", login_shell());
            builder
        }
        [program, args @ ..] => {
            let mut builder = CommandBuilder::new(program);
            builder.args(args);
            builder
        }
    };
    builder.cwd(cwd);
    // The program that runs, whose shell integration goes in: the login shell, or the
    // command's own.
    let program = match command {
        [] => extra_env.get("SHELL").cloned().unwrap_or_else(login_shell),
        [program, ..] => program.clone(),
    };
    for (key, value) in environment(handle, worktree, &program, extra_env) {
        builder.env(key, value);
    }
    let child = pair
        .slave
        .spawn_command(builder)
        .map_err(io::Error::other)?;
    drop(pair.slave);
    let reader = reader_of(pair.master.as_ref())?;
    // The flag is the open PTY's, so the writer's too.
    set_nonblocking(reader.as_raw_fd())?;
    let writer = pair.master.take_writer().map_err(io::Error::other)?;
    let pid = child.process_id().unwrap_or(0);
    Ok(Spawned {
        pty: Pty {
            master: pair.master,
            writer,
            pid,
        },
        reader,
        child,
    })
}

/// A copy of `master`'s descriptor for the reader to own: it reads and waits on it for as
/// long as it runs, whatever happens to the master meanwhile.
fn reader_of(master: &dyn MasterPty) -> io::Result<File> {
    let fd = descriptor(master)?;
    // SAFETY: `master` holds `fd` open while it's borrowed here; the copy is the file's own.
    let fd = unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned()?;
    Ok(File::from(fd))
}

pub fn pty_size(size: Size) -> PtySize {
    PtySize {
        rows: size.rows.max(1),
        cols: size.cols.max(1),
        pixel_width: (u32::from(size.cols) * size.cell_width_px).min(u32::from(u16::MAX)) as u16,
        pixel_height: (u32::from(size.rows) * size.cell_height_px).min(u32::from(u16::MAX)) as u16,
    }
}

fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| "/bin/zsh".to_owned())
}

/// The daemon's environment plus what identifies the terminal, and the shell integration for
/// `program`; `TERM` is Ghostty's own when its terminfo could be written, `xterm-256color`
/// otherwise.
fn environment(
    handle: &Handle,
    worktree: Option<&WorktreeRef>,
    program: &str,
    extra: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();
    match colosso_term_engine::terminfo_dir() {
        Ok(dir) => {
            env.push(("TERM".into(), colosso_term_engine::TERM.into()));
            env.push(("TERMINFO".into(), dir.to_string_lossy().into_owned()));
        }
        Err(error) => {
            log::warn!(
                "no terminfo for {}: {error}; children get xterm-256color",
                colosso_term_engine::TERM
            );
            env.push(("TERM".into(), "xterm-256color".into()));
        }
    }
    let resources = colosso_term_engine::shell_integration_dir().ok();
    if let Some(dir) = &resources {
        env.push((
            "GHOSTTY_RESOURCES_DIR".into(),
            dir.to_string_lossy().into_owned(),
        ));
    }
    env.push(("COLORTERM".into(), "truecolor".into()));
    env.push(("TERM_PROGRAM".into(), "Colosso".into()));
    env.push((
        "TERM_PROGRAM_VERSION".into(),
        env!("CARGO_PKG_VERSION").into(),
    ));
    env.push(("COLOSSO_TERMINAL".into(), handle.as_str().to_owned()));
    if let Some(worktree) = worktree {
        env.push((
            "COLOSSO_WORKTREE".into(),
            worktree.path.to_string_lossy().into_owned(),
        ));
    }
    for (key, value) in extra {
        env.push((key.clone(), value.clone()));
    }
    // Last, as it folds the terminal's own ZDOTDIR or XDG_DATA_DIRS into what it sets.
    if let Some(dir) = &resources {
        let get = |key: &str| extra.get(key).cloned().or_else(|| std::env::var(key).ok());
        env.extend(super::integration::variables(program, dir, get));
    }
    env
}

/// Sends `signal` to the child's whole process group, so a shell's foreground job gets it too.
pub fn signal_group(pid: u32, signal: i32) -> io::Result<()> {
    // SAFETY: `killpg` takes two integers and touches no memory.
    let result = unsafe { libc::killpg(pid as libc::pid_t, signal) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Read;
    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    /// What `spawned` prints, a chunk at a time, read on a thread so a test can give up.
    fn printed(spawned: &mut Spawned) -> Receiver<Vec<u8>> {
        let mut reader = spawned.reader.try_clone().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            loop {
                let n = match reader.read(&mut chunk) {
                    Ok(n @ 1..) => n,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        let _ = wait_for(reader.as_raw_fd(), libc::POLLIN);
                        continue;
                    }
                    _ => break,
                };
                if tx.send(chunk[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        rx
    }

    /// Reads into `seen` until `done` holds of it, for up to ten seconds; whether it did.
    fn read_until(
        printed: &Receiver<Vec<u8>>,
        seen: &mut Vec<u8>,
        done: impl Fn(&str) -> bool,
    ) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(&String::from_utf8_lossy(seen)) {
            match printed.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(chunk) => seen.extend_from_slice(&chunk),
                Err(_) => return false,
            }
        }
        true
    }

    #[test]
    fn an_empty_command_is_the_login_shell() {
        let handle = Handle("term_test".into());
        let env = BTreeMap::from([("SHELL".to_owned(), "/bin/sh".to_owned())]);
        let cwd = std::env::current_dir().unwrap();
        let mut spawned = spawn(&handle, None, &[], &cwd, &env, Size::cells(80, 24)).unwrap();
        let printed = printed(&mut spawned);
        let mut seen = Vec::new();
        // Typed once the shell shows its prompt, as a person would: a login shell may drop
        // what came before it was ready.
        let ready = read_until(&printed, &mut seen, |text| text.trim_end().ends_with('$'));
        if ready {
            write_all(&mut spawned.pty, b"echo \"zero=$0\"; exit\n").unwrap();
        }
        let answered = ready && read_until(&printed, &mut seen, |text| text.contains("zero=-sh"));
        let _ = spawned.child.kill();
        let _ = spawned.child.wait();
        let seen = String::from_utf8_lossy(&seen);
        assert!(ready, "the shell showed no prompt: {seen:?}");
        assert!(answered, "not a login shell: {seen:?}");
    }
}
