//! Port of claude-swap's `credentials.py` (0fe22a8), its unclaimed-credential stash: live
//! credential bytes a switch attributed to someone other than the outgoing slot, kept before
//! the live store is overwritten (they may be the only copy of some account's refresh token),
//! and successors the refresh gate couldn't persist.
//!
//! Each entry is a base64 0600 file, `<store>/credentials/.unclaimed-<id>.enc`, append-only,
//! with a JSON manifest beside it (`.unclaimed-manifest.json`) carrying the evidence.
//! Deliberately files, outside the Keychain: a failed stash aborts the switch, and that abort
//! must not inherit the Keychain's failures (#101/#106: a flaky Keychain would block
//! switches). The entry is written before the manifest row: bytes without a row are
//! recoverable, a row without bytes isn't.
//!
//! The reads tell three states apart, since "nothing stashed" and "couldn't tell" have
//! opposite consequences: a stash row can be the only record of a generation already spent.

use std::io::ErrorKind;
use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::{GeneralPurpose, GeneralPurposeConfig};
use serde_json::{Map, Value, json};

use crate::credentials::CredentialStore;
use crate::locking::{self, FileLock};
use crate::oauth::{iso_utc, sha256_hex, utc_at};
use crate::random::random_u64;
use crate::{AccountsError, Result, fsutil, paths};

const MANIFEST: &str = ".unclaimed-manifest.json";
const MANIFEST_LOCK: &str = ".unclaimed-manifest.lock";
const ENTRY_PREFIX: &str = ".unclaimed-";
const ENTRY_SUFFIX: &str = ".enc";

/// Python's `b64decode(validate=True)`: the standard alphabet and its padding only, trailing
/// bits not checked.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
);

/// What a manifest read established.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestVerdict {
    /// Read, or genuinely absent: no rows and nothing to protect read the same to every caller.
    Ok,
    /// The bytes exist and couldn't be read (EIO, a mode): transient, so every caller defers.
    Unreadable,
    /// The bytes were read and aren't a manifest: permanent, so the mutator proceeds and sets
    /// the file aside, the only repair, and the adoption scan asks whether entry bytes are at
    /// risk ([`CredentialStore::stash_entry_files_exist`]).
    Corrupt,
}

impl ManifestVerdict {
    /// cswap's verdict string.
    pub fn as_str(self) -> &'static str {
        match self {
            ManifestVerdict::Ok => "ok",
            ManifestVerdict::Unreadable => "unreadable",
            ManifestVerdict::Corrupt => "corrupt",
        }
    }
}

/// `bytes.strip()`: Python's ASCII whitespace, vertical tab included.
fn strip(bytes: &[u8]) -> &[u8] {
    let space = |byte: &u8| b" \t\n\r\x0b\x0c".contains(byte);
    let start = bytes
        .iter()
        .position(|byte| !space(byte))
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !space(byte))
        .map_or(start, |at| at + 1);
    bytes.get(start..end).unwrap_or_default()
}

/// The UTF-8 text a stash file's base64 holds, or why it holds none.
fn decode(raw: &[u8]) -> Result<String, String> {
    let bytes = BASE64
        .decode(strip(raw))
        .map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

/// `secrets.token_hex(3)`: six random hex digits.
fn token_hex3() -> String {
    format!("{:06x}", random_u64() & 0x00ff_ffff)
}

impl CredentialStore {
    /// `<store>/credentials`, cswap's `credentials_dir`.
    pub fn credentials_dir(&self) -> PathBuf {
        paths::store::credentials_dir(&self.env)
    }

    pub fn stash_manifest_path(&self) -> PathBuf {
        self.credentials_dir().join(MANIFEST)
    }

    pub fn stash_entry_path(&self, entry_id: &str) -> PathBuf {
        self.credentials_dir()
            .join(format!("{ENTRY_PREFIX}{entry_id}{ENTRY_SUFFIX}"))
    }

    /// The manifest's rows with what the read established. The read decides, not an existence
    /// check: bytes, so undecodable bytes are corrupt, never an escape from the reader.
    pub fn read_stash_manifest_ex(&self) -> (Map<String, Value>, ManifestVerdict) {
        let raw = match std::fs::read(self.stash_manifest_path()) {
            Ok(raw) => raw,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return (Map::new(), ManifestVerdict::Ok);
            }
            Err(error) => {
                log::warn!("Unclaimed manifest unreadable: {error}");
                return (Map::new(), ManifestVerdict::Unreadable);
            }
        };
        let parsed = std::str::from_utf8(&raw)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                serde_json::from_str::<Value>(text).map_err(|error| error.to_string())
            });
        let manifest = match parsed {
            Ok(Value::Object(manifest)) => manifest,
            Ok(_) => {
                log::warn!("Failed to read unclaimed manifest: not a JSON object");
                return (Map::new(), ManifestVerdict::Corrupt);
            }
            Err(error) => {
                log::warn!("Failed to read unclaimed manifest: {error}");
                return (Map::new(), ManifestVerdict::Corrupt);
            }
        };
        if let Some(Value::Object(entries)) = manifest.get("entries") {
            return (entries.clone(), ManifestVerdict::Ok);
        }
        // Parseable but not a manifest: the rows are as unestablishable as under garbage, and
        // "ok, no rows" would let orphan entries past the fail-closed check.
        log::warn!("Unclaimed manifest parses but has no valid 'entries' member");
        (Map::new(), ManifestVerdict::Corrupt)
    }

    /// Whether any stashed credential's bytes are on disk, asked when the manifest is corrupt.
    /// A folder that can't be listed answers `true`: not knowing isn't evidence that nothing is
    /// at risk. A missing folder answers `false`: nothing was ever stashed.
    pub fn stash_entry_files_exist(&self) -> bool {
        let entries = match std::fs::read_dir(self.credentials_dir()) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return false,
            Err(_) => return true,
        };
        for entry in entries {
            let Ok(entry) = entry else {
                return true;
            };
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(ENTRY_PREFIX) && name.ends_with(ENTRY_SUFFIX) {
                return true;
            }
        }
        false
    }

    /// The manifest's rows, verdict dropped: for tests, as every caller here asks the verdict.
    #[cfg(test)]
    pub(crate) fn read_stash_manifest(&self) -> Map<String, Value> {
        self.read_stash_manifest_ex().0
    }

    /// Writes the manifest. A file there that doesn't parse is set aside as
    /// `….corrupt-<epoch>` first, never clobbered: its rows are evidence, and failing instead
    /// would block every switch, since a failed stash aborts one.
    ///
    /// # Errors
    /// The file system's, or the guard's.
    pub fn write_stash_manifest(&self, entries: &Map<String, Value>) -> Result<()> {
        fsutil::create_dir_all(&self.env, &self.credentials_dir())?;
        let path = self.stash_manifest_path();
        if path.exists() {
            let parses = std::fs::read_to_string(&path)
                .is_ok_and(|text| serde_json::from_str::<Value>(&text).is_ok());
            if !parses {
                // `int(time.time())`: whole seconds.
                let aside =
                    path.with_file_name(format!("{MANIFEST}.corrupt-{}", self.env.now() as i64));
                match fsutil::rename(&self.env, &path, &aside) {
                    Ok(()) => log::warn!(
                        "Unreadable unclaimed manifest preserved as {}",
                        aside.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    Err(error) => {
                        log::warn!("Could not preserve corrupt unclaimed manifest: {error}");
                    }
                }
            }
        }
        fsutil::atomic_write_json(
            &self.env,
            &path,
            &json!({"schemaVersion": 1, "entries": entries}),
        )
    }

    /// Applies `mutate` to the manifest as one step, under its own lock: the stash writer runs
    /// when the slot lock was unavailable, so it can't be that one, and unserialised the two
    /// writers each drop the other's row (50 of 80 lost, measured).
    ///
    /// # Errors
    /// [`AccountsError::Lock`] when the lock stays held; [`AccountsError::CredentialRead`] for
    /// an unreadable manifest, which a rewrite from an empty read would orphan (a corrupt one
    /// proceeds: the set-aside is its repair); the write's own failure. A failed stash must be
    /// loud: a successful one licenses overwriting the live store.
    pub fn mutate_stash_manifest(
        &self,
        mutate: impl FnOnce(&mut Map<String, Value>),
    ) -> Result<()> {
        let dir = self.credentials_dir();
        fsutil::create_dir_all(&self.env, &dir)?;
        let _held = FileLock::hold(
            &self.env,
            &dir.join(MANIFEST_LOCK),
            locking::DEFAULT_TIMEOUT,
        )?;
        let (mut entries, verdict) = self.read_stash_manifest_ex();
        if verdict == ManifestVerdict::Unreadable {
            return Err(AccountsError::CredentialRead(
                "the unclaimed manifest is unreadable; refusing to rewrite it from an empty \
                 read, which would orphan every stashed successor it maps"
                    .into(),
            ));
        }
        mutate(&mut entries);
        self.write_stash_manifest(&entries)
    }

    /// Stashes a credential of unknown provenance with `context` as its row's evidence: the
    /// entry id. The id carries a nonce, so identical bytes stashed in one second still get two.
    ///
    /// # Errors
    /// As [`Self::mutate_stash_manifest`], and the entry's own write.
    pub fn write_unclaimed_credential(
        &self,
        credentials: &str,
        context: Map<String, Value>,
    ) -> Result<String> {
        let now = self.env.now();
        // The sha256's first 12 hex digits.
        let digest = &sha256_hex(credentials)[..12];
        // `strftime`: whole seconds, as the `createdAt` beside it.
        let stamp = utc_at(now.floor()).unwrap_or_default();
        let entry_id = format!(
            "{}-{digest}-{}",
            stamp.format("%Y%m%dT%H%M%S"),
            token_hex3()
        );
        fsutil::create_dir_all(&self.env, &self.credentials_dir())?;
        fsutil::atomic_write(
            &self.env,
            &self.stash_entry_path(&entry_id),
            BASE64.encode(credentials).as_bytes(),
        )?;
        let mut row = Map::new();
        row.insert("createdAt".into(), iso_utc(now).unwrap_or_default().into());
        row.extend(context);
        self.mutate_stash_manifest(|entries| {
            entries.insert(entry_id.clone(), Value::Object(row));
        })?;
        Ok(entry_id)
    }

    /// The manifest's rows by id, with entry files that have no row listed too
    /// (`{"createdAt": null}`), so bytes never vanish from view. For the tests of the stash's
    /// writers: nothing else lists it.
    #[cfg(test)]
    pub(crate) fn list_unclaimed_credentials(&self) -> Map<String, Value> {
        let mut entries = self.read_stash_manifest();
        if let Ok(dir) = std::fs::read_dir(self.credentials_dir()) {
            for entry in dir.flatten() {
                let name = entry.file_name();
                let Some(id) = name
                    .to_str()
                    .and_then(|name| name.strip_prefix(ENTRY_PREFIX))
                    .and_then(|rest| rest.strip_suffix(ENTRY_SUFFIX))
                else {
                    continue;
                };
                entries
                    .entry(id.to_owned())
                    .or_insert_with(|| json!({"createdAt": null}));
            }
        }
        entries
    }

    /// One stashed credential: `(value, unreadable)`, as a backup read. `unreadable` when the
    /// file exists and couldn't be read: it may be the only copy of a spent generation's
    /// successor. Absent, or corrupt (its bytes are gone for good), both give `("", false)`.
    pub fn read_unclaimed_credential(&self, entry_id: &str) -> (String, bool) {
        let raw = match std::fs::read(self.stash_entry_path(entry_id)) {
            Ok(raw) => raw,
            Err(error) if error.kind() == ErrorKind::NotFound => return (String::new(), false),
            Err(error) => {
                log::warn!("Unclaimed credential {entry_id} unreadable: {error}");
                return (String::new(), true);
            }
        };
        match decode(&raw) {
            Ok(value) => (value, false),
            Err(error) => {
                log::warn!("Failed to decode unclaimed credential {entry_id}: {error}");
                (String::new(), false)
            }
        }
    }

    /// Deletes a stash entry, its bytes and its row, after it was adopted.
    ///
    /// # Errors
    /// As [`Self::mutate_stash_manifest`]; the bytes' removal only logs.
    pub fn remove_unclaimed_credential(&self, entry_id: &str) -> Result<()> {
        match fsutil::remove_file(&self.env, &self.stash_entry_path(entry_id)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => log::warn!("Failed to remove unclaimed credential {entry_id}: {error}"),
        }
        self.mutate_stash_manifest(|entries| {
            entries.remove(entry_id);
        })
    }
}
