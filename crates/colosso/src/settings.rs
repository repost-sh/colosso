//! The user's settings, kept in `settings.json` under the app's folder in Application Support
//! (`COLOSSO_CONFIG_DIR` points elsewhere). Each module reads its part at startup and writes
//! changes back through `update`, which saves once they settle.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, Context, Global, Subscription, Task};
use serde::{Deserialize, Serialize};

use crate::gradient::GradientTheme;
use crate::theme::{self, ThemePreference};

const APP_ID: &str = "dev.colosso.desktop";
const FILE: &str = "settings.json";
/// How long changes must settle before they are written, so a drag saves once.
const SAVE_DELAY: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Layout {
    #[default]
    Orca,
    Arc,
}

/// Switches the window to a layout, from a menu or a key.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetLayout(pub Layout);

impl Layout {
    pub const ALL: [Layout; 2] = [Layout::Orca, Layout::Arc];

    pub fn label(self) -> &'static str {
        match self {
            Layout::Orca => "Orca",
            Layout::Arc => "Arc",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Layout::Orca => Layout::Arc,
            Layout::Arc => Layout::Orca,
        }
    }

    /// A menu of the layouts with `self` checked.
    pub fn menu(self) -> NativeMenu {
        Layout::ALL
            .into_iter()
            .fold(NativeMenu::new(), |menu, layout| {
                menu.menu_with_check(layout.label(), layout == self, Box::new(SetLayout(layout)))
            })
    }
}

/// What the branches of new worktrees are named under: `<prefix>/<name>`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BranchPrefix {
    /// The GitHub username git's config or the GitHub CLI knows; no prefix if neither does.
    #[default]
    GitHubUser,
    Custom(String),
    None,
}

/// Picks what new branches are named under, from a menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetBranchPrefix(pub BranchPrefix);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: ThemePreference,
    pub light_theme: GradientTheme,
    pub dark_theme: GradientTheme,
    pub layout: Layout,
    pub sidebar_width: f32,
    pub changes_width: f32,
    pub changes_files_width: f32,
    pub files_width: f32,
    pub sidebar_open_on_launch: bool,
    /// The main checkouts of the projects in the sidebar, in order.
    pub projects: Vec<PathBuf>,
    /// Where new worktrees go, in a folder per project. Unset, they go in
    /// `~/colosso/workspaces`.
    pub worktrees_dir: Option<PathBuf>,
    pub branch_prefix: BranchPrefix,
}

impl Settings {
    /// The gradient theme for the dark or the light appearance.
    pub fn theme(&self, dark: bool) -> &GradientTheme {
        if dark {
            &self.dark_theme
        } else {
            &self.light_theme
        }
    }

    pub fn theme_mut(&mut self, dark: bool) -> &mut GradientTheme {
        if dark {
            &mut self.dark_theme
        } else {
            &mut self.light_theme
        }
    }

    /// The folder new worktrees go in, each project's in a folder of its own.
    pub fn worktrees_root(&self) -> PathBuf {
        self.worktrees_dir
            .clone()
            .unwrap_or_else(default_worktrees_dir)
    }

    /// The folder new worktrees of a project called `project` go in.
    pub fn worktrees_dir(&self, project: &str) -> PathBuf {
        self.worktrees_root().join(project)
    }

    /// The sidebar's width, kept within its limits even if the file says otherwise.
    pub fn sidebar_width(&self) -> f32 {
        self.sidebar_width
            .clamp(theme::SIDEBAR_MIN_WIDTH, theme::SIDEBAR_MAX_WIDTH)
    }

    /// The changes panel's file list's width, kept within its limits.
    pub fn changes_files_width(&self) -> f32 {
        self.changes_files_width.clamp(
            theme::CHANGES_FILES_MIN_WIDTH,
            theme::CHANGES_FILES_MAX_WIDTH,
        )
    }

    /// The changes panel's width, kept within its limits even if the file says otherwise.
    pub fn changes_width(&self) -> f32 {
        self.changes_width
            .clamp(theme::CHANGES_MIN_WIDTH, theme::CHANGES_MAX_WIDTH)
    }

    /// The Files panel's width, kept within its limits.
    pub fn files_width(&self) -> f32 {
        self.files_width
            .clamp(theme::FILES_MIN_WIDTH, theme::FILES_MAX_WIDTH)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            appearance: ThemePreference::default(),
            light_theme: GradientTheme::default(),
            dark_theme: GradientTheme::default(),
            layout: Layout::default(),
            sidebar_width: theme::SIDEBAR_WIDTH,
            changes_width: theme::CHANGES_WIDTH,
            changes_files_width: theme::CHANGES_FILES_WIDTH,
            files_width: theme::FILES_WIDTH,
            sidebar_open_on_launch: true,
            projects: Vec::new(),
            worktrees_dir: None,
            branch_prefix: BranchPrefix::default(),
        }
    }
}

fn default_worktrees_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join("colosso")
        .join("workspaces")
}

struct Store {
    settings: Settings,
    path: Option<PathBuf>,
    /// The save waiting for changes to settle; replacing it cancels it.
    pending: Option<Task<()>>,
}

impl Global for Store {}

/// Loads the settings, falling back to the defaults for a missing or unreadable file.
pub fn init(cx: &mut App) {
    let path = std::env::var_os("COLOSSO_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join(APP_ID)))
        .map(|dir| dir.join(FILE));
    let settings = path.as_deref().map(load).unwrap_or_default();
    cx.set_global(Store {
        settings,
        path,
        pending: None,
    });
    cx.on_action(|SetLayout(layout): &SetLayout, cx| {
        update(cx, |settings| settings.layout = *layout);
    });
    cx.on_action(|SetBranchPrefix(prefix): &SetBranchPrefix, cx| {
        update(cx, |settings| settings.branch_prefix = prefix.clone());
    });
    // A change still settling when the app quits is written on the way out.
    cx.on_app_quit(|cx| {
        if cx.global_mut::<Store>().pending.take().is_some()
            && let Some((path, json)) = encode(cx)
        {
            write(&path, &json);
        }
        async {}
    })
    .detach();
}

pub fn get(cx: &App) -> &Settings {
    &cx.global::<Store>().settings
}

/// Where the settings are saved, if the app has a folder to save them in.
pub fn path(cx: &App) -> Option<&Path> {
    cx.global::<Store>().path.as_deref()
}

/// Re-renders the observing view whenever a setting changes.
pub fn observe<V: 'static>(cx: &mut Context<V>) -> Subscription {
    cx.observe_global::<Store>(|_, cx| cx.notify())
}

/// Applies `change` and saves once changes settle, if it changed anything. Observers only
/// hear about changes, so a change to nothing does not re-render them.
pub fn update(cx: &mut App, change: impl FnOnce(&mut Settings)) {
    let mut settings = get(cx).clone();
    change(&mut settings);
    if settings == *get(cx) {
        return;
    }
    cx.global_mut::<Store>().settings = settings;
    let save = cx.spawn(async move |cx| {
        cx.background_executor().timer(SAVE_DELAY).await;
        if let Some((path, json)) = cx.update(encode) {
            cx.background_executor()
                .spawn(async move { write(&path, &json) })
                .await;
        }
    });
    cx.global_mut::<Store>().pending = Some(save);
}

fn load(path: &Path) -> Settings {
    let read = std::fs::read(path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|error| error.to_string()));
    match read {
        Ok(settings) => settings,
        // Before anything was saved.
        Err(_) if !path.exists() => Settings::default(),
        Err(error) => {
            log::warn!("ignoring unreadable {}: {error}", path.display());
            Settings::default()
        }
    }
}

/// The settings file's path and the JSON to write there.
fn encode(cx: &mut App) -> Option<(PathBuf, Vec<u8>)> {
    let store = cx.global::<Store>();
    let path = store.path.clone()?;
    match serde_json::to_vec_pretty(&store.settings) {
        Ok(json) => Some((path, json)),
        Err(error) => {
            log::error!("couldn't encode the settings: {error}");
            None
        }
    }
}

/// Writes through a temporary file, so a crash mid-write never leaves half a file behind.
fn write(path: &Path, json: &[u8]) {
    let temporary = path.with_extension("json.tmp");
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&temporary, json))
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = written {
        log::error!("couldn't save {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::point;

    use super::*;
    use crate::gradient::Harmony;

    #[test]
    fn settings_survive_a_round_trip() {
        let mut settings = Settings {
            appearance: ThemePreference::Dark,
            layout: Layout::Arc,
            sidebar_width: 320.,
            changes_width: 700.,
            projects: vec!["/src/app".into()],
            worktrees_dir: Some("/work".into()),
            branch_prefix: BranchPrefix::Custom("me".into()),
            ..Settings::default()
        };
        settings.dark_theme.dots = vec![point(250., 120.), point(110., 240.)];
        settings.dark_theme.harmony = Harmony::Complementary;
        settings.light_theme.texture = 0.5;
        let json = serde_json::to_vec(&settings).unwrap();
        assert_eq!(serde_json::from_slice::<Settings>(&json).unwrap(), settings);
    }

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "a missing width is the default itself, not near it"
    )]
    fn missing_fields_fall_back_to_defaults() {
        let settings: Settings = serde_json::from_str(r#"{ "layout": "Arc" }"#).unwrap();
        assert_eq!(settings.layout, Layout::Arc);
        assert_eq!(settings.sidebar_width, theme::SIDEBAR_WIDTH);
        assert_eq!(settings.changes_width, theme::CHANGES_WIDTH);
        assert_eq!(settings.dark_theme, GradientTheme::default());
        assert_eq!(settings.branch_prefix, BranchPrefix::GitHubUser);
    }

    #[test]
    fn worktrees_go_in_a_folder_per_project() {
        let settings = Settings {
            worktrees_dir: Some("/work".into()),
            ..Settings::default()
        };
        assert_eq!(settings.worktrees_dir("app"), Path::new("/work/app"));
        assert!(
            Settings::default()
                .worktrees_dir("app")
                .ends_with("colosso/workspaces/app")
        );
    }
}
