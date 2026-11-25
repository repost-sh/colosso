//! Bringing a settings file written by an older build to what the app has now: keys it no
//! longer has are dropped, and keys it names otherwise are moved.

use serde_json::{Map, Value};

use super::{CHAT_SIZES, Preset};

/// The sizes a chat's text could be, under `fonts.chat_size`, before they came down a notch.
const OLD_CHAT_SIZES: [f64; 7] = [13., 14., 15., 16., 18., 20., 22.];

/// On the way to a key, each value of a map keyed by what the person has, as `project_looks`
/// holds a look for each project.
const EACH: &str = "*";

/// Settings a file may hold that the app no longer has, each where it was and why it went:
/// where a list is on the way, in each of its entries, and in each value of a map at [`EACH`].
/// A key that changes what it means goes here under its old name; one that only changes its
/// name goes in [`RENAMED`].
const RETIRED: [(&[&str], &str); 17] = [
    (&[], "files_width"),
    (&[], "files_panel_width"),
    (&[], "changes_width"),
    (&["key_repeat"], "start_per_frame"),
    (&["key_repeat"], "accelerate_after_ms"),
    (&["key_repeat"], "full_speed_after_ms"),
    (&["key_repeat"], "most_per_frame"),
    (&[], "native_pages"),
    (&[], "card_glass"),
    (&[], "glass_blur"),
    (&[], "sidebar_blur"),
    (&["chat_backdrop", "surfaces"], "sidebar"),
    (&["spaces"], "light_theme"),
    (&["spaces"], "dark_theme"),
    (&[], "layout"),
    (&["project_looks", EACH], "name"),
    (&["project_looks", EACH], "icon"),
];

/// Settings a file may hold under a name the app no longer uses for them, each where it is,
/// with its old name and its new one. A key whose meaning stays goes here.
const RENAMED: [(&[&str], &str, &str); 2] = [
    (&["language_servers"], "vtsls", "typescript"),
    (&["language_servers"], "basedpyright", "pyrefly"),
];

/// Brings settings read from a file to what the app has now: retired keys are dropped, so
/// what they meant isn't taken for what a key of the same name means later, and renamed
/// keys are moved to their new name, unless the file has that one already.
pub(super) fn migrate(value: &mut Value) {
    for (within, key) in RETIRED {
        for object in objects_within(value, within) {
            object.remove(key);
        }
    }
    for (within, old, new) in RENAMED {
        for object in objects_within(value, within) {
            if let Some(moved) = object.remove(old) {
                object.entry(new).or_insert(moved);
            }
        }
    }
    chat_work(value);
    chat_size(value);
}

/// A chat's text sizes came down a notch: what was picked of the old ones is the size at the same
/// place on the slider now, the old default the new one.
fn chat_size(value: &mut Value) {
    let Some(fonts) = value.get_mut("fonts").and_then(Value::as_object_mut) else {
        return;
    };
    let Some(old) = fonts.remove("chat_size").and_then(|old| old.as_f64()) else {
        return;
    };
    let distance = |stop: &f64| (stop - old).abs();
    let place = (0..OLD_CHAT_SIZES.len())
        .min_by(|a, b| distance(&OLD_CHAT_SIZES[*a]).total_cmp(&distance(&OLD_CHAT_SIZES[*b])));
    if let Some(size) = place.and_then(|place| CHAT_SIZES.get(place)) {
        fonts
            .entry("chat_text_size")
            .or_insert(Value::from(f64::from(*size)));
    }
}

/// "Work to show" became Verbosity, a choice for each kind of step: what was picked of the three
/// it had is the preset that shows as much.
fn chat_work(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let Some(work) = object.remove("chat_work") else {
        return;
    };
    let preset = match work.as_str() {
        Some("Turns") => Preset::Quiet,
        Some("Everything") => Preset::Verbose,
        _ => Preset::Normal,
    };
    if let Ok(verbosity) = serde_json::to_value(preset.verbosity()) {
        object.entry("chat_verbosity").or_insert(verbosity);
    }
}

/// The objects at `path` in `value`: one at most, unless a list is on the way, which stands for
/// each of its entries, as `spaces` holds an object for each space, or [`EACH`], which stands
/// for each value of the map there.
fn objects_within<'a>(value: &'a mut Value, path: &[&str]) -> Vec<&'a mut Map<String, Value>> {
    match value {
        Value::Array(entries) => (entries.iter_mut())
            .flat_map(|entry| objects_within(entry, path))
            .collect(),
        Value::Object(object) => match path.split_first() {
            None => vec![object],
            Some((key, rest)) if *key == EACH => (object.values_mut())
                .flat_map(|inner| objects_within(inner, rest))
                .collect(),
            Some((key, rest)) => (object.get_mut(*key))
                .map_or_else(Vec::new, |inner| objects_within(inner, rest)),
        },
        _ => Vec::new(),
    }
}
