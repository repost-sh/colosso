//! The global config: the identity read from it is kept until the file changes.

use super::World;
use crate::paths;

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
    let next = path.with_extension("next");
    std::fs::write(
        &next,
        r#"{"oauthAccount": {"emailAddress": "b@x.com"}, "n": 1}"#,
    )
    .unwrap();
    std::fs::rename(&next, &path).unwrap();
    assert_eq!(email(&world).as_deref(), Some("b@x.com"));
    std::fs::write(
        &path,
        r#"{"oauthAccount": {"emailAddress": "c@x.com"}, "nn": 22}"#,
    )
    .unwrap();
    assert_eq!(email(&world).as_deref(), Some("c@x.com"));
}
