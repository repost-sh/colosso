//! Bringing a settings file written by an older build to what the app has now: keys it no
//! longer has are dropped, and keys it names otherwise are moved.

use serde_json::{Map, Value};

/// Settings a file may hold that the app no longer has, each where it was and why it went.
/// A key that changes what it means goes here under its old name; one that only changes
/// its name goes in [`RENAMED`].
const RETIRED: [(&[&str], &str); 5] = [
    // The Files panel's tree's width, from before the panel showed a file beside its tree:
    // too narrow for both, it gave way to `files_panel_width` and `files_tree_width`.
    (&[], "files_width"),
    // A held arrow sped up the longer it was held, in steps a frame; it now has one speed,
    // `per_second`, the same whatever the display's rate.
    (&["key_repeat"], "start_per_frame"),
    (&["key_repeat"], "accelerate_after_ms"),
    (&["key_repeat"], "full_speed_after_ms"),
    (&["key_repeat"], "most_per_frame"),
];

/// Settings a file may hold under a name the app no longer uses for them, each where it is,
/// with its old name and its new one. A key whose meaning stays goes here.
const RENAMED: [(&[&str], &str, &str); 2] = [
    // The TypeScript and Python servers were vtsls and basedpyright, npm packages run by
    // Node, named for them; TypeScript's own native server and Pyrefly took over, and the
    // switches are named for the language's server now, whichever engine runs it.
    (&["language_servers"], "vtsls", "typescript"),
    (&["language_servers"], "basedpyright", "pyrefly"),
];

/// Brings settings read from a file to what the app has now: retired keys are dropped, so
/// what they meant isn't taken for what a key of the same name means later, and renamed
/// keys are moved to their new name, unless the file has that one already.
pub(super) fn migrate(value: &mut Value) {
    for (within, key) in RETIRED {
        if let Some(object) = object_within(value, within) {
            object.remove(key);
        }
    }
    for (within, old, new) in RENAMED {
        if let Some(object) = object_within(value, within)
            && let Some(moved) = object.remove(old)
        {
            object.entry(new).or_insert(moved);
        }
    }
}

/// The object at `path` in `value`, if there's one.
fn object_within<'a>(
    value: &'a mut Value,
    path: &[&str],
) -> Option<&'a mut Map<String, Value>> {
    let value = path.iter().try_fold(value, |value, key| value.get_mut(*key))?;
    value.as_object_mut()
}
