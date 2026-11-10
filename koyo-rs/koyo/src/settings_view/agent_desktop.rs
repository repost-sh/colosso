//! Settings → Agent desktop (`docs/agent-desktop.md` 11): whether agents have a desktop of
//! their own, the two grants its host needs as the host reports them, and what is on it.
//! Nothing here is reachable by `settings_set`: only the person turns it on or grants.

use gpui_kit::App;
use koyo_desk_wire::link::DesktopState;

use super::components::{PillButton, SettingRow, SettingsSection};
use super::native_switch::NativeSwitch;
use crate::agent_desktop::{self, Desk, Grants, State};

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    let desk = agent_desktop::desk(cx);
    let desk = desk.read(cx);
    let mut sections = vec![on_off(desk)];
    if let (State::Running, Some(grants)) = (&desk.state, desk.grants) {
        sections.push(access(grants));
        sections.push(desktop(desk));
    }
    sections
}

fn on_off(desk: &Desk) -> SettingsSection {
    let stands = match &desk.state {
        State::Off => "Agents open and drive apps on a desktop of their own, shown live in one \
                       Koyo tab, while you keep working on yours: your pointer, keyboard and the \
                       desktop you're on stay yours. On, Koyo keeps one more desktop in Mission \
                       Control while it runs, and Mission Control shows for a moment as Koyo \
                       makes it and removes it."
            .to_owned(),
        State::Starting => "Starting…".to_owned(),
        State::Running => match &desk.desktop {
            Some(DesktopState::Ready { .. }) => {
                "On. The agent desktop is the last one in Mission Control.".to_owned()
            }
            Some(DesktopState::Missing { .. }) | None => {
                "On, with no desktop made yet: see below what is in the way.".to_owned()
            }
        },
        State::Failed(why) => why.clone(),
    };
    let mut row = SettingRow::new("Agent desktop")
        .description(stands)
        .control(NativeSwitch::new(
            "agent-desktop-on",
            |settings| settings.agent_desktop,
            |settings, on| settings.agent_desktop = on,
        ));
    if matches!(desk.state, State::Failed(_)) {
        row = row.control(
            PillButton::new("agent-desktop-start", "Start again").on_click(|_, _, cx| {
                agent_desktop::desk(cx).update(cx, Desk::start);
            }),
        );
    }
    SettingsSection::new("Agent desktop").row(row)
}

/// A grant's row: how it stands.
fn grant(name: &'static str, held: bool, why: &'static str) -> SettingRow {
    SettingRow::new(name).description(if held {
        "Allowed.".to_owned()
    } else {
        format!("Not allowed yet. {why}")
    })
}

fn access(grants: Grants) -> SettingsSection {
    let mut section = SettingsSection::new("Access").rows([
        grant(
            "Accessibility",
            grants.accessibility,
            "Koyo Agent Desktop needs it to make the agent desktop and to act in the apps on it.",
        ),
        grant(
            "Screen Recording",
            grants.screen_recording,
            "Koyo Agent Desktop needs it to show the agent desktop in its tab and to take \
             screenshots for agents.",
        ),
    ]);
    if !(grants.accessibility && grants.screen_recording) {
        section = section.row(
            SettingRow::new("Set up")
                .description("Takes you through both, one at a time, and starts the desktop.")
                .control(
                    PillButton::new("agent-desktop-setup", "Set Up…")
                        .on_click(|_, window, cx| agent_desktop::setup::open(window, cx)),
                ),
        );
    }
    section
}

fn desktop(desk: &Desk) -> SettingsSection {
    let stands = match &desk.desktop {
        // Its size is the display's, said so that it isn't taken for something Koyo set.
        Some(DesktopState::Ready { size, .. }) => format!(
            "Made. It is as large as your display ({:.0} × {:.0}), which Koyo leaves as it is.",
            size.width, size.height
        ),
        Some(DesktopState::Missing { why }) => format!("Not there: {why}."),
        None => "Being made…".to_owned(),
    };
    let windows = &desk.windows;
    let mut apps: Vec<&str> = windows.iter().map(|window| window.app.as_str()).collect();
    apps.sort_unstable();
    apps.dedup();
    let open = if apps.is_empty() {
        "No app is open on it.".to_owned()
    } else {
        format!(
            "{} with {} {}.",
            apps.join(", "),
            windows.len(),
            if windows.len() == 1 {
                "window"
            } else {
                "windows"
            }
        )
    };
    SettingsSection::new("The desktop").rows([
        SettingRow::new("Desktop").description(stands),
        SettingRow::new("Apps").description(open),
    ])
}
