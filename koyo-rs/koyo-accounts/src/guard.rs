//! The guard that keeps a sandboxed run off the person's real login (`docs/claude-accounts.md`
//! 7; cswap's `tests/conftest.py` real-store guard, #572).
//!
//! cswap's suite once overwrote the real account store: a thread outlived the fixture that
//! patched `$HOME`. Per-test patching can't stop that, so the guard is process-wide and keyed
//! on the real roots, frozen once from the process's own environment before any test changes
//! anything. Every write this crate makes goes through `fsutil`, which asks here first; for a
//! sandboxed [`Env`], a path outside the sandbox's folder, or under a real root, is refused
//! before the file system is touched. The system's `Env` has no sandbox and passes.

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use crate::{AccountsError, Env, Result};

/// The real roots: the person's Claude Code login and both account stores, as this process's
/// environment names them. Frozen at first use.
fn real_roots() -> &'static [PathBuf] {
    static ROOTS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        let mut roots = Vec::new();
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            for name in [
                ".claude",
                ".claude.json",
                ".claude.json.lock",
                ".claude.lock",
                ".claude-swap-backup",
            ] {
                roots.push(home.join(name));
            }
            roots.push(home.join("Library/Application Support/dev.koyo.desktop/claude-accounts"));
        }
        if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
            let dir = PathBuf::from(dir);
            roots.push(dir.join(".claude.json"));
            roots.push(dir);
        }
        roots.iter().flat_map(|root| spellings(root)).collect()
    })
}

/// `path` as written, made absolute without touching the disk, and with its existing part
/// resolved through symlinks: a link planted in a sandbox can't point a write at a real root.
fn spellings(path: &Path) -> Vec<PathBuf> {
    let lexical = lexical(path);
    let mut out = vec![lexical.clone()];
    // The deepest ancestor that exists, resolved, with the rest put back on.
    let mut existing = lexical.as_path();
    let mut rest = Vec::new();
    while let Some(parent) = existing.parent() {
        if let Ok(resolved) = existing.canonicalize() {
            let mut full = resolved;
            full.extend(rest.iter().rev());
            if full != lexical {
                out.push(full);
            }
            break;
        }
        if let Some(name) = existing.file_name() {
            rest.push(name.to_owned());
        }
        existing = parent;
    }
    out
}

/// `path` with `.` and `..` folded away, as an absolute path.
fn lexical(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Whether a sandboxed run may write `path`: under the sandbox's folder, and under no real
/// root, by every spelling. The system's `Env` may write anywhere.
///
/// # Errors
/// [`AccountsError::Guarded`] for a write the sandbox refuses.
pub fn check(env: &Env, path: &Path) -> Result<()> {
    let Some(sandbox) = env.sandbox() else {
        return Ok(());
    };
    let sandbox = spellings(sandbox);
    let targets = spellings(path);
    let inside = targets
        .iter()
        .all(|target| sandbox.iter().any(|root| target.starts_with(root)));
    let real = targets
        .iter()
        .any(|target| real_roots().iter().any(|root| target.starts_with(root)));
    if inside && !real {
        Ok(())
    } else {
        log::error!("guard: refused a write to {}", path.display());
        Err(AccountsError::Guarded(path.to_owned()))
    }
}

/// Whether `path` is under a real root, for tests of the guard itself.
#[cfg(any(test, feature = "testing"))]
pub fn is_real(path: &Path) -> bool {
    spellings(path)
        .iter()
        .any(|target| real_roots().iter().any(|root| target.starts_with(root)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Sandbox;

    #[test]
    fn a_sandbox_writes_only_inside_its_folder() {
        let sandbox = Sandbox::new();
        let env = sandbox.env();
        assert!(check(&env, &sandbox.root().join("store/sequence.json")).is_ok());
        let outside = sandbox.root().parent().unwrap().join("elsewhere.json");
        assert!(matches!(
            check(&env, &outside),
            Err(AccountsError::Guarded(_))
        ));
        // `..` can't climb out.
        let climbing = sandbox.root().join("store/../../elsewhere.json");
        assert!(check(&env, &climbing).is_err());
    }

    #[test]
    fn the_real_login_is_refused_by_every_spelling() {
        let sandbox = Sandbox::new();
        let env = sandbox.env();
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return;
        };
        for real in [
            home.join(".claude.json"),
            home.join(".claude/.credentials.json"),
            home.join(".claude/./x/../.credentials.json"),
            home.join(".claude-swap-backup/sequence.json"),
        ] {
            assert!(is_real(&real), "{}", real.display());
            assert!(check(&env, &real).is_err(), "{}", real.display());
        }
    }

    #[test]
    fn a_link_in_the_sandbox_cant_aim_a_write_out_of_it() {
        let sandbox = Sandbox::new();
        let env = sandbox.env();
        let outside = sandbox.root().parent().unwrap().to_owned();
        let link = sandbox.root().join("escape");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(check(&env, &link.join("file.json")).is_err());
    }
}
