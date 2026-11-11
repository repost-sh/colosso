//! The global config: its read-modify-write keeps what it doesn't own, every other key where the
//! file had it, and refuses to write over a file it couldn't read (pitfall 21); the identity read
//! from it is kept until the file changes.

use super::World;
use crate::{AccountsError, claude_locks, paths};

#[test]
fn a_change_keeps_every_other_key_where_the_file_had_it() {
    let world = World::new();
    let env = world.store.env().clone();
    let path = paths::global_config_path(&env);
    std::fs::write(
        &path,
        r#"{"numStartups": 3, "oauthAccount": {"emailAddress": "a@x.com"}, "projects": {"/w": {}}, "autoUpdates": false}"#,
    )
    .unwrap();
    let lock = claude_locks::claude_config_lock(&env, None).unwrap();
    world
        .store
        .update_global_config(&lock, |config| {
            config.insert(
                "oauthAccount".into(),
                serde_json::json!({"emailAddress": "b@x.com"}),
            );
        })
        .unwrap();
    let written = std::fs::read_to_string(&path).unwrap();
    let order: Vec<usize> = ["numStartups", "oauthAccount", "projects", "autoUpdates"]
        .iter()
        .map(|key| written.find(&format!("\"{key}\"")).unwrap())
        .collect();
    assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{written}");
    assert!(written.contains("b@x.com") && !written.contains("a@x.com"));
}

#[test]
fn a_config_that_cant_be_read_is_never_written_over() {
    let world = World::new();
    let env = world.store.env().clone();
    let path = paths::global_config_path(&env);
    std::fs::write(&path, r#"{"projects": {"#).unwrap();
    let lock = claude_locks::claude_config_lock(&env, None).unwrap();
    let result = world.store.update_global_config(&lock, |config| {
        config.insert("oauthAccount".into(), serde_json::json!({}));
    });
    assert!(matches!(result, Err(AccountsError::CredentialWrite(_))));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"projects": {"#);
}

#[test]
fn the_identity_is_kept_until_the_config_changes() {
    let world = World::new();
    let path = paths::global_config_path(world.store.env());
    let email = |world: &World| {
        let oauth = world.store.read_oauth_account()?;
        oauth.get("emailAddress")?.as_str().map(str::to_owned)
    };
    assert_eq!(email(&world), None);
    std::fs::write(&path, r#"{"oauthAccount": {"emailAddress": "a@x.com"}}"#).unwrap();
    assert_eq!(email(&world).as_deref(), Some("a@x.com"));
    // Claude Code writes it anew (a new inode, as its atomic write): read again.
    let next = path.with_extension("next");
    std::fs::write(
        &next,
        r#"{"oauthAccount": {"emailAddress": "b@x.com"}, "n": 1}"#,
    )
    .unwrap();
    std::fs::rename(&next, &path).unwrap();
    assert_eq!(email(&world).as_deref(), Some("b@x.com"));
    // In place, a different size: read again too.
    std::fs::write(
        &path,
        r#"{"oauthAccount": {"emailAddress": "c@x.com"}, "nn": 22}"#,
    )
    .unwrap();
    assert_eq!(email(&world).as_deref(), Some("c@x.com"));
}
