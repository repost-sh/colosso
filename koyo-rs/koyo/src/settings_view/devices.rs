//! Settings → Devices (`docs/devices.md` 10): what Koyo's simulators need of the Mac and what
//! it has, Koyo's own simulators with what runs, the iPhones and iPads plugged in with the step
//! each misses, how many simulators may run at once, and in words what Koyo doesn't touch. Nothing here is reachable by `settings_set`: only the person changes it.

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, ElementId, SharedString};

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection};
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
    vec![needs(devices), simulators(devices, &running, cx), plugged(devices), at_once(&running, cx), untouched()]
}

fn id(what: &str, udid: &str) -> ElementId {
    ElementId::Name(SharedString::from(format!("device-{what}-{udid}")))
}

fn set_up() -> PillButton {
    PillButton::new("devices-set-up", "Set Up…").on_click(|_, window, cx| devices::setup::open(window, cx))
}

/// Xcode and the systems: what Koyo's simulators need of the Mac.
fn needs(devices: &Devices) -> SettingsSection {
    let section = SettingsSection::new("What simulators need");
    let Some(found) = &devices.found else {
        return section.row(SettingRow::new("Xcode").description("Looking at what this Mac has…"));
    };
    let missing = matches!(found.need(), Some(Need::Xcode | Need::Components));
    let xcode = SettingRow::new("Xcode").description(match (&found.xcode, missing) {
        (Xcode::Ready(version), true) => format!("Xcode {version}, the one in use. {}", found.sentence(Need::Components)),
        (Xcode::Ready(version), false) => format!("Xcode {version}, the one in use, set up."),
        _ => found.sentence(Need::Xcode),
    });
    if missing {
        return section.row(xcode.control(set_up()));
    }
    section.row(xcode).row(systems(found))
}

fn systems(found: &Found) -> SettingRow {
    let row = SettingRow::new("iOS systems");
    if found.systems.is_empty() {
        return row
            .description(
                "None installed. Xcode downloads one, about 8 GB: in Xcode, choose Settings → \
                 Components.",
            )
            .control(set_up());
    }
    let listed: Vec<String> = (found.systems.iter())
        .map(|system| format!("{} ({})", system.name, crate::viewer::size_on_disk(system.bytes)))
        .collect();
    row.description(format!(
        "{}. They're Xcode's too: add or remove one in Xcode, under Settings → Components.",
        listed.join(", ")
    ))
}

/// Koyo's own simulators, each with Stop while it runs and Delete while it doesn't, and a new one.
fn simulators(devices: &Devices, running: &[(String, String)], cx: &App) -> SettingsSection {
    let section = SettingsSection::new("Koyo's simulators");
    let Some(Ok(listed)) = &devices.simulators else {
        let said = match &devices.simulators {
            Some(Err(why)) => why.clone(),
            _ => "Looking for Koyo's simulators…".to_owned(),
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
        (true, _) => ". Stopping…",
        (false, true) => ". Running.",
        (false, false) => ".",
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
            text: "Its apps and everything on it go with it, from Koyo's own set of simulators. \
                   Xcode's simulators stay as they are."
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
            let none = "None plugged in. Plug an iPhone or iPad on iOS or iPadOS 27 into this Mac with a \
                        cable to use it in a Device tab.";
            return section.row(SettingRow::new("Plugged in").description(none).control(set_up(None)));
        }
        Some(Err(why)) => return section.row(SettingRow::new("Plugged in").description(why.clone())),
        None => return section.row(SettingRow::new("Plugged in").description("Looking for iPhones and iPads plugged in…")),
    };
    section.rows(listed.iter().map(|device| {
        let row = SettingRow::new(device.name.clone());
        match &device.missing {
            Some((_, said)) => row.description(format!("{}. {said}", device.system)).control(set_up(Some(device.udid.clone()))),
            None => row.description(format!("{}. Ready to use in a Device tab.", device.system)),
        }
    }))
}

/// A new simulator, of a model the newest system has.
fn new_one(devices: &Devices) -> SettingRow {
    let said = devices.said.clone().unwrap_or_else(|| {
        "Made on the newest iOS installed, in Koyo's own set: about 2 GB of disk each.".into()
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
    let now = match running.len() {
        0 => "None runs now.".to_owned(),
        count => format!("{count} {} now: {}.", if count == 1 { "runs" } else { "run" }, running.join(", ")),
    };
    let row = SettingRow::new("Simulators running at once")
        .description(format!(
            "At most this many run at once. An agent asking for one more is refused; when you \
             start one more, Koyo asks first. Each takes a share of this Mac's memory. {now}"
        ))
        .control(MenuButton::new("simulators-at-once", most.to_string(), |cx| {
            let most = settings::get(cx).simulators_at_once();
            (1..=settings::MOST_SIMULATORS_AT_ONCE).fold(NativeMenu::new(), |menu, count| {
                menu.menu_with_check(count.to_string(), count == most, Box::new(SetSimulatorsAtOnce(count)))
            })
        }));
    SettingsSection::new("Running").row(row)
}

fn untouched() -> SettingsSection {
    SettingsSection::new("What Koyo doesn't touch")
        .row(SettingRow::new("Xcode's simulators").description(
            "Koyo keeps its simulators apart from Xcode's and never changes yours. It deletes a \
             simulator only when you press Delete here, and never an iOS system: those are Xcode's.",
        ))
        .row(SettingRow::new("Your iPhone or iPad").description(
            "Koyo changes no setting on it: it keeps it awake only while its tab is open, and its \
             camera and microphone are off meanwhile.",
        ))
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use crate::devices::needs::fixtures::{booted, found, ready, simulator};

    #[gpui_kit::test]
    fn the_page_shows_what_runs_with_stop_and_the_limit(cx: &mut TestAppContext) {
        cx.update(crate::init);
        cx.update(|cx| {
            let devices = crate::devices::devices(cx);
            devices.update(cx, |devices, cx| devices.listed(Ok(vec![booted("U-RUNS"), simulator("U-OFF")]), Some(found(ready(), true, true)), cx));
        });
        let sections = cx.read(super::sections);
        let titled = |title: &str| sections.iter().find(|section| section.matches(title)).is_some();
        assert!(titled("xcode 27.2, the one in use, set up"));
        assert!(titled("ios 27.2 (8.3 gb)"), "the system and its size");
        assert!(titled("ios 27.2. running."), "the one that runs");
        assert!(titled("1 runs now: iphone 18 pro."));
        assert!(titled("what koyo doesn't touch"));
    }
}
