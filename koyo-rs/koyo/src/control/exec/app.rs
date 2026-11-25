//! The window: its status, bringing it forward, its panels, and the Settings tab and the page
//! it shows.

use futures::channel::oneshot;
use gpui_kit::App;
use koyo_app_control::ControlError;
use koyo_app_control::ops::app::{
    AppStatusOutput, LayoutSetInput, LayoutState, McpStatus, RightPanel, RightPanelChoice,
    Selected, SettingsPage,
};
use serde_json::Value;

use super::{Exec, main_shell, output, try_update_shell, update_shell};
use crate::model::SettingsTab;
use crate::navigation::SettingsPage as Page;
use crate::right_panel::View;
use crate::settings;
use crate::shell::Shell;

/// What the window shows, as `app_status` answers it.
pub(crate) fn status(cx: &mut App) -> AppStatusOutput {
    let closed = AppStatusOutput {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        window_open: false,
        focused: false,
        on_screen: false,
        route: "workspace".to_owned(),
        sidebar_open: false,
        right_panel: None,
        bottom_panel_open: false,
        selected: Selected::default(),
        mcp: {
            let http = super::super::http_state(cx);
            McpStatus {
                http: http.url.is_some(),
                url: http.url,
            }
        },
        data_dir: (settings::data_dir())
            .map(|dir| dir.display().to_string())
            .unwrap_or_default(),
    };
    let Some((window, shell)) = main_shell(cx) else {
        return closed;
    };
    let open = window.update(cx, |_, window, cx| {
        let shell = shell.read(cx);
        let workspace = &shell.workspace;
        AppStatusOutput {
            window_open: true,
            focused: window.is_window_active(),
            on_screen: window.is_visible(),
            route: route(shell.workspace.settings()),
            sidebar_open: shell.sidebar_open(),
            right_panel: right_panel(shell),
            bottom_panel_open: shell.bottom_panel_open(),
            selected: Selected {
                project: workspace
                    .project()
                    .map(|project| project.path.display().to_string()),
                worktree: (workspace.branch()).map(|unit| unit.worktree.path.display().to_string()),
                pane: shell.focused_pane(window, cx),
            },
            ..closed.clone()
        }
    });
    open.unwrap_or(closed)
}

/// [`status`] as JSON.
pub(crate) fn status_value(cx: &mut App) -> Result<Value, ControlError> {
    output(&status(cx))
}

/// Brings the window forward, opening it first when it's closed: then the answer comes once
/// its file is read and it's open, as it stands then.
pub(crate) fn focus(cx: &mut App) -> Exec {
    if main_shell(cx).is_some() {
        return Exec::Ready(focused(cx));
    }
    let (answer, answered) = oneshot::channel();
    crate::reopen_main_window(cx, move |opened, cx| {
        let focused = match opened {
            Ok(()) => focused(cx),
            Err(error) => Err(ControlError::internal(format!(
                "Koyo couldn't open its window: {error:#}"
            ))),
        };
        let _ = answer.send(focused);
    });
    Exec::Later(cx.spawn(async move |_| {
        answered.await.unwrap_or_else(|_| {
            Err(ControlError::internal("Koyo didn't open its window."))
        })
    }))
}

/// Brings the open window forward, and says how it stands.
fn focused(cx: &mut App) -> Result<Value, ControlError> {
    cx.activate(true);
    update_shell(cx, |_, window, _| window.activate_window())?;
    status_value(cx)
}

/// Opens or closes the panels, as `input` says.
pub(crate) fn layout_set(input: &LayoutSetInput, cx: &mut App) -> Result<Value, ControlError> {
    let state = update_shell(cx, |shell, _, cx| {
        if let Some(open) = input.sidebar {
            shell.set_sidebar_open(open, cx);
        }
        if let Some(open) = input.bottom_panel {
            shell.set_bottom_panel_open(open, cx);
        }
        match input.right_panel {
            Some(RightPanelChoice::Changes) => shell.show(View::Changes, cx),
            Some(RightPanelChoice::Files) => shell.show(View::Files, cx),
            Some(RightPanelChoice::None) => shell.close_right_panel(cx),
            None => {}
        }
        LayoutState {
            sidebar_open: shell.sidebar_open(),
            right_panel: right_panel(shell),
            bottom_panel_open: shell.bottom_panel_open(),
        }
    })?;
    output(&state)
}

/// Opens the Settings tab on `page`, or picks it again if it's open: on the page it shows, or
/// the last one showed, with none asked for.
pub(crate) fn settings_open(
    page: Option<SettingsPage>,
    cx: &mut App,
) -> Result<Value, ControlError> {
    let page = match page {
        None => None,
        Some(SettingsPage::General) => Some(Page::General),
        Some(SettingsPage::Chat) => Some(Page::Chat),
        Some(SettingsPage::Accounts) => Some(Page::Accounts),
        Some(SettingsPage::Appearance) => Some(Page::Appearance),
        Some(SettingsPage::Editor) => Some(Page::Editor),
        Some(SettingsPage::Languages) => Some(Page::Languages),
        Some(SettingsPage::Browser) => Some(Page::Browser),
        Some(SettingsPage::Keyboard) => Some(Page::KeyboardShortcuts),
        Some(SettingsPage::About) => Some(Page::About),
        Some(SettingsPage::Mcp) => Some(Page::McpServer),
        Some(SettingsPage::AgentDesktop) => Some(Page::AgentDesktop),
        Some(SettingsPage::Devices) => Some(Page::Devices),
    };
    try_update_shell(cx, |shell, _, cx| {
        shell.open_settings_page(page, cx);
        Ok(())
    })?;
    status_value(cx)
}

/// Closes the Settings tab: the tab it was shown over shows again.
pub(crate) fn settings_close(cx: &mut App) -> Result<Value, ControlError> {
    update_shell(cx, |shell, _, cx| shell.close_settings(cx))?;
    status_value(cx)
}

fn right_panel(shell: &Shell) -> Option<RightPanel> {
    shell.right_panel.shown_view().map(|view| match view {
        View::Changes => RightPanel::Changes,
        View::Files => RightPanel::Files,
    })
}

/// What `app_status` says the window shows: the Settings tab's page while it's open, as
/// `settings:<page>`, and `workspace` otherwise.
fn route(settings: Option<&SettingsTab>) -> String {
    match settings.map(SettingsTab::page) {
        None => "workspace".to_owned(),
        Some(page) => format!(
            "settings:{}",
            match page {
                Page::General => "general",
                Page::Chat => "chat",
                Page::Accounts => "accounts",
                Page::Editor => "editor",
                Page::Languages => "languages",
                Page::Appearance => "appearance",
                Page::Browser => "browser",
                Page::KeyboardShortcuts => "keyboard",
                Page::McpServer => "mcp",
                Page::AgentDesktop => "agent_desktop",
                Page::Devices => "devices",
                Page::About => "about",
            }
        ),
    }
}
