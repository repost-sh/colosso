//! The unclaimed stash: `test_switcher.py`'s `TestStashAndRetentionStore` stash cases,
//! `TestStashStorageHardening`, `TestStashManifestConcurrentMutation`, and the store halves of
//! `TestConsumeGate`'s and `TestStashReaderUnreadableVsAbsent`'s manifest and entry cases.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use serde_json::{Map, Value, json};

use super::World;
use crate::AccountsError;
use crate::credentials::{CredentialStore, ManifestVerdict};

fn context(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// An entry's bytes, decoded straight from its file: the store is write-only by design.
fn read_safety_copy(store: &CredentialStore, entry_id: &str) -> String {
    let raw = fs::read_to_string(store.stash_entry_path(entry_id)).unwrap();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .unwrap();
    String::from_utf8(bytes).unwrap()
}

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn safety_copy_write_and_list() {
    let world = World::new();
    let store = &world.store;
    let entry_id = store
        .write_unclaimed_credential("secret-bytes", context(json!({"reason": "alien"})))
        .unwrap();
    assert_eq!(read_safety_copy(store, &entry_id), "secret-bytes");
    let entries = store.list_unclaimed_credentials();
    assert_eq!(entries[&entry_id]["reason"], "alien");
    assert!(
        entries[&entry_id]["createdAt"]
            .as_str()
            .is_some_and(|at| !at.is_empty())
    );
}

#[test]
fn safety_copy_file_is_owner_only() {
    let world = World::new();
    let entry_id = world
        .store
        .write_unclaimed_credential("secret-bytes", Map::new())
        .unwrap();
    let mode = fs::metadata(world.store.stash_entry_path(&entry_id))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let dir = fs::metadata(world.store.credentials_dir())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(dir & 0o777, 0o700, "the folder is kept 0700, as cswap's");
}

#[test]
fn two_snapshots_same_refresh_token_never_collide() {
    let world = World::new();
    let store = &world.store;
    let a = json!({"claudeAiOauth": {"accessToken": "sk-a", "refreshToken": "rt"}}).to_string();
    let b = json!({"claudeAiOauth": {"accessToken": "sk-b", "refreshToken": "rt"}}).to_string();
    let id_a = store.write_unclaimed_credential(&a, Map::new()).unwrap();
    let id_b = store.write_unclaimed_credential(&b, Map::new()).unwrap();
    assert_ne!(id_a, id_b);
    assert_eq!(read_safety_copy(store, &id_a), a);
    assert_eq!(read_safety_copy(store, &id_b), b);
}

#[test]
fn orphaned_entry_file_still_listed() {
    let world = World::new();
    let store = &world.store;
    let entry_id = store
        .write_unclaimed_credential("bytes", Map::new())
        .unwrap();
    fs::remove_file(store.stash_manifest_path()).unwrap();
    assert!(store.list_unclaimed_credentials().contains_key(&entry_id));
}

#[test]
fn identical_bytes_same_second_get_distinct_ids() {
    // The sandbox's clock stands still: both land in one second.
    let world = World::new();
    let store = &world.store;
    let id_a = store
        .write_unclaimed_credential("same-bytes", Map::new())
        .unwrap();
    let id_b = store
        .write_unclaimed_credential("same-bytes", Map::new())
        .unwrap();
    assert_ne!(id_a, id_b);
    for entry_id in [id_a, id_b] {
        assert_eq!(read_safety_copy(store, &entry_id), "same-bytes");
    }
}

#[test]
fn corrupt_manifest_is_preserved_not_clobbered() {
    let world = World::new();
    let store = &world.store;
    let entry_id = store
        .write_unclaimed_credential("bytes-1", context(json!({"reason": "x"})))
        .unwrap();
    fs::write(store.stash_manifest_path(), "{ not json !!!").unwrap();
    let new_id = store
        .write_unclaimed_credential("bytes-2", context(json!({"reason": "y"})))
        .unwrap();
    // Set aside, not destroyed...
    let corrupt: Vec<_> = fs::read_dir(store.credentials_dir())
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".unclaimed-manifest.json.corrupt-")
        })
        .collect();
    assert_eq!(corrupt.len(), 1);
    assert!(
        fs::read_to_string(corrupt[0].path())
            .unwrap()
            .contains("not json")
    );
    // ...the new manifest is valid, and the older entry's bytes are still listed.
    let entries = store.list_unclaimed_credentials();
    assert_eq!(entries[&new_id]["reason"], "y");
    assert!(entries.contains_key(&entry_id));
    assert_eq!(read_safety_copy(store, &entry_id), "bytes-1");
}

const ROWS: usize = 40;

/// Two stashers and a stash-then-retire churner at once; `gate` wraps each mutation. The
/// probe tags whose rows didn't survive.
fn run(store: &Arc<CredentialStore>, gate: &Arc<Mutex<()>>, serialized: bool) -> Vec<String> {
    let stash = |tag: &'static str| {
        let (store, gate) = (store.clone(), gate.clone());
        std::thread::spawn(move || {
            for i in 0..ROWS {
                let _held = serialized.then(|| gate.lock().unwrap());
                store
                    .write_unclaimed_credential(
                        &format!("creds-{tag}{i}"),
                        context(json!({
                            "reason": "consume-gate-persist-lock-failed",
                            "configSlot": "1",
                            "consumedFp": "fp",
                            "probe": format!("{tag}{i}"),
                        })),
                    )
                    .unwrap();
            }
        })
    };
    let churn = {
        let (store, gate) = (store.clone(), gate.clone());
        std::thread::spawn(move || {
            for i in 0..ROWS {
                let entry_id = {
                    let _held = serialized.then(|| gate.lock().unwrap());
                    store
                        .write_unclaimed_credential(
                            &format!("retire-{i}"),
                            context(json!({"configSlot": "1", "consumedFp": "other"})),
                        )
                        .unwrap()
                };
                let _held = serialized.then(|| gate.lock().unwrap());
                store.remove_unclaimed_credential(&entry_id).unwrap();
            }
        })
    };
    let workers = [stash("a"), churn, stash("b")];
    for worker in workers {
        worker.join().unwrap();
    }
    let probes: Vec<String> = store
        .read_stash_manifest()
        .values()
        .filter_map(|meta| meta["probe"].as_str().map(str::to_owned))
        .collect();
    ["a", "b"]
        .iter()
        .flat_map(|tag| (0..ROWS).map(move |i| format!("{tag}{i}")))
        .filter(|probe| !probes.contains(probe))
        .collect()
}

#[test]
fn concurrent_stash_and_retire_lose_no_manifest_row() {
    let world = World::new();
    let lost = run(&world.store, &Arc::new(Mutex::new(())), false);
    assert_eq!(
        lost,
        Vec::<String>::new(),
        "rows lost to a concurrent manifest rewrite"
    );
}

#[test]
fn control_serialized_mutation_loses_no_row() {
    let world = World::new();
    let lost = run(&world.store, &Arc::new(Mutex::new(())), true);
    assert_eq!(lost, Vec::<String>::new());
}

#[test]
fn a_stash_write_refuses_an_unreadable_manifest() {
    let world = World::new();
    let store = &world.store;
    store
        .write_unclaimed_credential("successor", context(json!({"consumedFp": "fp"})))
        .unwrap();
    let path = store.stash_manifest_path();
    let before = fs::read(&path).unwrap();
    chmod(&path, 0o000);
    let stashed = store.write_unclaimed_credential(
        "old",
        context(json!({"reason": "consume-gate-persist-failed", "configSlot": "1"})),
    );
    chmod(&path, 0o600);
    assert!(
        matches!(stashed, Err(AccountsError::CredentialRead(_))),
        "{stashed:?}"
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "a healthy manifest was replaced"
    );
    let aside = fs::read_dir(store.credentials_dir())
        .unwrap()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"));
    assert!(!aside, "a readable manifest was set aside as corrupt");
}

#[test]
fn an_unlistable_dir_counts_as_entries_at_risk() {
    let world = World::new();
    let store = &world.store;
    store
        .write_unclaimed_credential("successor", Map::new())
        .unwrap();
    assert!(store.stash_entry_files_exist(), "premise");
    let dir = store.credentials_dir();
    chmod(&dir, 0o311); // searchable, files reachable, but unlistable
    let at_risk = store.stash_entry_files_exist();
    chmod(&dir, 0o700);
    assert!(at_risk);
}

#[test]
fn a_missing_credentials_dir_is_provably_nothing_stashed() {
    let world = World::new();
    assert!(!world.store.credentials_dir().exists());
    assert!(!world.store.stash_entry_files_exist());
}

#[test]
fn a_manifest_without_a_dict_entries_member_is_corrupt() {
    let world = World::new();
    let store = &world.store;
    fs::create_dir_all(store.credentials_dir()).unwrap();
    for payload in [
        r#"{"schemaVersion": 1, "entries": "bogus"}"#,
        r#"{"schemaVersion": 1}"#,
    ] {
        fs::write(store.stash_manifest_path(), payload).unwrap();
        assert_eq!(
            store.read_stash_manifest_ex(),
            (Map::new(), ManifestVerdict::Corrupt),
            "{payload}"
        );
    }
}

#[test]
fn a_manifest_of_invalid_utf8_is_corrupt_not_unreadable() {
    let world = World::new();
    let store = &world.store;
    store
        .write_unclaimed_credential("successor", Map::new())
        .unwrap();
    fs::write(store.stash_manifest_path(), b"\xff\xfe\x00not utf8").unwrap();
    assert_eq!(
        store.read_stash_manifest_ex(),
        (Map::new(), ManifestVerdict::Corrupt)
    );
    assert_eq!(ManifestVerdict::Corrupt.as_str(), "corrupt");
}

/// The store half of the gate's rows: what `read_unclaimed_credential` answers for an entry
/// that's corrupt, undecodable, unreadable or gone.
fn entry_with(world: &World, bytes: &[u8]) -> String {
    let entry_id = world
        .store
        .write_unclaimed_credential("live", Map::new())
        .unwrap();
    fs::write(world.store.stash_entry_path(&entry_id), bytes).unwrap();
    entry_id
}

#[test]
fn corrupt_entry_keeps_its_current_behaviour() {
    let world = World::new();
    let entry_id = entry_with(&world, b"not-valid-base64!!!");
    assert_eq!(
        world.store.read_unclaimed_credential(&entry_id),
        (String::new(), false)
    );
}

#[test]
fn undecodable_entry_bytes_are_corrupt_not_a_crash() {
    let world = World::new();
    let entry_id = entry_with(&world, b"\xff\xfegarbled");
    assert_eq!(
        world.store.read_unclaimed_credential(&entry_id),
        (String::new(), false)
    );
}

#[test]
fn row_a_unreadable_entry_does_not_post_the_spent_generation() {
    let world = World::new();
    let entry_id = world
        .store
        .write_unclaimed_credential("live", Map::new())
        .unwrap();
    let path = world.store.stash_entry_path(&entry_id);
    chmod(&path, 0o000);
    let read = world.store.read_unclaimed_credential(&entry_id);
    chmod(&path, 0o600);
    assert_eq!(
        read,
        (String::new(), true),
        "the only copy must not read as absent"
    );
    assert!(
        world
            .store
            .list_unclaimed_credentials()
            .contains_key(&entry_id)
    );
}

#[test]
fn row_d_absent_entry_bytes_terminate_instead_of_deferring() {
    let world = World::new();
    let entry_id = world
        .store
        .write_unclaimed_credential("live", Map::new())
        .unwrap();
    assert_eq!(
        world.store.read_unclaimed_credential(&entry_id),
        ("live".into(), false)
    );
    world.store.remove_unclaimed_credential(&entry_id).unwrap();
    assert_eq!(
        world.store.read_unclaimed_credential(&entry_id),
        (String::new(), false)
    );
    assert!(
        !world
            .store
            .list_unclaimed_credentials()
            .contains_key(&entry_id)
    );
    // A second retire of the same id is housekeeping, not a failure.
    world.store.remove_unclaimed_credential(&entry_id).unwrap();
}

#[test]
fn whitespace_around_an_entry_is_stripped_as_python_strips_bytes() {
    let world = World::new();
    let encoded = base64::engine::general_purpose::STANDARD.encode("live");
    let entry_id = entry_with(&world, format!("\x0b {encoded}\n").as_bytes());
    assert_eq!(
        world.store.read_unclaimed_credential(&entry_id),
        ("live".into(), false)
    );
}
