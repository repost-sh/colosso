//! Where profiles keep their data in the root, and clearing it without stopping the engine:
//! a profile's folder can't be deleted under a running Chromium, so cleared profiles move to
//! fresh folders, and the old ones go as the host next starts (`docs/native-pages.md` 3).

use std::path::Path;

/// A profile's folder in the root: `profile-<name>`, and once the profiles were cleared
/// `profile-<name>.g<times>`, a fresh one (a name has no dot in it).
pub(super) fn folder(name: &str, generation: u32) -> String {
    if generation == 0 {
        format!("profile-{name}")
    } else {
        format!("profile-{name}.g{generation}")
    }
}

/// Where the root keeps how many times the profiles were cleared.
const GENERATION: &str = "generation";

/// How many times the profiles in `root` were cleared.
pub(super) fn generation(root: &Path) -> u32 {
    std::fs::read_to_string(root.join(GENERATION))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Deletes the folders of profiles cleared before, which were still open when they were
/// cleared, and takes them off Chrome's list of profiles, which it would make again from.
/// Before CEF opens the root, and not on a thread that mustn't wait.
pub(crate) fn drop_cleared(root: &Path) {
    let now = generation(root);
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut dropped = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(rest) = name.strip_prefix("profile-") else {
            continue;
        };
        let of = (rest.rsplit_once(".g")).and_then(|(_, times)| times.parse::<u32>().ok());
        if of.unwrap_or(0) != now && std::fs::remove_dir_all(entry.path()).is_ok() {
            dropped.push(name);
        }
    }
    if !dropped.is_empty() {
        unlist(root, &dropped);
    }
}

/// Takes `profiles` off the list Chrome keeps of the root's profiles (`Local State`): it
/// makes the folder of each one listed again as it starts.
fn unlist(root: &Path, profiles: &[String]) {
    use serde_json::Value;
    let path = root.join("Local State");
    let Some(mut state) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        return;
    };
    let Some(profile) = state.get_mut("profile").and_then(Value::as_object_mut) else {
        return;
    };
    let gone = |name: &Value| name.as_str().is_some_and(|name| profiles.iter().any(|p| p == name));
    if let Some(known) = profile.get_mut("info_cache").and_then(Value::as_object_mut) {
        known.retain(|name, _| !profiles.contains(name));
    }
    for list in ["profiles_order", "last_active_profiles"] {
        if let Some(names) = profile.get_mut(list).and_then(Value::as_array_mut) {
            names.retain(|name| !gone(name));
        }
    }
    if profile.get("last_used").is_some_and(gone) {
        profile.remove("last_used");
    }
    if let Ok(text) = serde_json::to_string(&state) {
        let _ = std::fs::write(&path, text);
    }
}

/// Keeps that the profiles in `root` were cleared `times` times.
pub(super) fn keep_generation(root: &Path, times: u32) {
    if let Err(error) = std::fs::write(root.join(GENERATION), times.to_string()) {
        log::warn!("couldn't keep that the profiles were cleared: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cleared_profile_has_a_folder_of_its_own() {
        assert_eq!(folder("proj-1", 0), "profile-proj-1");
        assert_eq!(folder("proj-1", 3), "profile-proj-1.g3");
    }

    #[test]
    fn folders_cleared_before_go_and_come_off_chromes_list() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        for name in ["profile-a", "profile-a.g1", "profile-a.g2", "Default"] {
            std::fs::create_dir(root.join(name)).unwrap();
        }
        let listed = serde_json::json!({ "profile": {
            "info_cache": { "Default": {}, "profile-a": {}, "profile-a.g1": {}, "profile-a.g2": {} },
            "profiles_order": ["Default", "profile-a", "profile-a.g1", "profile-a.g2"],
            "last_active_profiles": ["profile-a.g1"],
            "last_used": "profile-a",
        }});
        std::fs::write(root.join("Local State"), listed.to_string()).unwrap();
        assert_eq!(generation(root), 0);
        keep_generation(root, 2);
        assert_eq!(generation(root), 2);

        drop_cleared(root);

        let left = |name: &str| root.join(name).is_dir();
        assert!(left("profile-a.g2") && left("Default"));
        assert!(!left("profile-a") && !left("profile-a.g1"));
        let state: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("Local State")).unwrap()).unwrap();
        let profile = &state["profile"];
        let known: Vec<&String> = profile["info_cache"].as_object().unwrap().keys().collect();
        assert_eq!(known, ["Default", "profile-a.g2"]);
        assert_eq!(profile["profiles_order"], serde_json::json!(["Default", "profile-a.g2"]));
        assert_eq!(profile["last_active_profiles"], serde_json::json!([]));
        assert!(profile.get("last_used").is_none());
    }
}
