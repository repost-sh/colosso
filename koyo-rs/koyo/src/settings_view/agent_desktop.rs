//! Settings → Agent desktop (`docs/agent-desktop.md` 11): whether the Mac is set up for agent
//! desktops and how it stands, one action at a time: Set up…, Start session, End session,
//! Remove…. Nothing here is reachable by `settings_set`: an agent can't set up, remove or widen
//! any of it.

use gpui_kit::App;
use koyo_desk_wire::helper::HelperStatus;
use koyo_desk_wire::holder::SessionState;

use super::components::{PillButton, SettingRow, SettingsSection};
use crate::agent_desktop::{self, Busy, Desk};

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    let desk = agent_desktop::get(cx);
    let mut sections = vec![setup(desk)];
    if let Some(Ok(status)) = &desk.helper {
        sections.push(this_mac(status));
        sections.push(session(desk));
        sections.push(removal(desk));
    }
    if let Some(last) = &desk.last {
        let mut said = last.message.clone();
        if !last.steps.is_empty() {
            said.push_str("\n\n");
            said.push_str(&last.steps.join("\n"));
        }
        sections.push(SettingsSection::new("Last action").row(
            SettingRow::new(if last.ok { "Done" } else { "Didn't finish" }).description(said),
        ));
    }
    sections
}

/// An action's button: while anything is under way, it's off and says what is.
fn action(
    id: &'static str,
    label: &'static str,
    busy: Busy,
    on_click: impl Fn(&mut gpui_kit::Window, &mut App) + 'static,
) -> PillButton {
    let label = match busy {
        Busy::Idle => label,
        Busy::SettingUp => "Setting up…",
        Busy::Removing => "Removing…",
        Busy::Starting => "Starting…",
        Busy::Ending => "Ending…",
    };
    PillButton::new(id, label)
        .disabled(busy != Busy::Idle)
        .on_click(move |_, window, cx| on_click(window, cx))
}

fn setup(desk: &Desk) -> SettingsSection {
    let busy = desk.busy != Busy::Idle;
    let check = PillButton::new("agent-desktop-check", "Check again")
        .disabled(busy)
        .on_click(|_, _, cx| agent_desktop::refresh(cx));
    let row = match &desk.helper {
        Some(Ok(status)) => SettingRow::new("Set up").description(format!(
            "Agents work in the hidden account {} (uid {}), helper {}.",
            status.account, status.uid, status.helper_version
        )),
        Some(Err(_)) | None => SettingRow::new("Not set up")
            .description(
                "Agents get desktops of their own: a hidden account on this Mac, which Koyo makes \
                 with one administrator approval, with Screen Sharing for that account alone and \
                 only on this Mac (a firewall rule keeps other computers out, so this Mac's \
                 Screen Sharing isn't reachable from them while it's set up). Nothing an agent \
                 does reaches your pointer, keyboard, screens, windows or files.",
            )
            .control(action(
                "agent-desktop-set-up",
                "Set up…",
                desk.busy,
                |_, cx| {
                    agent_desktop::set_up(cx);
                },
            )),
    };
    SettingsSection::new("Agent desktop").row(row.control(check))
}

fn this_mac(status: &HelperStatus) -> SettingsSection {
    let macos = status
        .refusal
        .clone()
        .unwrap_or_else(|| format!("macOS {}", status.macos));
    SettingsSection::new("This Mac").rows([
        SettingRow::new("macOS").description(macos),
        SettingRow::new("Screen Sharing").description(if status.screen_sharing_on {
            "On, for the agent account's session."
        } else {
            "Off. It's on only while an agent session runs."
        }),
        SettingRow::new("Firewall rule").description(if status.anchor_loaded {
            "Loaded: Screen Sharing answers on this Mac alone."
        } else {
            "Not loaded. The helper loads it before Screen Sharing is ever on."
        }),
        SettingRow::new("Host").description(status.host_version.as_ref().map_or_else(
            || "Not installed yet.".to_owned(),
            |version| format!("Koyo Agent Desktop {version}"),
        )),
    ])
}

fn session(desk: &Desk) -> SettingsSection {
    let (said, held): (String, bool) = match desk
        .holder
        .as_ref()
        .map(|holder| (&holder.state, holder.screen))
    {
        None => ("No agent session.".into(), false),
        Some((SessionState::Starting, _)) => ("Logging the agent account in…".into(), false),
        Some((SessionState::Held { .. }, screen)) => (
            screen.map_or_else(
                || "Held.".to_owned(),
                |size| format!("Held; the agent screen is {}×{}.", size.width, size.height),
            ),
            true,
        ),
        Some((SessionState::Failed { message }, _)) => (format!("Failed: {message}"), false),
        Some((SessionState::Ended, _)) => ("Ended.".into(), false),
    };
    let control = if held {
        action("agent-desktop-end", "End session", desk.busy, |_, cx| {
            agent_desktop::end_session(cx);
        })
    } else {
        action(
            "agent-desktop-start",
            "Start session",
            desk.busy,
            |_, cx| {
                agent_desktop::start_session(cx);
            },
        )
    };
    SettingsSection::new("Session").row(
        SettingRow::new("Agent session")
            .description(format!(
                "{said} It ends when Koyo quits, when you restart, shut down or log out, and when \
                 nobody has used it for a while."
            ))
            .control(control),
    )
}

fn removal(desk: &Desk) -> SettingsSection {
    SettingsSection::new("Remove").row(
        SettingRow::new("Remove the agent desktop")
            .description(
                "The agent account and everything in its home, its Screen Sharing access, the \
                 firewall rule and the helper go. macOS asks for an administrator's own password \
                 to delete an account.",
            )
            .control(action(
                "agent-desktop-remove",
                "Remove…",
                desk.busy,
                |window, cx| {
                    super::agent_desktop_remove::open(window, cx);
                },
            )),
    )
}
