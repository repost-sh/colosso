//! Koyo's own simulators (`docs/devices.md` 4): those of its device set, read with
//! `simctl --set <set> list -j` when a Devices surface wants them, made with `simctl create`,
//! and those a killed host left booted shut down. The person's own Xcode simulators are never
//! listed, booted or touched: they live in Xcode's set, not this one.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

/// A simulator of Koyo's set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Simulator {
    pub udid: String,
    pub name: String,
    /// Its system, as the person reads it: `iOS 27.2`.
    pub system: String,
    /// Whether it runs now: it counts against the simulators running at once.
    pub booted: bool,
    /// Whether it has ever booted: a first boot takes about a minute.
    pub booted_before: bool,
    /// Whether it's an iPad, for the words that name it.
    pub ipad: bool,
}

fn simctl(set: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("/usr/bin/xcrun")
        .arg("simctl")
        .arg("--set")
        .arg(set)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("simctl didn't run: {error}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

/// `com.apple.CoreSimulator.SimRuntime.iOS-27-2` as `iOS 27.2`.
fn system(runtime: &str) -> String {
    let name = runtime.rsplit('.').next().unwrap_or(runtime);
    let mut parts = name.split('-');
    let platform = parts.next().unwrap_or_default();
    let version: Vec<&str> = parts.take_while(|part| part.chars().all(|c| c.is_ascii_digit())).collect();
    format!("{platform} {}", version.join("."))
}

/// The simulators `simctl list devices -j` lists.
pub fn parse(json: &str) -> Vec<Simulator> {
    let Ok(listed) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(by_runtime) = listed["devices"].as_object() else {
        return Vec::new();
    };
    let mut all: Vec<Simulator> = by_runtime
        .iter()
        .flat_map(|(runtime, devices)| {
            devices.as_array().into_iter().flatten().filter_map(move |device| {
                Some(Simulator {
                    udid: device["udid"].as_str()?.to_owned(),
                    name: device["name"].as_str()?.to_owned(),
                    system: system(runtime),
                    booted: device["state"] == "Booted",
                    booted_before: device.get("lastBootedAt").is_some(),
                    ipad: device["deviceTypeIdentifier"].as_str().is_some_and(|kind| kind.contains("iPad")),
                })
            })
        })
        .collect();
    all.sort_by(|a, b| a.name.cmp(&b.name).then(a.udid.cmp(&b.udid)));
    all
}

pub fn list(set: &Path) -> Result<Vec<Simulator>, String> {
    std::fs::create_dir_all(set).map_err(|error| format!("Koyo's device set can't be made: {error}"))?;
    simctl(set, &["list", "devices", "-j"]).map(|json| parse(&json))
}

/// The newest iOS system installed and its id, from `simctl list runtimes -j`.
fn newest_ios(json: &str) -> Option<(String, String)> {
    let listed: Value = serde_json::from_str(json).ok()?;
    listed["runtimes"]
        .as_array()?
        .iter()
        .filter(|runtime| runtime["platform"] == "iOS" && runtime["isAvailable"] == true)
        // An ordinary one, `…SimRuntime.iOS-27-2`: not a device's own (`…iOS-27-2-iPhone19-4`).
        .filter(|runtime| {
            runtime["identifier"].as_str().is_some_and(|id| {
                id.rsplit('.')
                    .next()
                    .and_then(|name| name.strip_prefix("iOS-"))
                    .is_some_and(|version| version.chars().all(|c| c.is_ascii_digit() || c == '-'))
            })
        })
        .max_by(|a, b| {
            let version = |r: &Value| -> Vec<u32> {
                r["version"].as_str().unwrap_or("0").split('.').filter_map(|n| n.parse().ok()).collect()
            };
            version(a).cmp(&version(b))
        })
        .and_then(|runtime| Some((runtime["identifier"].as_str()?.to_owned(), runtime["version"].as_str()?.to_owned())))
}

/// A new iPhone 17 Pro in Koyo's set, on the newest iOS installed.
pub fn make_default(set: &Path) -> Result<Simulator, String> {
    make(set, "iPhone 17 Pro").map_err(|unmade| unmade.to_string())
}

/// Why a simulator wasn't made.
#[derive(Debug)]
pub enum Unmade {
    /// Xcode has no simulator by the name asked for.
    Model(String),
    /// What it takes is missing, the person's to put right: Xcode, a simulator system, the set.
    Missing(String),
}

/// A new simulator of `model` (a device type's name, as `iPad Pro 13-inch (M5)`) in Koyo's
/// set, on the newest iOS installed.
pub fn make(set: &Path, model: &str) -> Result<Simulator, Unmade> {
    std::fs::create_dir_all(set).map_err(|error| Unmade::Missing(format!("Koyo's device set can't be made: {error}")))?;
    let types = simctl(set, &["list", "devicetypes", "-j"]).map_err(Unmade::Missing)?;
    let kind = device_type(&types, model).ok_or_else(|| {
        Unmade::Model(format!("Xcode has no simulator called {model:?}: name one as `xcrun simctl list devicetypes` does."))
    })?;
    let runtimes = simctl(set, &["list", "runtimes", "-j"]).map_err(Unmade::Missing)?;
    let (runtime, version) =
        newest_ios(&runtimes).ok_or_else(|| Unmade::Missing("No iOS simulator system is installed.".into()))?;
    let udid = simctl(set, &["create", model, &kind, &runtime]).map_err(Unmade::Missing)?.trim().to_owned();
    Ok(Simulator {
        udid,
        name: model.to_owned(),
        system: format!("iOS {version}"),
        booted: false,
        booted_before: false,
        ipad: model.starts_with("iPad"),
    })
}

impl std::fmt::Display for Unmade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unmade::Model(why) | Unmade::Missing(why) => f.write_str(why),
        }
    }
}

/// The identifier of the device type named `model`, any case, from `simctl list devicetypes -j`.
fn device_type(json: &str, model: &str) -> Option<String> {
    let listed: Value = serde_json::from_str(json).ok()?;
    (listed.get("devicetypes")?.as_array()?.iter()).find_map(|kind| {
        let name = kind.get("name")?.as_str()?;
        name.eq_ignore_ascii_case(model.trim())
            .then(|| kind.get("identifier")?.as_str().map(str::to_owned))?
    })
}

/// The lock a device's host holds for as long as it runs (`koyo-device-host` `run.rs`), taken
/// here when free: held, a host started meanwhile waits for it.
fn lock(state: &Path, udid: &str) -> Option<std::fs::File> {
    let file = (std::fs::OpenOptions::new().create(true).truncate(false).write(true))
        .open(state.join(koyo_device_wire::lock_name(udid)))
        .ok()?;
    file.try_lock().is_ok().then_some(file)
}

/// Shuts down what hosts killed last time left booted, by their records, and drops the records;
/// not a simulator a host has taken meanwhile, which holds its lock.
pub fn shut_down_leftovers(set: &Path, state: &Path) {
    for (record, udid) in super::host::leftovers(state) {
        let Some(_held) = lock(state, &udid) else {
            log::info!("simulator {udid} has a host again: left to it");
            continue;
        };
        log::info!("shutting down simulator {udid}, which a stopped host left running");
        if let Err(why) = simctl(set, &["shutdown", &udid])
            && !why.contains("current state: Shutdown")
        {
            log::warn!("simulator {udid} wasn't shut down: {why}");
            continue;
        }
        let _ = std::fs::remove_file(record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulators_read_from_the_set_with_their_systems() {
        let json = r#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-27-2":[
            {"udid":"B","name":"iPhone 17 Pro","state":"Shutdown","deviceTypeIdentifier":"com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro","lastBootedAt":"2026-11-11T10:00:00Z"},
            {"udid":"A","name":"iPad Pro 13","state":"Booted","deviceTypeIdentifier":"com.apple.CoreSimulator.SimDeviceType.iPad-Pro-13-inch-M5-16GB"}]}}"#;
        let all = parse(json);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "iPad Pro 13");
        assert!(all[0].booted && all[0].ipad && !all[0].booted_before);
        assert_eq!(all[1].system, "iOS 27.2");
        assert!(all[1].booted_before);
    }

    #[test]
    fn the_newest_ios_is_chosen_and_the_duo_s_own_runtime_left_out() {
        let json = r#"{"runtimes":[
            {"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-26-5","platform":"iOS","version":"26.5","isAvailable":true},
            {"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-27-2-iPhone19-4","platform":"iOS","version":"27.2","isAvailable":true},
            {"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-27-2","platform":"iOS","version":"27.2","isAvailable":true}]}"#;
        assert_eq!(
            newest_ios(json),
            Some(("com.apple.CoreSimulator.SimRuntime.iOS-27-2".into(), "27.2".into()))
        );
    }

    #[test]
    fn a_model_is_found_by_its_name_in_any_case() {
        let json = r#"{"devicetypes":[
            {"name":"iPhone 17 Pro","identifier":"com.apple.CoreSimulator.SimDeviceType.iPhone-17-Pro"},
            {"name":"iPad Pro 13-inch (M5)","identifier":"com.apple.CoreSimulator.SimDeviceType.iPad-Pro-13-inch-M5-12GB"}
        ]}"#;
        assert_eq!(
            device_type(json, "ipad pro 13-inch (m5)").as_deref(),
            Some("com.apple.CoreSimulator.SimDeviceType.iPad-Pro-13-inch-M5-12GB")
        );
        assert_eq!(device_type(json, "iPhone 99"), None);
    }

    #[test]
    fn a_simulator_whose_host_holds_its_lock_isnt_swept() {
        let state = crate::test_support::scratch("device-lock");
        let held = std::fs::File::create(state.join(koyo_device_wire::lock_name("HELD"))).unwrap();
        held.try_lock().unwrap();
        assert!(lock(&state, "HELD").is_none(), "a host holds it");
        assert!(lock(&state, "FREE").is_some(), "nobody does");
        drop(held);
        assert!(lock(&state, "HELD").is_some(), "let go");
    }
}
