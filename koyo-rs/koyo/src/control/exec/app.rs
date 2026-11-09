//! The window: its status, bringing it forward, its panels and layout, and the settings
//! pages it shows.

use gpui_kit::App;
use koyo_app_control::ControlError;
use koyo_app_control::ops::app::{
    AppStatusOutput, LayoutSetInput, LayoutState, McpStatus, RightPanel, RightPanelChoice,
    Selected, SettingsPage,
};
use koyo_app_control::ops::shared::Layout;
use serde_json::Value;

use super::{main_shell, output, try_update_shell, update_shell};
use crate::navigation::Route;
use crate::right_panel::View;
use crate::settings;
use crate::settings_view::SettingsPage as Page;
use crate::shell::Shell;

/// What the window shows, as `app_status` answers it.
pub(crate) fn status(cx: &mut App) -> AppStatusOutput {
    let layout: Layout = settings::get(cx).layout.into();
    let closed = AppStatusOutput {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        window_open: false,
        focused: false,
        route: "workspace".to_owned(),
        layout,
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
            route: route(shell.route(cx)),
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

/// Brings the window forward, opening it when it's closed.
pub(crate) fn focus(cx: &mut App) -> Result<Value, ControlError> {
    if main_shell(cx).is_none() {
        crate::open_main_window(cx).map_err(|error| {
            ControlError::internal(format!("Koyo couldn't open its window: {error:#}"))
        })?;
    }
    cx.activate(true);
    update_shell(cx, |_, window, _| window.activate_window())?;
    status_value(cx)
}

/// Opens or closes the panels and switches the layout, as `input` says.
pub(crate) fn layout_set(input: &LayoutSetInput, cx: &mut App) -> Result<Value, ControlError> {
    if let Some(layout) = input.layout {
        settings::update(cx, |settings| settings.layout = layout.into());
    }
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
            layout: settings::get(cx).layout.into(),
            sidebar_open: shell.sidebar_open(),
            right_panel: right_panel(shell),
            bottom_panel_open: shell.bottom_panel_open(),
        }
    })?;
    output(&state)
}

/// Shows Settings on `page`, or on the page shown last.
pub(crate) fn settings_open(
    page: Option<SettingsPage>,
    cx: &mut App,
) -> Result<Value, ControlError> {
    let page = match page {
        None => None,
        Some(SettingsPage::General) => Some(Page::General),
        Some(SettingsPage::Chat) => Some(Page::Chat),
        Some(SettingsPage::Appearance) => Some(Page::Appearance),
        Some(SettingsPage::Editor) => Some(Page::Editor),
        Some(SettingsPage::Languages) => Some(Page::Languages),
        Some(SettingsPage::Browser) => Some(Page::Browser),
        Some(SettingsPage::Keyboard) => Some(Page::KeyboardShortcuts),
        Some(SettingsPage::About) => Some(Page::About),
        Some(SettingsPage::Mcp) => Some(Page::McpServer),
        Some(SettingsPage::AgentDesktop) => Some(Page::AgentDesktop),
    };
    try_update_shell(cx, |shell, _, cx| {
        shell.open_settings_page(page, cx);
        Ok(())
    })?;
    status_value(cx)
}

/// Leaves Settings for the workspace.
pub(crate) fn settings_close(cx: &mut App) -> Result<Value, ControlError> {
    update_shell(cx, |shell, _, cx| shell.navigate(Route::Workspace, cx))?;
    status_value(cx)
}

fn right_panel(shell: &Shell) -> Option<RightPanel> {
    shell.right_panel.shown_view().map(|view| match view {
        View::Changes => RightPanel::Changes,
        View::Files => RightPanel::Files,
    })
}

fn route(route: Route) -> String {
    match route {
        Route::Workspace => "workspace".to_owned(),
        Route::Settings(page) => format!(
            "settings:{}",
            match page {
                Page::General => "general",
                Page::Chat => "chat",
                Page::Editor => "editor",
                Page::Languages => "languages",
                Page::Appearance => "appearance",
                Page::Browser => "browser",
                Page::KeyboardShortcuts => "keyboard",
                Page::McpServer => "mcp",
                Page::AgentDesktop => "agent_desktop",
                Page::About => "about",
            }
        ),
    }
}
