//! Port of claude-swap's `credentials.py` (0fe22a8), its global-config helpers: Claude Code's
//! `~/.claude.json` (or the legacy `.config.json`, `paths::global_config_path`), read whole or
//! for its `oauthAccount` alone.
//!
//! Left out: `_update_global_config`, which nothing here calls: a switch writes the config
//! itself, under Claude Code's config lock (`switcher/switch/perform.rs`).

use std::fmt;

use serde::de::{Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde_json::{Map, Value};

use crate::credentials::{CredentialStore, generation};
use crate::paths;

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
    /// skipped, since the identity is read on every check, and while the file hasn't changed
    /// (its generation) the last answer is kept, the file read only to see that.
    pub fn read_oauth_account(&self) -> Option<Map<String, Value>> {
        let path = paths::global_config_path(&self.env);
        let changed = generation(&path)?;
        if let Some((read, oauth)) = &self.state().oauth_account
            && *read == changed
        {
            return oauth.clone();
        }
        let oauth = parse_oauth_account(&path);
        self.state().oauth_account = Some((changed, oauth.clone()));
        oauth
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

/// The `oauthAccount` object of the global config at `path`, the rest skipped.
fn parse_oauth_account(path: &std::path::Path) -> Option<Map<String, Value>> {
    let parsed = std::fs::read_to_string(path)
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
