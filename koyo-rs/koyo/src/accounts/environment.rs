//! Claude Code's variables as the Claude Codes on the Mac see them: the login shell's, read once,
//! as the agent host reads its environment (`koyo-agent-host/src/providers/claude/environment.ts`),
//! since an app started from the Dock has none of the shell's. `CLAUDE_CONFIG_DIR` and
//! `CLAUDE_SECURESTORAGE_CONFIG_DIR` move the login (`docs/claude-accounts.md` 2), and a few
//! others make Claude Code ignore the login altogether (study pitfall 42): those are named to the
//! person, never stripped. No value is logged. It's read once a process: the person's rc files run
//! once.

use std::io::Read as _;
use std::os::unix::process::CommandExt as _;
use std::process::{Command, Stdio};
use std::sync::{OnceLock, mpsc};
use std::time::Duration;

use koyo_accounts::Vars;

/// How long the login shell may take to print its environment, as the host allows.
const LOGIN: Duration = Duration::from_secs(5);
/// What makes Claude Code use something other than the Mac's login, in the order said.
const OVERRIDES: [&str; 4] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
];

/// What the login shell says.
#[derive(Clone)]
pub struct Shell {
    pub vars: Vars,
    /// The first variable set that makes Claude Code ignore the accounts.
    pub overridden_by: Option<&'static str>,
}

/// The variables `env -0` printed: each `NAME=value`, ended by a NUL.
fn parse(printed: &[u8]) -> Vec<(String, String)> {
    printed
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry).ok()?;
            let (name, value) = entry.split_once('=')?;
            (!name.is_empty()).then(|| (name.to_owned(), value.to_owned()))
        })
        .collect()
}

/// `$SHELL -l -c 'env -0'`'s output, within [`LOGIN`].
fn printed() -> Option<Vec<u8>> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| "/bin/zsh".to_owned());
    let mut child = Command::new(shell)
        .args(["-l", "-c", "env -0"])
        // A group of its own, so a timeout stops what its rc files started too.
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .inspect_err(|error| log::warn!("couldn't start the login shell: {error}"))
        .ok()?;
    let mut out = child.stdout.take()?;
    let pid = child.id();
    let (sent, got) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        out.read_to_end(&mut bytes).ok();
        child.wait().ok();
        sent.send(bytes).ok();
    });
    if let Ok(bytes) = got.recv_timeout(LOGIN) {
        return Some(bytes);
    }
    // SAFETY: signalling the process group of the shell this function started.
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
    log::warn!("the login shell took too long to print its environment");
    None
}

fn from(vars: &[(String, String)]) -> Shell {
    let get = |name: &str| {
        vars.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    Shell {
        vars: Vars {
            claude_config_dir: get("CLAUDE_CONFIG_DIR"),
            securestorage_config_dir: get("CLAUDE_SECURESTORAGE_CONFIG_DIR"),
            user: get("USER"),
        },
        overridden_by: OVERRIDES
            .into_iter()
            .find(|name| get(name).is_some_and(|value| !value.is_empty())),
    }
}

/// The login shell's, or failing that the app's own; read the first time it's asked for.
pub fn read() -> Shell {
    static READ: OnceLock<Shell> = OnceLock::new();
    READ.get_or_init(|| {
        let vars = printed()
            .map(|bytes| parse(&bytes))
            .filter(|vars| vars.iter().any(|(name, _)| name == "PATH"))
            .unwrap_or_else(|| {
                log::warn!("the login shell's environment couldn't be read; using the app's");
                std::env::vars().collect()
            });
        from(&vars)
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_defined_empty_secure_store_is_kept_apart_from_an_unset_one() {
        let shell = from(&parse(
            b"PATH=/bin\0CLAUDE_SECURESTORAGE_CONFIG_DIR=\0USER=ana\0ANTHROPIC_API_KEY=\0",
        ));
        assert_eq!(shell.vars.securestorage_config_dir.as_deref(), Some(""));
        assert_eq!(shell.vars.claude_config_dir, None);
        assert_eq!(shell.vars.user.as_deref(), Some("ana"));
        // Set but empty, it overrides nothing.
        assert_eq!(shell.overridden_by, None);
    }

    #[test]
    fn a_variable_that_overrides_the_login_is_named() {
        let shell = from(&parse(b"PATH=/bin\0ANTHROPIC_BASE_URL=http://proxy\0"));
        assert_eq!(shell.overridden_by, Some("ANTHROPIC_BASE_URL"));
    }
}
