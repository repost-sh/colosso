//! Port of claude-swap's `switcher.py` (0fe22a8), its store half: `sequence.json` and the
//! per-slot config backups under `configs/`, with cswap's file layout and JSON keys
//! (`activeAccountNumber`, `lastUpdated`, `sequence`, `accounts`), and the JSON reader and
//! writer every file of the switch goes through.
//!
//! The reader tells ABSENT from UNREADABLE: `None` is only a file that isn't there; a strict
//! read refuses one that is there but torn, unreadable or not an object. `sequence.json` is
//! always read strictly: cswap measured a torn roster read as "no accounts", after which
//! `add_account` reassigned slot 1 and overwrote its live credential backup. The writer
//! publishes by temp file and rename, 0600 (`fsutil::atomic_write`), so nothing can fail after
//! the file is live. `serde_json` writes non-ASCII as UTF-8 where cswap's `json.dumps` escapes
//! it; both read back the same. It can't hold a NaN, which cswap's `allow_nan=False` refuses.
//!
//! Left out: the session-profile check and removal in `_delete_account_files`
//! (`_ensure_no_live_session`, `_delete_session_profile`), with session mode.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::switcher::Switcher;
use crate::{AccountsError, Env, Result, fsutil};

/// A `sequence.json` (or any store file) as cswap passes it around: a JSON object.
pub(crate) type Data = Map<String, Value>;

/// cswap's `get_timestamp`: now, in UTC, `2024-01-01T00:00:00Z`.
pub(crate) fn get_timestamp(env: &Env) -> String {
    crate::oauth::iso_utc(env.now()).unwrap_or_default()
}

/// The accounts object of loaded data, or `None` when it's missing or not an object.
pub(crate) fn accounts(data: &Data) -> Option<&Map<String, Value>> {
    data.get("accounts").and_then(Value::as_object)
}

/// One slot's record of loaded data.
pub(crate) fn record<'a>(data: &'a Data, account_num: &str) -> Option<&'a Map<String, Value>> {
    accounts(data)?.get(account_num)?.as_object()
}

/// One slot's record of loaded data, to change it.
pub(crate) fn record_mut<'a>(
    data: &'a mut Data,
    account_num: &str,
) -> Option<&'a mut Map<String, Value>> {
    data.get_mut("accounts")?
        .as_object_mut()?
        .get_mut(account_num)?
        .as_object_mut()
}

/// A string field of a record, `""` when missing or not a string, as `record.get(key, "") or ""`.
pub(crate) fn text<'a>(record: Option<&'a Map<String, Value>>, key: &str) -> &'a str {
    record
        .and_then(|record| record.get(key))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// The slot numbers of `sequence`, as cswap's `str(num)` of each.
pub(crate) fn sequence(data: &Data) -> Vec<String> {
    data.get("sequence")
        .and_then(Value::as_array)
        .map(|nums| {
            nums.iter()
                .map(|num| match num {
                    Value::String(num) => num.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `str(data["activeAccountNumber"]) == account_num`.
pub(crate) fn is_active(data: &Data, account_num: &str) -> bool {
    match data.get("activeAccountNumber") {
        Some(Value::String(active)) => active == account_num,
        Some(Value::Number(active)) => {
            active.as_u64().map(|active| active.to_string()).as_deref() == Some(account_num)
        }
        _ => false,
    }
}

/// A slot number as the int cswap stores (`int(account_num)`).
pub(crate) fn slot_int(account_num: &str) -> Result<u64> {
    account_num.parse().map_err(|error| {
        AccountsError::Config(format!("invalid account number {account_num:?}: {error}"))
    })
}

/// `data["activeAccountNumber"] = int(account_num)`.
pub(crate) fn set_active(data: &mut Data, account_num: &str) -> Result<()> {
    data.insert(
        "activeAccountNumber".into(),
        Value::from(slot_int(account_num)?),
    );
    Ok(())
}

/// The global config's text, as the add and the switch read it for a backup or a snapshot.
pub(crate) fn read_config_text(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            AccountsError::Config("Claude config file not found".into())
        }
        std::io::ErrorKind::PermissionDenied => {
            AccountsError::Config("Permission denied reading Claude config".into())
        }
        _ => AccountsError::io(format!("reading {}", path.display()), error),
    })
}

/// The bytes of `path` as a JSON object, or why not (logged): what `_read_json` refuses
/// when it is strict, for bytes already read.
pub(crate) fn parse_object(path: &Path, bytes: &[u8]) -> std::result::Result<Data, String> {
    let parsed = std::str::from_utf8(bytes)
        .map_err(|error| error.to_string())
        .and_then(|text| serde_json::from_str::<Value>(text).map_err(|e| e.to_string()));
    match parsed {
        Ok(Value::Object(data)) => Ok(data),
        Ok(other) => {
            let kind = python_type_name(&other);
            log::warn!("{} holds {kind}, not a JSON object", path.display());
            Err(format!(
                "{} holds {kind}, not a JSON object. Repair or move it, then retry.",
                path.display()
            ))
        }
        Err(error) => {
            log::warn!("Invalid JSON in {}", path.display());
            Err(format!(
                "{} exists but could not be parsed ({error}). Repair or move it, then retry \
                 — refusing to overwrite it unread.",
                path.display()
            ))
        }
    }
}

/// The test seam for `write_json`: a write a test fails, as cswap's tests patch `_write_json`.
#[cfg(test)]
pub(crate) type WriteFault = Box<dyn FnMut(&Path, &Value) -> Option<std::io::Error>>;

#[cfg(test)]
thread_local! {
    pub(crate) static WRITE_FAULT: std::cell::RefCell<Option<WriteFault>> =
        const { std::cell::RefCell::new(None) };
}

impl Switcher {
    /// `_get_claude_config_path`: Claude Code's global config (`paths::global_config_path`).
    pub(crate) fn claude_config_path(&self) -> PathBuf {
        crate::paths::global_config_path(&self.env)
    }

    /// `_setup_directories`: the store, `configs/` and `credentials/`, each 0700.
    ///
    /// # Errors
    /// The guard's refusal, or the file system's.
    pub(crate) fn setup_directories(&self) -> Result<()> {
        for directory in [&self.env.store, &self.configs_dir, &self.credentials_dir] {
            fsutil::create_dir_all(&self.env, directory)?;
            fsutil::set_mode(&self.env, directory, 0o700).map_err(|error| {
                AccountsError::io(format!("narrowing {}", directory.display()), error)
            })?;
        }
        Ok(())
    }

    /// `_read_json`: the file as a JSON object, `None` when it is ABSENT.
    ///
    /// # Errors
    /// With `strict`, [`AccountsError::Config`] when the file is there but couldn't be read or
    /// parsed, or holds something other than an object; without, those read as `None` too
    /// (logged): a reader that is about to replace a malformed file on purpose stays a reader.
    pub(crate) fn read_json(&self, path: &Path, strict: bool) -> Result<Option<Data>> {
        if !path.exists() {
            return Ok(None);
        }
        let parsed = match std::fs::read(path) {
            Ok(bytes) => parse_object(path, &bytes),
            Err(error) => {
                log::warn!("Could not read {}: {error}", path.display());
                Err(format!(
                    "{} exists but could not be read ({error}). Fix what is blocking the read, \
                     then retry.",
                    path.display()
                ))
            }
        };
        match parsed {
            Ok(data) => Ok(Some(data)),
            Err(why) if strict => Err(AccountsError::Config(why)),
            Err(_) => Ok(None),
        }
    }

    /// `_write_json`: `data` as two-space JSON at `path`, 0600, by temp file and rename.
    ///
    /// # Errors
    /// The guard's refusal, or the file system's.
    pub(crate) fn write_json(&self, path: &Path, data: &(impl Serialize + ?Sized)) -> Result<()> {
        #[cfg(test)]
        if let Some(error) = WRITE_FAULT.with_borrow_mut(|fault| {
            let fault = fault.as_mut()?;
            fault(path, &serde_json::to_value(data).unwrap_or_default())
        }) {
            return Err(AccountsError::io(
                format!("writing {}", path.display()),
                error,
            ));
        }
        let text = serde_json::to_string_pretty(data)
            .map_err(|error| AccountsError::Config(format!("Generated invalid JSON: {error}")))?;
        fsutil::atomic_write(&self.env, path, text.as_bytes())
    }

    /// The roster written back, `lastUpdated` stamped first, as every change to it is.
    ///
    /// # Errors
    /// As [`Switcher::write_json`].
    pub(crate) fn save_sequence(&self, data: &mut Data) -> Result<()> {
        data.insert("lastUpdated".into(), get_timestamp(&self.env).into());
        self.write_json(&self.sequence_file, data)
    }

    /// `_salvage_unreadable`: a copy of an unreadable file kept beside it before it's replaced,
    /// the copy returned and the warning added to `warnings_out`. The copy is written 0600 from
    /// creation (it can hold `primaryApiKey`), and a counter keeps two copies in one second
    /// apart: the retry after a failed switch is exactly when the first copy matters.
    ///
    /// # Errors
    /// [`AccountsError::Switch`] when the copy can't be made: the switch aborts rather than
    /// destroy the bytes.
    pub(crate) fn salvage_unreadable(
        &self,
        path: &Path,
        warnings_out: &mut Vec<String>,
    ) -> Result<PathBuf> {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = format!("{name}.unreadable-{}", self.env.now().floor());
        let mut salvage = path.with_file_name(&stem);
        let mut n = 1;
        while salvage.exists() {
            salvage = path.with_file_name(format!("{stem}.{n}"));
            n += 1;
        }
        let copied = std::fs::read(path)
            .map_err(|error| AccountsError::io(format!("reading {}", path.display()), error))
            .and_then(|bytes| fsutil::atomic_write(&self.env, &salvage, &bytes));
        if let Err(error) = copied {
            return Err(AccountsError::Switch(format!(
                "{} could not be parsed and the salvage copy failed ({error}); aborting rather \
                 than destroying it",
                path.display()
            )));
        }
        let salvage_name = salvage
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        log::warn!(
            "{} could not be parsed; a copy was kept at {} before it was replaced",
            path.display(),
            salvage.display()
        );
        warnings_out.push(format!(
            "{name} could not be parsed — a copy was kept at {salvage_name}"
        ));
        Ok(salvage)
    }

    /// `_init_sequence_file`: an empty roster when there's none.
    ///
    /// # Errors
    /// As [`Switcher::write_json`].
    pub(crate) fn init_sequence_file(&self) -> Result<()> {
        if self.sequence_file.exists() {
            return Ok(());
        }
        let data = json!({
            "activeAccountNumber": null,
            "lastUpdated": get_timestamp(&self.env),
            "sequence": [],
            "accounts": {},
        });
        self.write_json(&self.sequence_file, &data)
    }

    /// `_get_sequence_data`: the roster, `None` ONLY when it doesn't exist yet.
    ///
    /// # Errors
    /// [`AccountsError::Config`] for a roster that is there but unreadable: read as "no
    /// accounts", the next write would rebuild it from nothing.
    pub(crate) fn get_sequence_data(&self) -> Result<Option<Data>> {
        self.read_json(&self.sequence_file, true)
    }

    /// The roster, or an empty one when it doesn't exist (cswap's `… or {}`).
    ///
    /// # Errors
    /// As [`Switcher::get_sequence_data`].
    pub(crate) fn sequence_data_or_empty(&self) -> Result<Data> {
        Ok(self.get_sequence_data()?.unwrap_or_default())
    }

    /// `_get_next_account_number`: one past the highest slot number, 1 for an empty roster.
    ///
    /// # Errors
    /// As [`Switcher::get_sequence_data`]; a slot key that isn't a number.
    pub(crate) fn get_next_account_number(&self) -> Result<u64> {
        let data = self.sequence_data_or_empty()?;
        let mut highest = 0;
        for num in accounts(&data).into_iter().flat_map(Map::keys) {
            highest = highest.max(slot_int(num)?);
        }
        Ok(highest + 1)
    }

    fn config_backup_path(&self, account_num: &str, email: &str) -> PathBuf {
        self.configs_dir
            .join(format!(".claude-config-{account_num}-{email}.json"))
    }

    /// `_read_account_config`: a slot's config backup, `""` when there's none.
    ///
    /// # Errors
    /// [`AccountsError::Io`] for a backup that is there but couldn't be read.
    pub(crate) fn read_account_config(&self, account_num: &str, email: &str) -> Result<String> {
        let path = self.config_backup_path(account_num, email);
        if !path.exists() {
            return Ok(String::new());
        }
        std::fs::read_to_string(&path)
            .map_err(|error| AccountsError::io(format!("reading {}", path.display()), error))
    }

    /// `_write_account_config`: a slot's config backup, atomically (a torn backup is a broken
    /// future restore).
    ///
    /// # Errors
    /// The guard's refusal, or the file system's.
    pub(crate) fn write_account_config(
        &self,
        account_num: &str,
        email: &str,
        config: &str,
    ) -> Result<()> {
        fsutil::atomic_write(
            &self.env,
            &self.config_backup_path(account_num, email),
            config.as_bytes(),
        )
    }

    /// `_delete_account_files`: a slot's credential backups (best-effort) and its config
    /// backup, the one chokepoint of everything that removes a slot.
    ///
    /// # Errors
    /// [`AccountsError::Io`] when the config backup is there and can't be removed.
    pub(crate) fn delete_account_files(&self, account_num: &str, email: &str) -> Result<()> {
        self.store.delete_account_credentials(account_num, email);
        let config_file = self.config_backup_path(account_num, email);
        if config_file.exists() {
            fsutil::remove_file(&self.env, &config_file).map_err(|error| {
                AccountsError::io(format!("removing {}", config_file.display()), error)
            })?;
        }
        Ok(())
    }

    /// `_account_is_switchable`: the slot of loaded `data` has a stored credential and a config
    /// backup. An eligibility read, served from the display cache; every activation re-reads
    /// fresh.
    ///
    /// # Errors
    /// As [`Switcher::read_account_config`].
    pub(crate) fn account_is_switchable(&self, data: &Data, account_num: &str) -> Result<bool> {
        let Some(record) = record(data, account_num).filter(|record| !record.is_empty()) else {
            return Ok(false);
        };
        let email = text(Some(record), "email");
        if self
            .store
            .read_account_credentials(account_num, email, true)
            .is_empty()
        {
            return Ok(false);
        }
        Ok(!self.read_account_config(account_num, email)?.is_empty())
    }
}

/// Python's `type(data).__name__` for a JSON value, for the "holds …" message.
fn python_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}
