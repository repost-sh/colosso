//! The pseudo-terminal a terminal's program runs in, opened and spawned as agent-terminal
//! does with portable-pty, with the environment `docs/terminals.md` 5.3 lists.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::Path;

use colosso_term_control::{Handle, Size, WorktreeRef};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub struct Spawned {
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub reader: Box<dyn Read + Send>,
    pub child: Box<dyn Child + Send + Sync>,
    pub pid: Option<u32>,
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
    for (key, value) in environment(handle, worktree, extra_env) {
        builder.env(key, value);
    }
    let child = pair
        .slave
        .spawn_command(builder)
        .map_err(io::Error::other)?;
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
    let writer = pair.master.take_writer().map_err(io::Error::other)?;
    let pid = child.process_id();
    Ok(Spawned {
        master: pair.master,
        writer,
        reader,
        child,
        pid,
    })
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

/// The daemon's environment plus what identifies the terminal; `TERM` is Ghostty's own when
/// its terminfo could be written, `xterm-256color` otherwise.
fn environment(
    handle: &Handle,
    worktree: Option<&WorktreeRef>,
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
    if let Ok(dir) = colosso_term_engine::shell_integration_dir() {
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

    use std::sync::mpsc::{Receiver, channel};
    use std::time::{Duration, Instant};

    /// What `spawned` prints, a chunk at a time, read on a thread so a test can give up.
    fn printed(spawned: &mut Spawned) -> Receiver<Vec<u8>> {
        let mut reader = std::mem::replace(&mut spawned.reader, Box::new(io::empty()));
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n @ 1..) = reader.read(&mut chunk) {
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
            spawned
                .writer
                .write_all(b"echo \"zero=$0\"; exit\n")
                .unwrap();
        }
        let answered = ready && read_until(&printed, &mut seen, |text| text.contains("zero=-sh"));
        let _ = spawned.child.kill();
        let _ = spawned.child.wait();
        let seen = String::from_utf8_lossy(&seen);
        assert!(ready, "the shell showed no prompt: {seen:?}");
        assert!(answered, "not a login shell: {seen:?}");
    }
}
