//! A device's apps: what is installed, installing one built for the simulator, launching and
//! stopping one, and opening a link. All but the list take the device for their caller.

use std::path::Path;

use gpui_kit::App;
use koyo_app_control::ControlError;
use koyo_app_control::ops::device::{
    DeviceApp, DeviceAppsOutput, DeviceDone, DeviceInstallInput, DeviceLaunchInput,
    DeviceOpenUrlInput, DeviceTerminateInput, Which,
};
use koyo_device_wire::{Answer, Ask};
use serde_json::Value;

use super::{APPS, INSTALL, Target, act_on, asking, unexpected, which};
use crate::control::Caller;
use crate::control::exec::{Exec, output};

/// An answer that says it was done, on `target`.
fn done(target: &Target) -> impl FnOnce(Answer) -> Result<Value, ControlError> + use<> {
    let (device, name) = (target.udid.clone(), target.name.clone());
    move |answer| match answer {
        Answer::Done => output(&DeviceDone { device, name }),
        other => Err(unexpected(&other)),
    }
}

pub(crate) fn apps(caller: &Caller, input: &Which, cx: &mut App) -> Result<Exec, ControlError> {
    let target = which(caller, input.device.as_deref(), input.pane, cx)?;
    let name = target.name.clone();
    Ok(asking(&target, Ask::Apps, APPS, cx, move |answer| match answer {
        Answer::Apps { apps } => {
            let mut apps: Vec<DeviceApp> = (apps.into_iter())
                .map(|app| DeviceApp {
                    name: app.name,
                    bundle: app.bundle,
                })
                .collect();
            apps.sort_by_key(|app| app.name.to_lowercase());
            output(&DeviceAppsOutput { name, apps })
        }
        other => Err(unexpected(&other)),
    }))
}

/// The app at `path`, if it is one a simulator installs: a folder ending `.app`, by an absolute
/// path that exists.
fn installable(path: &str) -> Result<String, ControlError> {
    let given = Path::new(path);
    if !given.is_absolute() {
        return Err(ControlError::invalid_input(format!("`path` is absolute: {path} isn't.")));
    }
    if given.extension().is_none_or(|extension| extension != "app") || !given.is_dir() {
        return Err(ControlError::invalid_input(format!(
            "{path} isn't an `.app` built for the simulator: build with `xcodebuild -sdk iphonesimulator` and give the `.app` in its products."
        )));
    }
    Ok(path.to_owned())
}

pub(crate) fn install(caller: &Caller, input: &DeviceInstallInput, cx: &mut App) -> Result<Exec, ControlError> {
    let ask = installable(&input.path).map(|path| Ask::Install { path });
    act_on(caller, (input.device.as_deref(), input.pane), ask, INSTALL, cx, done)
}

/// A bundle id as given, if it reads as one.
fn bundle(bundle: &str) -> Result<String, ControlError> {
    let bundle = bundle.trim();
    let reads = !bundle.is_empty()
        && bundle.contains('.')
        && (bundle.chars()).all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if reads {
        Ok(bundle.to_owned())
    } else {
        Err(ControlError::invalid_input(format!(
            "{bundle:?} isn't a bundle id: device_apps lists them, as `com.apple.Preferences`."
        )))
    }
}

pub(crate) fn launch(caller: &Caller, input: &DeviceLaunchInput, cx: &mut App) -> Result<Exec, ControlError> {
    let ask = bundle(&input.bundle).map(|bundle| Ask::Launch {
        bundle,
        arguments: input.arguments.clone().unwrap_or_default(),
    });
    act_on(caller, (input.device.as_deref(), input.pane), ask, APPS, cx, done)
}

pub(crate) fn terminate(caller: &Caller, input: &DeviceTerminateInput, cx: &mut App) -> Result<Exec, ControlError> {
    let ask = bundle(&input.bundle).map(|bundle| Ask::Terminate { bundle });
    act_on(caller, (input.device.as_deref(), input.pane), ask, APPS, cx, done)
}

pub(crate) fn open_url(caller: &Caller, input: &DeviceOpenUrlInput, cx: &mut App) -> Result<Exec, ControlError> {
    let url = input.url.trim();
    let ask = if url.contains(':') && !url.contains(char::is_whitespace) {
        Ok(Ask::OpenUrl { url: url.to_owned() })
    } else {
        Err(ControlError::invalid_input(format!(
            "{url:?} isn't a link: give its scheme, as `https://example.com` or `myapp://open`."
        )))
    };
    act_on(caller, (input.device.as_deref(), input.pane), ask, APPS, cx, done)
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundle_ids_and_links_are_checked_before_anything_is_asked() {
        assert_eq!(super::bundle(" com.apple.Preferences ").ok().as_deref(), Some("com.apple.Preferences"));
        assert!(super::bundle("Settings").is_err());
        assert!(super::bundle("com.apple.Pref erences").is_err());
        assert!(super::installable("relative/My.app").is_err());
        assert!(super::installable("/nowhere/My.app").is_err());
    }
}
