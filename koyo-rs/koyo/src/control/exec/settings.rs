//! The settings an operation may read and change: an allowlist, camelCase. The projects in
//! the sidebar are the project operations', and the control policy is the person's alone,
//! so neither is here; nor are the themes' gradients or the language server tables.

use std::path::PathBuf;

use koyo_app_control::ControlError;
use koyo_app_control::ops::settings::{
    Appearance, BranchPrefix, Devtools, DevtoolsDock, SaveScope, SettingsSetInput, SettingsValues,
};
use koyo_app_control::ops::shared::Layout;
use gpui_kit::App;
use serde_json::Value;

use super::{info, output};
use crate::browser::{DOCK_MIN, Dock};
use crate::settings::{self, Settings};
use crate::theme::{self, ThemePreference};

/// The allowlisted settings as they stand.
pub(crate) fn values(settings: &Settings) -> SettingsValues {
    SettingsValues {
        appearance: match settings.appearance {
            ThemePreference::System => Appearance::System,
            ThemePreference::Light => Appearance::Light,
            ThemePreference::Dark => Appearance::Dark,
        },
        layout: settings.layout.into(),
        preview_files: settings.preview_files,
        save_scope: match settings.save_scope {
            settings::SaveScope::Shown => SaveScope::Shown,
            settings::SaveScope::All => SaveScope::All,
        },
        inlay_hints: settings.inlay_hints,
        semantic_colouring: settings.semantic_colouring,
        sidebar_open_on_launch: settings.sidebar_open_on_launch,
        worktrees_dir: (settings.worktrees_dir.as_ref()).map(|dir| dir.display().to_string()),
        branch_prefix: match &settings.branch_prefix {
            settings::BranchPrefix::GitHubUser => BranchPrefix::GithubUser,
            settings::BranchPrefix::Custom(prefix) => BranchPrefix::Custom(prefix.clone()),
            settings::BranchPrefix::None => BranchPrefix::None,
        },
        most_language_servers: (settings.most_language_servers)
            .map(|most| u32::try_from(most).unwrap_or(u32::MAX)),
        stop_idle_language_servers: settings.stop_idle_language_servers,
        sidebar_width: info::decimal(settings.sidebar_width()),
        devtools: Devtools {
            dock: match settings.devtools.dock {
                Dock::Bottom => DevtoolsDock::Bottom,
                Dock::Left => DevtoolsDock::Left,
                Dock::Right => DevtoolsDock::Right,
                Dock::Detached => DevtoolsDock::Detached,
            },
            height: info::decimal(settings.devtools.height),
            width: info::decimal(settings.devtools.width),
        },
    }
}

/// The allowlisted settings, as `settings_get` answers them.
pub(crate) fn get(cx: &App) -> Result<Value, ControlError> {
    output(&values(settings::get(cx)))
}

/// A size in pixels from the input, refused outside `min..=max`.
fn pixels(name: &str, value: f64, min: f32, max: f32) -> Result<f32, ControlError> {
    if value.is_finite() && value >= f64::from(min) && value <= f64::from(max) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "checked to be within a few thousand"
        )]
        Ok(value as f32)
    } else {
        Err(ControlError::invalid_input(format!(
            "`{name}` is between {min} and {max} pixels."
        )))
    }
}

/// Refuses the fields of `input` no setting takes: a relative worktrees folder, an empty
/// custom prefix, a limit of no language servers.
fn check(input: &SettingsSetInput) -> Result<(), ControlError> {
    if let Some(Some(dir)) = &input.worktrees_dir
        && !PathBuf::from(dir).is_absolute()
    {
        return Err(ControlError::invalid_input(
            "`worktreesDir` is an absolute path, or null for the default.",
        ));
    }
    if let Some(BranchPrefix::Custom(prefix)) = &input.branch_prefix
        && prefix.trim().is_empty()
    {
        return Err(ControlError::invalid_input(
            "A custom branch prefix can't be empty; pass `\"none\"` for no prefix.",
        ));
    }
    if input.most_language_servers == Some(Some(0)) {
        return Err(ControlError::invalid_input(
            "`mostLanguageServers` is at least 1, or null for no limit.",
        ));
    }
    Ok(())
}

/// Changes the settings `input` names, all or none, and answers the settings after.
pub(crate) fn set(input: SettingsSetInput, cx: &mut App) -> Result<Value, ControlError> {
    // Checked first, so a refused field changes nothing.
    check(&input)?;
    let sidebar_width = (input.sidebar_width)
        .map(|width| {
            pixels(
                "sidebarWidth",
                width,
                theme::SIDEBAR_MIN_WIDTH,
                theme::SIDEBAR_MAX_WIDTH,
            )
        })
        .transpose()?;
    let devtools = input.devtools.unwrap_or_default();
    let height = (devtools.height)
        .map(|height| pixels("devtools.height", height, DOCK_MIN, 4000.))
        .transpose()?;
    let width = (devtools.width)
        .map(|width| pixels("devtools.width", width, DOCK_MIN, 4000.))
        .transpose()?;
    settings::update(cx, |settings| {
        if let Some(appearance) = input.appearance {
            settings.appearance = match appearance {
                Appearance::System => ThemePreference::System,
                Appearance::Light => ThemePreference::Light,
                Appearance::Dark => ThemePreference::Dark,
            };
        }
        if let Some(layout) = input.layout {
            settings.layout = layout.into();
        }
        if let Some(scope) = input.save_scope {
            settings.save_scope = match scope {
                SaveScope::Shown => settings::SaveScope::Shown,
                SaveScope::All => settings::SaveScope::All,
            };
        }
        let switches = [
            (input.preview_files, &mut settings.preview_files),
            (input.inlay_hints, &mut settings.inlay_hints),
            (input.semantic_colouring, &mut settings.semantic_colouring),
            (
                input.sidebar_open_on_launch,
                &mut settings.sidebar_open_on_launch,
            ),
            (
                input.stop_idle_language_servers,
                &mut settings.stop_idle_language_servers,
            ),
        ];
        for (asked, switch) in switches {
            if let Some(on) = asked {
                *switch = on;
            }
        }
        if let Some(dir) = input.worktrees_dir {
            settings.worktrees_dir = dir.map(PathBuf::from);
        }
        if let Some(prefix) = input.branch_prefix {
            settings.branch_prefix = match prefix {
                BranchPrefix::GithubUser => settings::BranchPrefix::GitHubUser,
                BranchPrefix::Custom(prefix) => {
                    settings::BranchPrefix::Custom(prefix.trim().to_owned())
                }
                BranchPrefix::None => settings::BranchPrefix::None,
            };
        }
        if let Some(most) = input.most_language_servers {
            settings.most_language_servers =
                most.map(|most| usize::try_from(most).unwrap_or(usize::MAX));
        }
        if let Some(width) = sidebar_width {
            settings.sidebar_width = width;
        }
        if let Some(dock) = devtools.dock {
            settings.devtools.dock = match dock {
                DevtoolsDock::Bottom => Dock::Bottom,
                DevtoolsDock::Left => Dock::Left,
                DevtoolsDock::Right => Dock::Right,
                DevtoolsDock::Detached => Dock::Detached,
            };
        }
        if let Some(height) = height {
            settings.devtools.height = height;
        }
        if let Some(width) = width {
            settings.devtools.width = width;
        }
    });
    get(cx)
}

impl From<settings::Layout> for Layout {
    fn from(layout: settings::Layout) -> Layout {
        match layout {
            settings::Layout::Orca => Layout::Orca,
            settings::Layout::Arc => Layout::Arc,
        }
    }
}

impl From<Layout> for settings::Layout {
    fn from(layout: Layout) -> settings::Layout {
        match layout {
            Layout::Orca => settings::Layout::Orca,
            Layout::Arc => settings::Layout::Arc,
        }
    }
}
