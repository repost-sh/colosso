//! The user's settings, kept in `settings.json` under the app's folder in Application Support
//! (`COLOSSO_CONFIG_DIR` points elsewhere). Each module reads its part at startup and writes
//! changes back through `update`, which saves once they settle.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, Context, Global, Subscription, Task, Window};
use serde::{Deserialize, Serialize};

use crate::gradient::GradientTheme;
use crate::key_repeat::KeyRepeat;
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

/// Turns the Files panel's preview tab on or off, from its menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetPreviewFiles(pub bool);

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
    /// The Files panel's width, its tree and the file shown beside it.
    pub files_panel_width: f32,
    pub files_tree_width: f32,
    /// Whether a file picked in the Files panel's tree, by a click or the arrow keys, opens in
    /// a preview tab the next one picked takes over, rather than in a tab of its own.
    pub preview_files: bool,
    pub sidebar_open_on_launch: bool,
    /// The main checkouts of the projects in the sidebar, in order.
    pub projects: Vec<PathBuf>,
    /// Where new worktrees go, in a folder per project. Unset, they go in
    /// `~/colosso/workspaces`.
    pub worktrees_dir: Option<PathBuf>,
    pub branch_prefix: BranchPrefix,
    /// The most language servers running at once. rust-analyzer takes a gigabyte or more on
    /// a big workspace; past this, the least recently used one stops for the next.
    pub most_language_servers: usize,
    /// How a held arrow key repeats.
    pub key_repeat: KeyRepeat,
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
            files_panel_width: theme::FILES_WIDTH,
            files_tree_width: theme::FILES_TREE_WIDTH,
            preview_files: true,
            sidebar_open_on_launch: true,
            projects: Vec::new(),
            worktrees_dir: None,
            branch_prefix: BranchPrefix::default(),
            most_language_servers: 4,
            key_repeat: KeyRepeat::default(),
        }
    }
}

/// The folder the app keeps what it fetches in, language servers among it, unless
/// `COLOSSO_DATA_DIR` points elsewhere.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("COLOSSO_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join(APP_ID)))
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
    /// Whether a change made in the app is still to be written, so the file doesn't undo it.
    unsaved: bool,
    /// When the file was last changed as the app knows it, read or written.
    seen: Option<SystemTime>,
}

impl Global for Store {}

/// Loads the settings, falling back to the defaults for a missing or unreadable file.
pub fn init(cx: &mut App) {
    let path = std::env::var_os("COLOSSO_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join(APP_ID)))
        .map(|dir| dir.join(FILE));
    let settings = path.as_deref().map(load).unwrap_or_default();
    let seen = path.as_deref().and_then(modified);
    cx.set_global(Store {
        settings,
        path,
        pending: None,
        unsaved: false,
        seen,
    });
    // Edits to the file while the app runs, a key's repeat tuned by hand say, take effect
    // within half a second. Tests share one file, so they leave it be.
    #[cfg(not(test))]
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            cx.update(reload);
        }
    })
    .detach();
    cx.on_action(|SetLayout(layout): &SetLayout, cx| {
        update(cx, |settings| settings.layout = *layout);
    });
    cx.on_action(|SetPreviewFiles(on): &SetPreviewFiles, cx| {
        update(cx, |settings| settings.preview_files = *on);
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

/// Calls `changed` whenever a setting changes, with the window `cx`'s view is in.
pub fn observe_in<V: 'static>(
    window: &Window,
    cx: &mut Context<V>,
    changed: impl FnMut(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> Subscription {
    cx.observe_global_in::<Store>(window, changed)
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
            let written = cx.background_executor().spawn(async move {
                write(&path, &json);
                modified(&path)
            });
            let seen = written.await;
            cx.update(|cx| {
                let store = cx.global_mut::<Store>();
                (store.unsaved, store.seen) = (false, seen);
            });
        }
    });
    let store = cx.global_mut::<Store>();
    (store.pending, store.unsaved) = (Some(save), true);
}

/// Takes up an edit made to the file outside the app since it was last read or written,
/// unless a change made in the app is still to be written; an edit that doesn't read as
/// settings, as one half done may not, is noted and left until the next.
fn reload(cx: &mut App) {
    let store = cx.global::<Store>();
    let Some(path) = store.path.clone().filter(|_| !store.unsaved) else {
        return;
    };
    let changed = modified(&path);
    if changed == store.seen {
        return;
    }
    let read = read(&path);
    let store = cx.global_mut::<Store>();
    store.seen = changed;
    match read {
        Ok(settings) if settings != store.settings => store.settings = settings,
        Ok(_) => {}
        Err(error) => log::warn!("not taking up {}: {error}", path.display()),
    }
}

/// When the file at `path` last changed, if it's there.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

fn read(path: &Path) -> Result<Settings, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let mut value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    migrate(&mut value);
    serde_json::from_value(value).map_err(|error| error.to_string())
}

/// Settings a file may hold that the app no longer has, each where it was and why it went.
/// A key that's renamed, or changes what it means, goes here under its old name.
const RETIRED: [(&[&str], &str); 5] = [
    // The Files panel's tree's width, from before the panel showed a file beside its tree:
    // too narrow for both, it gave way to `files_panel_width` and `files_tree_width`.
    (&[], "files_width"),
    // A held arrow sped up the longer it was held, in steps a frame; it now has one speed,
    // `per_second`, the same whatever the display's rate.
    (&["key_repeat"], "start_per_frame"),
    (&["key_repeat"], "accelerate_after_ms"),
    (&["key_repeat"], "full_speed_after_ms"),
    (&["key_repeat"], "most_per_frame"),
];

/// Brings settings read from a file to what the app has now: retired keys are dropped, so
/// what they meant isn't taken for what a key of the same name means later.
fn migrate(value: &mut serde_json::Value) {
    for (within, key) in RETIRED {
        let object = within
            .iter()
            .try_fold(&mut *value, |value, key| value.get_mut(*key));
        if let Some(serde_json::Value::Object(object)) = object {
            object.remove(key);
        }
    }
}

fn load(path: &Path) -> Settings {
    match read(path) {
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
mod tests;
