use serde_json::json;

use super::fixtures::{found, ready};
use super::*;

#[test]
fn an_xcodes_version_is_read_from_its_version_plist() {
    let plist = "<dict>\n\t<key>CFBundleShortVersionString</key>\n\t<string>27.2</string>\n</dict>";
    assert_eq!(short_version(plist).as_deref(), Some("27.2"));
    assert_eq!(short_version("<dict></dict>"), None);
}

#[test]
fn xcode_stands_by_the_one_in_use_and_the_newest_installed() {
    let all = |versions: &[&str]| versions.iter().map(|&version| version.to_owned()).collect::<Vec<_>>();
    // The one in use is new enough: the others aren't read.
    let read = std::cell::Cell::new(false);
    let installed = || {
        read.set(true);
        Vec::new()
    };
    assert_eq!(stands(Some("27.2".into()), installed), Xcode::Ready("27.2".into()));
    assert!(!read.get());
    // Older, and nothing newer.
    assert_eq!(stands(Some("26.4".into()), || all(&["26.4"])), Xcode::Older("26.4".into()));
    // The Command Line Tools in use, an old Xcode installed.
    assert_eq!(stands(None, || all(&["25.0", "26.1"])), Xcode::Older("26.1".into()));
    // A new one installed but another in use, or the Command Line Tools.
    assert_eq!(
        stands(Some("26.4".into()), || all(&["26.4", "27.0", "27.2"])),
        Xcode::Other {
            newest: "27.2".into(),
            selected: Some("26.4".into())
        }
    );
    assert_eq!(
        stands(None, || all(&["27.1"])),
        Xcode::Other {
            newest: "27.1".into(),
            selected: None
        }
    );
    assert_eq!(stands(None, Vec::new), Xcode::Missing);
}

#[test]
fn the_systems_are_the_ordinary_ios_ones_ready_newest_first() {
    let listed = json!({
        "A": {"platformIdentifier": "com.apple.platform.iphonesimulator", "state": "Ready",
              "runtimeIdentifier": "com.apple.CoreSimulator.SimRuntime.iOS-26-5", "version": "26.5", "sizeBytes": 8_494_282_293_u64},
        "B": {"platformIdentifier": "com.apple.platform.iphonesimulator", "state": "Ready",
              "runtimeIdentifier": "com.apple.CoreSimulator.SimRuntime.iOS-27-2", "version": "27.2", "sizeBytes": 8_315_434_484_u64},
        "C": {"platformIdentifier": "com.apple.platform.iphonesimulator", "state": "Ready",
              "runtimeIdentifier": "com.apple.CoreSimulator.SimRuntime.iOS-27-2-iPhone19-4", "version": "27.2", "sizeBytes": 1},
        "D": {"platformIdentifier": "com.apple.platform.appletvsimulator", "state": "Ready",
              "runtimeIdentifier": "com.apple.CoreSimulator.SimRuntime.tvOS-27-2", "version": "27.2", "sizeBytes": 1},
        "E": {"platformIdentifier": "com.apple.platform.iphonesimulator", "state": "Downloading",
              "runtimeIdentifier": "com.apple.CoreSimulator.SimRuntime.iOS-27-3", "version": "27.3", "sizeBytes": 1}
    });
    let systems = parse_systems(&listed);
    let names: Vec<&str> = systems.iter().map(|system| system.name.as_str()).collect();
    assert_eq!(names, ["iOS 27.2", "iOS 26.5"]);
}

#[test]
fn the_first_thing_missing_is_the_one_said() {
    let mut mac = found(Xcode::Older("26.4".into()), false, false);
    assert_eq!(mac.need(), Some(Need::Xcode));
    assert_eq!(mac.sentence(Need::Xcode), "Koyo's devices need Xcode 27 or later. You have Xcode 26.4.");
    mac.xcode = ready();
    assert_eq!(mac.need(), Some(Need::Components));
    mac.components = true;
    assert_eq!(mac.need(), Some(Need::System));
    let mac = found(ready(), true, true);
    assert_eq!(mac.need(), None);
    assert_eq!(mac.default_model().map(|model| model.name.as_str()), Some("iPhone 18 Pro"));
}
