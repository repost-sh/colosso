//! Settings → Agent desktop (`docs/agent-desktop.md` 11): whether agents have a desktop of
//! their own, the two grants its host needs as the host reports them, and what is on it.
//! Nothing here is reachable by `settings_set`: only the person turns it on or grants.

use gpui_kit::App;
use koyo_desk_wire::link::DesktopState;

use super::components::{PillButton, SettingRow, SettingsSection, Status};
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
        State::Off => "Gives agents a desktop of their own to open and control apps, shown live in a \
                       Koyo tab. Your pointer, keyboard and current desktop stay yours. While on, \
                       Koyo adds one desktop in Mission Control, which opens briefly when that \
                       desktop is created or removed."
            .to_owned(),
        State::Starting => "Starting…".to_owned(),
        State::Running => match &desk.desktop {
            Some(DesktopState::Ready { .. }) => {
                "On. It's the last desktop in Mission Control.".to_owned()
            }
            Some(DesktopState::Missing { .. }) | None => {
                "On, but the desktop isn't ready yet. See what's missing below.".to_owned()
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

/// A grant's row: what it's for, and whether it's held.
fn grant(name: &'static str, held: bool, why: &'static str) -> SettingRow {
    let status = if held { Status::met("Allowed") } else { Status::missing("Not allowed") };
    SettingRow::new(name).description(why).control(status)
}

fn access(grants: Grants) -> SettingsSection {
    let mut section = SettingsSection::new("Access").rows([
        grant(
            "Accessibility",
            grants.accessibility,
            "Lets Koyo create the agent desktop and control the apps on it.",
        ),
        grant(
            "Screen Recording",
            grants.screen_recording,
            "Lets Koyo show the agent desktop in its tab and take screenshots for agents.",
        ),
    ]);
    if !(grants.accessibility && grants.screen_recording) {
        section = section.row(
            SettingRow::new("Set up")
                .description("Walks you through both permissions, then starts the desktop.")
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
        Some(DesktopState::Ready { size, .. }) => {
            format!("Ready. Same size as your display ({:.0} × {:.0}).", size.width, size.height)
        }
        Some(DesktopState::Missing { why }) => format!("Not ready: {why}."),
        None => "Creating…".to_owned(),
    };
    let windows = &desk.windows;
    let mut apps: Vec<&str> = windows.iter().map(|window| window.app.as_str()).collect();
    apps.sort_unstable();
    apps.dedup();
    let open = if apps.is_empty() {
        "No apps open.".to_owned()
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
