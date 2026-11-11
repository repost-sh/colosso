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

/// `xcrun simctl` with `args`: what it printed, or else what it said went wrong.
pub(super) fn xcrun_simctl(args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let out = Command::new("/usr/bin/xcrun")
        .arg("simctl")
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

/// `simctl --set <set>` with `args`.
fn simctl(set: &Path, args: &[&str]) -> Result<String, String> {
    let mut all: Vec<&std::ffi::OsStr> = vec!["--set".as_ref(), set.as_os_str()];
    all.extend(args.iter().map(std::ffi::OsStr::new));
    xcrun_simctl(&all)
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

/// A device a system can be made as: `iPhone 18 Pro`, and its device type's id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub name: String,
    pub ipad: bool,
    kind: String,
}

impl Model {
    #[cfg(any(test, feature = "visual-check"))]
    pub fn new(name: &str, ipad: bool) -> Model {
        Model {
            name: name.into(),
            ipad,
            kind: String::new(),
        }
    }
}

/// The newest iOS system ready to run, as `simctl list runtimes -j` lists it.
struct Runtime {
    id: String,
    version: String,
    /// Newest first, iPhones before iPads.
    models: Vec<Model>,
}

/// A version's numbers, to compare: `27.2` as `[27, 2]`.
pub(super) fn numbers(version: &str) -> Vec<u32> {
    version.split('.').filter_map(|n| n.parse().ok()).collect()
}

/// An ordinary iOS system's id, `…SimRuntime.iOS-27-2`: not a device's own
/// (`…iOS-27-2-iPhone19-4`).
pub(super) fn ordinary(runtime: &str) -> bool {
    (runtime.rsplit('.').next())
        .and_then(|name| name.strip_prefix("iOS-"))
        .is_some_and(|version| version.chars().all(|c| c.is_ascii_digit() || c == '-'))
}

fn newest(listed: &Value) -> Option<Runtime> {
    let runtime = (listed["runtimes"].as_array()?.iter())
        .filter(|runtime| {
            runtime["platform"] == "iOS"
                && runtime["isAvailable"] == true
                && runtime["identifier"].as_str().is_some_and(ordinary)
        })
        .max_by_key(|runtime| numbers(runtime["version"].as_str().unwrap_or("0")))?;
    let mut models: Vec<Model> = (runtime["supportedDeviceTypes"].as_array().into_iter().flatten())
        .filter_map(|kind| {
            let family = kind["productFamily"].as_str()?;
            (family == "iPhone" || family == "iPad").then_some(Model {
                name: kind["name"].as_str()?.to_owned(),
                ipad: family == "iPad",
                kind: kind["identifier"].as_str()?.to_owned(),
            })
        })
        .collect();
    // Stable: each family keeps simctl's order, newest first.
    models.sort_by_key(|model| model.ipad);
    Some(Runtime {
        id: runtime["identifier"].as_str()?.to_owned(),
        version: runtime["version"].as_str()?.to_owned(),
        models,
    })
}

fn runtimes(set: &Path) -> Result<Option<Runtime>, String> {
    let listed = simctl(set, &["list", "runtimes", "-j"])?;
    Ok(serde_json::from_str(&listed).ok().as_ref().and_then(newest))
}

/// What the newest system installed can be made as.
pub fn models(set: &Path) -> Vec<Model> {
    runtimes(set).ok().flatten().map(|runtime| runtime.models).unwrap_or_default()
}

/// Why a simulator wasn't made.
#[derive(Debug)]
pub enum Unmade {
    /// The newest system has no simulator by the name asked for.
    Model(String),
    /// What it takes is missing, the person's to put right: Xcode, a simulator system, the set.
    Missing(String),
}

impl std::fmt::Display for Unmade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unmade::Model(why) | Unmade::Missing(why) => f.write_str(why),
        }
    }
}

/// The model of `runtime` named `name` in any case; with no name, its newest iPhone.
fn model_in<'a>(runtime: &'a Runtime, name: Option<&str>) -> Result<&'a Model, Unmade> {
    let Some(name) = name else {
        return (runtime.models.iter().find(|model| !model.ipad))
            .ok_or_else(|| Unmade::Missing(format!("iOS {} has no iPhone to make.", runtime.version)));
    };
    (runtime.models.iter()).find(|model| model.name.eq_ignore_ascii_case(name.trim())).ok_or_else(|| {
        Unmade::Model(format!(
            "iOS {} has no simulator called {name:?}: name one of the models `xcrun simctl list runtimes -j` gives it.",
            runtime.version
        ))
    })
}

/// A new simulator in Koyo's set on the newest iOS installed: of the model `name` names (a
/// device type's name, as `iPad Pro 13-inch (M5)`), else the newest iPhone.
pub fn make(set: &Path, name: Option<&str>) -> Result<Simulator, Unmade> {
    std::fs::create_dir_all(set).map_err(|error| Unmade::Missing(format!("Koyo's device set can't be made: {error}")))?;
    let runtime = (runtimes(set).map_err(Unmade::Missing)?)
        .ok_or_else(|| Unmade::Missing("No iOS simulator system is installed.".into()))?;
    let model = model_in(&runtime, name)?;
    let udid = simctl(set, &["create", &model.name, &model.kind, &runtime.id]).map_err(Unmade::Missing)?.trim().to_owned();
    Ok(Simulator {
        udid,
        name: model.name.clone(),
        system: format!("iOS {}", runtime.version),
        booted: false,
        booted_before: false,
        ipad: model.ipad,
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
    fn the_newest_ios_and_its_models_are_read_and_the_duos_own_left_out() {
        let listed: Value = serde_json::from_str(r#"{"runtimes":[
            {"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-26-5","platform":"iOS","version":"26.5","isAvailable":true,
             "supportedDeviceTypes":[{"name":"iPhone 17 Pro","identifier":"T-17","productFamily":"iPhone"}]},
            {"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-27-2-iPhone19-4","platform":"iOS","version":"27.2","isAvailable":true,
             "supportedDeviceTypes":[{"name":"iPhone Duo","identifier":"T-DUO","productFamily":"iPhone"}]},
            {"identifier":"com.apple.CoreSimulator.SimRuntime.iOS-27-2","platform":"iOS","version":"27.2","isAvailable":true,
             "supportedDeviceTypes":[
                {"name":"iPad Pro 13-inch (M5)","identifier":"T-IPAD","productFamily":"iPad"},
                {"name":"iPhone 18 Pro","identifier":"T-18","productFamily":"iPhone"},
                {"name":"Apple Watch","identifier":"T-W","productFamily":"Apple Watch"}]}]}"#).unwrap();
        let runtime = newest(&listed).unwrap();
        assert_eq!((runtime.id.as_str(), runtime.version.as_str()), ("com.apple.CoreSimulator.SimRuntime.iOS-27-2", "27.2"));
        let names: Vec<&str> = runtime.models.iter().map(|model| model.name.as_str()).collect();
        assert_eq!(names, ["iPhone 18 Pro", "iPad Pro 13-inch (M5)"]);
        // With no name, the newest iPhone; a name in any case; one the system hasn't, refused.
        assert_eq!(model_in(&runtime, None).unwrap().kind, "T-18");
        assert_eq!(model_in(&runtime, Some("ipad pro 13-inch (m5)")).unwrap().kind, "T-IPAD");
        assert!(matches!(model_in(&runtime, Some("iPhone 99")), Err(Unmade::Model(_))));
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
