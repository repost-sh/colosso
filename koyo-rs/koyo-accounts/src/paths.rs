//! Port of claude-swap's `paths.py` (0fe22a8), with the Keychain service names Claude Code
//! derives (`session.py`'s `keychain_service_name`): Koyo reads and writes the files Claude
//! Code does, found as Claude Code finds them.
//!
//! - Config home: `CLAUDE_CONFIG_DIR` if set, else `~/.claude`.
//! - Global config: `<config home>/.config.json` if it's a regular file that is non-empty, or
//!   empty and writable with no `.claude.json` (Claude Code 2.1.299's rule); otherwise
//!   `(CLAUDE_CONFIG_DIR || $HOME)/.claude.json`. `.claude.json` sits in the home, not in
//!   `.claude/`.
//! - Credentials file: `<config home>/.credentials.json`.
//!
//! The backup root is Koyo's store (`Env::store`) instead of cswap's XDG and legacy roots, and
//! the legacy-root migration isn't ported (`docs/claude-accounts.md` 3).

use std::fmt::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};
use unicode_normalization::UnicodeNormalization as _;

use crate::Env;

/// Claude Code's active OAuth credential in the Keychain.
pub const CLAUDE_CODE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
/// Koyo's backups, in place of cswap's `claude-swap` (`docs/claude-accounts.md` 2).
pub const BACKUP_KEYCHAIN_SERVICE: &str = "Koyo Claude accounts";

/// The config home: `CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn claude_config_home(env: &Env) -> PathBuf {
    match env.vars.config_dir() {
        Some(dir) => PathBuf::from(dir),
        None => default_claude_config_home(env),
    }
}

/// Claude Code 2.1.299's choice between `.config.json` and `.claude.json`.
fn pick_global_config(legacy: &Path, global: PathBuf) -> PathBuf {
    // Follows symlinks; an error makes the global file win.
    let Ok(meta) = legacy.metadata() else {
        return global;
    };
    let writable = meta.permissions().mode() & 0o222 != 0;
    if meta.is_file() && (meta.len() > 0 || (writable && !global.exists())) {
        legacy.to_owned()
    } else {
        global
    }
}

/// The global config: the legacy `<config home>/.config.json` when Claude Code would use it,
/// else `(CLAUDE_CONFIG_DIR || $HOME)/.claude.json`.
pub fn global_config_path(env: &Env) -> PathBuf {
    let base = env
        .vars
        .config_dir()
        .map_or_else(|| env.home.clone(), PathBuf::from);
    pick_global_config(
        &claude_config_home(env).join(".config.json"),
        base.join(".claude.json"),
    )
}

/// The default profile's config home, whatever `CLAUDE_CONFIG_DIR` says.
pub fn default_claude_config_home(env: &Env) -> PathBuf {
    env.home.join(".claude")
}

/// The default profile's global config, with the same legacy rule, whatever
/// `CLAUDE_CONFIG_DIR` says.
pub fn default_global_config_path(env: &Env) -> PathBuf {
    pick_global_config(
        &default_claude_config_home(env).join(".config.json"),
        env.home.join(".claude.json"),
    )
}

/// The credentials file: `<config home>/.credentials.json`.
pub fn credentials_path(env: &Env) -> PathBuf {
    claude_config_home(env).join(".credentials.json")
}

/// The Keychain service Claude Code derives for a config folder: sha256 of the folder's name
/// as written, NFC-normalised and unresolved, its first 8 hex. A trailing slash or a `~`
/// spelling is another item.
pub fn keychain_service_name(config_dir: &str) -> String {
    let normalized: String = config_dir.nfc().collect();
    let digest = Sha256::digest(normalized.as_bytes());
    let hex = digest[..4].iter().fold(String::new(), |mut hex, byte| {
        write!(hex, "{byte:02x}").ok();
        hex
    });
    format!("{CLAUDE_CODE_KEYCHAIN_SERVICE}-{hex}")
}

/// Whether two paths name the same folder, symlinks and `..` included, as written when either
/// can't be resolved (cswap's `_same_directory`).
pub fn same_directory(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// Koyo's store: the folders and files cswap keeps under its backup root.
pub mod store {
    use std::path::PathBuf;

    use crate::Env;

    pub fn sequence_file(env: &Env) -> PathBuf {
        env.store.join("sequence.json")
    }

    pub fn configs_dir(env: &Env) -> PathBuf {
        env.store.join("configs")
    }

    /// cswap's `credentials/`: the unclaimed-credential stash (backups are in the Keychain).
    pub fn credentials_dir(env: &Env) -> PathBuf {
        env.store.join("credentials")
    }

    /// Koyo's own lock, taken before Claude Code's.
    pub fn lock_file(env: &Env) -> PathBuf {
        env.store.join(".lock")
    }

    pub fn cache_dir(env: &Env) -> PathBuf {
        env.store.join("cache")
    }

    pub fn autoswitch_state(env: &Env) -> PathBuf {
        env.store.join("autoswitch_state.json")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::testing::Sandbox;

    #[test]
    fn default_is_dot_claude_in_home() {
        let sandbox = Sandbox::new();
        assert_eq!(
            claude_config_home(&sandbox.env()),
            sandbox.home().join(".claude")
        );
    }

    #[test]
    fn respects_env_var() {
        let mut sandbox = Sandbox::new();
        let custom = sandbox.root().join("custom-claude");
        sandbox.vars.claude_config_dir = Some(custom.to_str().unwrap().to_owned());
        assert_eq!(claude_config_home(&sandbox.env()), custom);
        // Empty is unset.
        sandbox.vars.claude_config_dir = Some(String::new());
        assert_eq!(
            claude_config_home(&sandbox.env()),
            sandbox.home().join(".claude")
        );
    }

    #[test]
    fn default_returns_homedir_claude_json() {
        let sandbox = Sandbox::new();
        assert_eq!(
            global_config_path(&sandbox.env()),
            sandbox.home().join(".claude.json")
        );
    }

    #[test]
    fn ccd_set_returns_ccd_claude_json() {
        let mut sandbox = Sandbox::new();
        let custom = sandbox.root().join("ccd");
        fs::create_dir_all(&custom).unwrap();
        sandbox.vars.claude_config_dir = Some(custom.to_str().unwrap().to_owned());
        assert_eq!(
            global_config_path(&sandbox.env()),
            custom.join(".claude.json")
        );
    }

    #[test]
    fn legacy_config_json_takes_precedence() {
        let sandbox = Sandbox::new();
        let legacy = sandbox.home().join(".claude/.config.json");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, "{}").unwrap();
        assert_eq!(global_config_path(&sandbox.env()), legacy);
    }

    #[test]
    fn legacy_config_json_in_ccd_takes_precedence() {
        let mut sandbox = Sandbox::new();
        let custom = sandbox.root().join("ccd");
        fs::create_dir_all(&custom).unwrap();
        sandbox.vars.claude_config_dir = Some(custom.to_str().unwrap().to_owned());
        let legacy = custom.join(".config.json");
        fs::write(&legacy, "{}").unwrap();
        assert_eq!(global_config_path(&sandbox.env()), legacy);
    }

    #[test]
    fn legacy_config_rule_matches_claude_code_2_1_299() {
        let cases = [
            ("absent", false, false),
            ("empty", true, false),
            ("empty", false, true),
            ("empty_readonly", false, false),
            ("nonempty", true, true),
            ("directory", false, false),
        ];
        let resolvers: [fn(&Env) -> PathBuf; 2] = [global_config_path, default_global_config_path];
        for resolver in resolvers {
            for (kind, global_present, expect_legacy) in cases {
                let sandbox = Sandbox::new();
                let legacy = sandbox.home().join(".claude/.config.json");
                fs::create_dir_all(legacy.parent().unwrap()).unwrap();
                match kind {
                    "directory" => fs::create_dir(&legacy).unwrap(),
                    "absent" => {}
                    _ => {
                        fs::write(&legacy, if kind == "nonempty" { "{}" } else { "" }).unwrap();
                        if kind == "empty_readonly" {
                            fs::set_permissions(&legacy, fs::Permissions::from_mode(0o444))
                                .unwrap();
                        }
                    }
                }
                let global = sandbox.home().join(".claude.json");
                if global_present {
                    fs::write(&global, "{}").unwrap();
                }
                let expected = if expect_legacy { legacy } else { global };
                assert_eq!(
                    resolver(&sandbox.env()),
                    expected,
                    "{kind} {global_present}"
                );
            }
        }
    }

    #[test]
    fn credentials_path_default_inside_dot_claude_and_respects_ccd() {
        let mut sandbox = Sandbox::new();
        assert_eq!(
            credentials_path(&sandbox.env()),
            sandbox.home().join(".claude/.credentials.json")
        );
        let custom = sandbox.root().join("ccd");
        sandbox.vars.claude_config_dir = Some(custom.to_str().unwrap().to_owned());
        assert_eq!(
            credentials_path(&sandbox.env()),
            custom.join(".credentials.json")
        );
    }

    #[test]
    fn a_hashed_service_hashes_the_string_as_written() {
        // `/Users/schulzf/.claude` hashes to the `-7bbb8320` item the study found on the Mac.
        assert_eq!(
            keychain_service_name("/Users/schulzf/.claude"),
            "Claude Code-credentials-7bbb8320"
        );
        assert_ne!(
            keychain_service_name("/Users/schulzf/.claude/"),
            keychain_service_name("/Users/schulzf/.claude")
        );
        // NFC: a decomposed é names the same item as a composed one.
        assert_eq!(
            keychain_service_name("/a/e\u{301}"),
            keychain_service_name("/a/\u{e9}")
        );
    }
}
