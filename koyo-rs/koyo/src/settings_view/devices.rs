//! Settings → Devices (`docs/devices.md` 10): what Koyo's simulators need of the Mac, each
//! marked there or missing; Koyo's own simulators with what runs; the iPhones and iPads plugged
//! in, ready or with the step each misses; and how many simulators may run at once. Nothing
//! here is reachable by `settings_set`: only the person changes it.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, ElementId, SharedString};

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection, Status};
use crate::devices::needs::{Found, Xcode};
use crate::devices::{self, Devices, Need, Simulator};
use crate::modal::{self, Confirm, Tone};
use crate::settings;

/// How many simulators may run at once, from the menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetSimulatorsAtOnce(pub u8);

/// A new simulator of this model, from the menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct NewSimulator(pub String);

pub fn init(cx: &mut App) {
    cx.on_action(|SetSimulatorsAtOnce(count): &SetSimulatorsAtOnce, cx| {
        settings::update(cx, |settings| settings.simulators_at_once = *count);
    });
    cx.on_action(|NewSimulator(model): &NewSimulator, cx| {
        devices::devices(cx).update(cx, |devices, cx| devices.make_new(model.clone(), cx));
    });
}

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    let devices = devices::devices(cx);
    let devices = devices.read(cx);
    let running = devices.running_now(cx);
    vec![needs(devices), simulators(devices, &running, cx), plugged(devices), at_once(&running, cx)]
}

fn id(what: &str, udid: &str) -> ElementId {
    ElementId::Name(SharedString::from(format!("device-{what}-{udid}")))
}

fn set_up() -> PillButton {
    PillButton::new("devices-set-up", "Set Up…").on_click(|_, window, cx| devices::setup::open(window, cx))
}

/// Xcode and the iOS versions simulators run: each says whether it's there.
fn needs(devices: &Devices) -> SettingsSection {
    let section = SettingsSection::new("Requirements");
    let Some(found) = &devices.found else {
        return section.row(SettingRow::new("Xcode").description("Checking…"));
    };
    let (said, status) = match &found.xcode {
        Xcode::Ready(version) if found.components => (format!("Version {version}"), Status::met("Installed")),
        Xcode::Ready(version) => (format!("Version {version}. {}", found.sentence(Need::Components)), Status::missing("Setup unfinished")),
        Xcode::Older(_) => (found.sentence(Need::Xcode), Status::missing("Update needed")),
        Xcode::Other { .. } => (found.sentence(Need::Xcode), Status::missing("Not selected")),
        Xcode::Missing => (found.sentence(Need::Xcode), Status::missing("Not installed")),
    };
    let xcode = SettingRow::new("Xcode").description(said).control(status);
    if matches!(found.need(), Some(Need::Xcode | Need::Components)) {
        return section.row(xcode.control(set_up()));
    }
    section.row(xcode).row(systems(found))
}

fn systems(found: &Found) -> SettingRow {
    let row = SettingRow::new("iOS versions");
    if found.systems.is_empty() {
        return row
            .description("None installed. Download one in Xcode → Settings → Components (about 8 GB).")
            .control(Status::missing("Not installed"))
            .control(set_up());
    }
    let listed: Vec<String> = (found.systems.iter())
        .map(|system| format!("{} ({})", system.name, crate::viewer::size_on_disk(system.bytes)))
        .collect();
    row.description(format!("{}. Add or remove versions in Xcode → Settings → Components.", listed.join(", ")))
        .control(Status::met("Installed"))
}

/// Koyo's own simulators, each with Stop while it runs and Delete while it doesn't, and a new one.
fn simulators(devices: &Devices, running: &[(String, String)], cx: &App) -> SettingsSection {
    let section = SettingsSection::new("Koyo's simulators");
    let Some(Ok(listed)) = &devices.simulators else {
        let said = match &devices.simulators {
            Some(Err(why)) => why.clone(),
            _ => "Checking…".to_owned(),
        };
        return section.row(SettingRow::new("Simulators").description(said));
    };
    let rows = listed.iter().map(|simulator| {
        let stopping = devices.existing(&simulator.udid).is_some_and(|link| link.read(cx).stopping());
        let runs = running.iter().any(|(udid, _)| *udid == simulator.udid);
        simulator_row(simulator, runs, stopping)
    });
    section.rows(rows).row(new_one(devices))
}

fn simulator_row(simulator: &Simulator, runs: bool, stopping: bool) -> SettingRow {
    let udid = simulator.udid.clone();
    let stands = match (stopping, runs) {
        (true, _) => " · Stopping…",
        (false, true) => " · Running",
        (false, false) => "",
    };
    let row = SettingRow::new(simulator.name.clone()).description(format!("{}{stands}", simulator.system));
    if stopping {
        return row;
    }
    if runs {
        return row.control(PillButton::new(id("stop", &udid), "Stop").on_click(move |_, _, cx| {
            devices::devices(cx).update(cx, |devices, cx| devices.stop(&udid, cx));
        }));
    }
    let name = simulator.name.clone();
    row.control(PillButton::new(id("delete", &udid), "Delete").on_click(move |_, window, cx| {
        let udid = udid.clone();
        let question = Confirm {
            title: format!("Delete {name}?"),
            text: "This deletes the simulator and everything installed on it. Xcode's own simulators \
                   aren't affected."
                .into(),
            yes: "Delete",
            tone: Tone::Danger,
        };
        let delete = move |cx: &mut App| devices::devices(cx).update(cx, |devices, cx| devices.delete(&udid, cx));
        modal::confirm(question, delete, window, cx);
    }))
}

/// The iPhones and iPads plugged in, each ready or with the step it misses and its setup.
fn plugged(devices: &Devices) -> SettingsSection {
    let section = SettingsSection::new("iPhones and iPads");
    let set_up = |udid: Option<String>| {
        let id = id("set-up", udid.as_deref().unwrap_or("any"));
        PillButton::new(id, "Set Up…").on_click(move |_, window, cx| devices::setup::device::open(udid.clone(), window, cx))
    };
    let listed = match &devices.connected {
        Some(Ok(listed)) if !listed.is_empty() => listed,
        Some(Ok(_)) => {
            let none = "Connect an iPhone or iPad running iOS or iPadOS 27 with a cable to use it in a Device tab.";
            return section.row(SettingRow::new("No device connected").description(none).control(set_up(None)));
        }
        Some(Err(why)) => return section.row(SettingRow::new("Connected devices").description(why.clone())),
        None => return section.row(SettingRow::new("Connected devices").description("Checking…")),
    };
    section.rows(listed.iter().map(|device| {
        let row = SettingRow::new(device.name.clone());
        match &device.missing {
            Some((code, said)) => {
                let needs = Status::missing(koyo_device_wire::real::label(*code));
                row.description(format!("{}. {said}", device.system)).control(needs).control(set_up(Some(device.udid.clone())))
            }
            None => row.description(device.system.clone()).control(Status::met("Ready")),
        }
    }))
}

/// A new simulator, of a model the newest system has.
fn new_one(devices: &Devices) -> SettingRow {
    let said = devices.said.clone().unwrap_or_else(|| {
        "Uses the newest iOS version installed. Each simulator takes about 2 GB of disk space.".into()
    });
    let row = SettingRow::new("New simulator").description(said);
    if devices.found.as_ref().is_none_or(|found| found.models.is_empty()) {
        return row;
    }
    row.control(MenuButton::new("device-new", "New…", |cx| {
        let devices = devices::devices(cx);
        let models = devices.read(cx).found.as_ref().map_or(&[][..], |found| found.models.as_slice());
        devices::setup::models_menu(models, None, |name| Box::new(NewSimulator(name)))
    }))
}

/// How many may run at once, and what runs now.
fn at_once(running: &[(String, String)], cx: &App) -> SettingsSection {
    let most = devices::at_once(cx);
    let running: Vec<&str> = running.iter().map(|(_, name)| name.as_str()).collect();
    let now = if running.is_empty() { "None are running.".to_owned() } else { format!("Running now: {}.", running.join(", ")) };
    let row = SettingRow::new("Simulators running at once")
        .description(format!(
            "The most simulators that can run at the same time, since each one uses a lot of memory. \
             Agents can't go over the limit, and Koyo asks before you do. {now}"
        ))
        .control(MenuButton::new("simulators-at-once", most.to_string(), |cx| {
            let most = settings::get(cx).simulators_at_once();
            (1..=settings::MOST_SIMULATORS_AT_ONCE).fold(NativeMenu::new(), |menu, count| {
                menu.menu_with_check(count.to_string(), count == most, Box::new(SetSimulatorsAtOnce(count)))
            })
        }));
    SettingsSection::new("Running").row(row)
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use crate::devices::needs::Xcode;
    use crate::devices::needs::fixtures::{booted, found, ready, simulator};

    #[gpui_kit::test]
    fn an_xcode_too_old_is_said_and_no_ios_version_is_listed(cx: &mut TestAppContext) {
        cx.update(crate::init);
        cx.update(|cx| {
            let old = found(Xcode::Older("26.4".into()), false, false);
            crate::devices::devices(cx).update(cx, |devices, cx| devices.listed(Ok(Vec::new()), Some(old), cx));
        });
        let sections = cx.read(super::sections);
        let titled = |title: &str| sections.iter().any(|section| section.matches(title));
        assert!(titled("requirements"));
        assert!(titled("you have xcode 26.4"));
        assert!(!titled("ios versions"), "not until Xcode will do");
    }

    #[gpui_kit::test]
    fn the_page_shows_what_runs_with_stop_and_the_limit(cx: &mut TestAppContext) {
        cx.update(crate::init);
        cx.update(|cx| {
            let devices = crate::devices::devices(cx);
            devices.update(cx, |devices, cx| devices.listed(Ok(vec![booted("U-RUNS"), simulator("U-OFF")]), Some(found(ready(), true, true)), cx));
        });
        let sections = cx.read(super::sections);
        let titled = |title: &str| sections.iter().find(|section| section.matches(title)).is_some();
        assert!(titled("version 27.2"));
        assert!(titled("ios 27.2 (8.3 gb)"), "the system and its size");
        assert!(titled("ios 27.2 · running"), "the one that runs");
        assert!(titled("running now: iphone 18 pro."));
    }
}
