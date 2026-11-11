//! Port of claude-swap's `credentials.py` (0fe22a8), its global-config helpers: Claude Code's
//! `~/.claude.json` (or the legacy `.config.json`, `paths::global_config_path`), read whole or
//! for its `oauthAccount` alone, and changed key by key under Claude Code's config lock.
//!
//! cswap's `_update_global_config` takes no lock of its own: its callers hold
//! `claude_config_lock` around it (and around the whole switch), and the lock is a directory,
//! so taking it again inside would wait on itself. Here the held lock is a parameter, so the
//! read-modify-write can't run without it.
//!
//! The rewrite is `serde_json`'s two-space pretty form, as cswap's `json.dumps(indent=2)`, with
//! the keys where the file had them (`serde_json`'s `preserve_order`), as Python's dicts keep
//! them: the person's whole Claude Code state is in this file.

use std::fmt;

use serde::de::{Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde_json::{Map, Value};

use crate::claude_locks::LockDir;
use crate::credentials::CredentialStore;
use crate::{AccountsError, Result, fsutil, paths};

impl CredentialStore {
    /// The global config as a JSON object, or `None` when it's absent, unreadable or not an
    /// object.
    pub fn read_global_config(&self) -> Option<Map<String, Value>> {
        let path = paths::global_config_path(&self.env);
        if !path.exists() {
            return None;
        }
        let parsed = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                serde_json::from_str::<Value>(&text).map_err(|error| error.to_string())
            });
        match parsed {
            Ok(Value::Object(config)) => Some(config),
            Ok(_) => None,
            Err(error) => {
                log::warn!("Failed to read global config: {error}");
                None
            }
        }
    }

    /// The global config's `oauthAccount` object alone, or `None` when the config is absent,
    /// unreadable or not an object, or carries no `oauthAccount` object. Only that key is
    /// parsed into values; the rest of the file (the person's whole Claude Code state) is
    /// skipped, since the identity is read on every check.
    pub fn read_oauth_account(&self) -> Option<Map<String, Value>> {
        let path = paths::global_config_path(&self.env);
        if !path.exists() {
            return None;
        }
        let parsed = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                serde_json::from_str::<OauthAccountOnly>(&text).map_err(|error| error.to_string())
            });
        match parsed {
            Ok(OauthAccountOnly(Some(Value::Object(oauth)))) => Some(oauth),
            Ok(_) => None,
            Err(error) => {
                log::warn!("Failed to read global config: {error}");
                None
            }
        }
    }

    /// Applies `mutate` to the global config and writes it back atomically, 0600: `mutate`
    /// changes only the keys it owns, and every other key (`oauthAccount`, `projects`,
    /// settings) is kept. Run with Claude Code's config lock held (`claude_config_lock`).
    ///
    /// # Errors
    /// [`AccountsError::CredentialWrite`] when the file exists but couldn't be read: a torn
    /// config read as empty and written back loses the person's whole Claude Code state. An
    /// absent config is a genuine start. The write's own failure.
    pub fn update_global_config(
        &self,
        _config_lock: &LockDir,
        mutate: impl FnOnce(&mut Map<String, Value>),
    ) -> Result<()> {
        let path = paths::global_config_path(&self.env);
        let data = self.read_global_config();
        if data.is_none() && path.exists() {
            return Err(AccountsError::CredentialWrite(format!(
                "{} exists but could not be read — refusing to overwrite it. Move or repair the \
                 file, then retry.",
                path.display()
            )));
        }
        let mut data = data.unwrap_or_default();
        mutate(&mut data);
        if let Some(parent) = path.parent() {
            fsutil::create_dir_all(&self.env, parent)?;
        }
        let text = serde_json::to_string_pretty(&Value::Object(data))
            .map_err(|error| AccountsError::CredentialWrite(error.to_string()))?;
        fsutil::atomic_write(&self.env, &path, text.as_bytes())
    }
}

/// A JSON object's `oauthAccount` value, every other value skipped unparsed. A repeated key
/// keeps its last value, as `json.loads` does; anything but an object is refused.
struct OauthAccountOnly(Option<Value>);

impl<'de> Deserialize<'de> for OauthAccountOnly {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Fields;

        impl<'de> Visitor<'de> for Fields {
            type Value = OauthAccountOnly;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<OauthAccountOnly, A::Error> {
                let mut oauth_account = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key == "oauthAccount" {
                        oauth_account = Some(map.next_value::<Value>()?);
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(OauthAccountOnly(oauth_account))
            }
        }

        deserializer.deserialize_map(Fields)
    }
}
