//! The user's settings, kept in `settings.json` under the app's folder in Application Support
//! (`KOYO_CONFIG_DIR` points elsewhere). Each module reads its part at startup and writes
//! changes back through `update`, which saves once they settle.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, Context, Global, SharedString, Subscription, Task, Window};
use serde::{Deserialize, Serialize};

mod chat_details;
mod chat_mode;
mod project_scripts;
mod chat_work;
mod fonts;
mod language_servers;
mod migration;
mod save_scope;

pub use chat_details::{ChatDetails, DetailsItem, FlipDetailsItem};
pub use chat_mode::{SetChatMode, chat_mode_label, chat_mode_menu};
pub use project_scripts::{ProjectScript, ProjectScripts};
pub use chat_work::{ChatWork, SetChatWork};
pub use fonts::{CHAT_SIZES, Fonts, SetCodeFont, SetInterfaceFont};
pub use language_servers::{LanguageServers, Scope, SetLanguageServer};
pub use save_scope::{SaveScope, SetSaveScope};
use migration::migrate;

use crate::atomic_file;
use crate::gradient::GradientTheme;
use crate::key_repeat::KeyRepeat;
use crate::theme::{self, ThemePreference};

pub const APP_ID: &str = "dev.koyo.desktop";
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
        checked_menu(Layout::ALL, self, Layout::label, |layout| Box::new(SetLayout(layout)))
    }
}

/// A menu of `choices` with `current` checked, each named by `label` and, picked, setting
/// itself through `action`.
pub fn checked_menu<T: Copy + PartialEq, L: Into<SharedString>>(
    choices: impl IntoIterator<Item = T>,
    current: T,
    label: impl Fn(T) -> L,
    action: impl Fn(T) -> Box<dyn Action>,
) -> NativeMenu {
    choices.into_iter().fold(NativeMenu::new(), |menu, choice| {
        menu.menu_with_check(label(choice), choice == current, action(choice))
    })
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

/// Turns the preview tab of files picked in the tree on or off, from the tree's menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetPreviewFiles(pub bool);

/// Picks how many language servers run at once, or no limit, from a menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetMostLanguageServers(pub Option<usize>);

/// Picks what new branches are named under, from a menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetBranchPrefix(pub BranchPrefix);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[expect(clippy::struct_excessive_bools, reason = "a settings file: each switch is one")]
pub struct Settings {
    pub appearance: ThemePreference,
    pub light_theme: GradientTheme,
    pub dark_theme: GradientTheme,
    /// What text is in, and how big a chat's is.
    pub fonts: Fonts,
    pub layout: Layout,
    pub sidebar_width: f32,
    pub changes_files_width: f32,
    /// The Files panel's width, its tree's.
    pub files_tree_width: f32,
    /// Whether a file picked in the tree, by a click or the arrow keys, opens in a preview
    /// tab the next one picked takes over, rather than in a tab of its own.
    pub preview_files: bool,
    /// What ⌘S saves: the file shown, or every file with unsaved edits in the window.
    pub save_scope: SaveScope,
    /// Whether the types and parameter names a language server infers are shown greyed in
    /// the code where they'd go (inlay hints). On, as in Zed and VS Code.
    pub inlay_hints: bool,
    /// Whether a language server's own reading of the code colours it over the grammar's
    /// (semantic tokens): methods, fields, parameters, types in macros, doc comments; and
    /// the names in a language with no grammar here. On, as in VS Code.
    pub semantic_colouring: bool,
    pub sidebar_open_on_launch: bool,
    /// The main checkouts of the projects in the sidebar, in order.
    pub projects: Vec<PathBuf>,
    /// Where new worktrees go, in a folder per project. Unset, they go in
    /// `~/koyo/workspaces`.
    pub worktrees_dir: Option<PathBuf>,
    pub branch_prefix: BranchPrefix,
    /// The most language servers running at once, or `None` (`null`) for no limit, as in Zed
    /// and VS Code. rust-analyzer takes a few gigabytes on a big workspace; past this, the
    /// least recently used one stops for the next.
    pub most_language_servers: Option<usize>,
    /// Whether a language server stops once it has had no file open for ten minutes. Off,
    /// as in Zed and VS Code: a server runs only once a file of its language is open, and
    /// starting it again means indexing again.
    pub stop_idle_language_servers: bool,
    /// Which language servers run, everywhere and in each project.
    pub language_servers: LanguageServers,
    /// How much of what an agent did along the way shows in a chat.
    pub chat_work: ChatWork,
    /// What the details card over a chat shows, and whether it's up.
    pub chat_details: ChatDetails,
    /// The commands a person keeps for each project.
    pub project_scripts: ProjectScripts,
    /// How hard the agent works in a chat not set to anything else, as far as its model can.
    pub chat_effort: koyo_agent::protocol::model::Effort,
    /// What a new chat may do without asking; with none, what Claude's own settings say.
    pub chat_mode: Option<koyo_agent::protocol::model::Mode>,
    /// Where the `claude` program is; with none, it's looked for.
    pub claude_path: Option<String>,
    /// How a held arrow key repeats.
    pub key_repeat: KeyRepeat,
    /// Where a page's `DevTools` open, and the room they take docked.
    pub devtools: crate::browser::DevtoolsPlace,
    /// What quitting does with the terminals still running.
    pub on_quit: crate::quitting::OnQuit,
    /// Whether clients outside Koyo's terminals may control the app, on which port the
    /// MCP server listens for them, and the clients the person allowed. Only the person
    /// changes it, in Settings: no app-control operation reads or sets it.
    #[serde(rename = "externalControl")]
    pub external_control: crate::control::Policy,
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
            fonts: Fonts::default(),
            layout: Layout::default(),
            sidebar_width: theme::SIDEBAR_WIDTH,
            changes_files_width: theme::CHANGES_FILES_WIDTH,
            files_tree_width: theme::FILES_TREE_WIDTH,
            preview_files: true,
            save_scope: SaveScope::default(),
            inlay_hints: true,
            semantic_colouring: true,
            sidebar_open_on_launch: true,
            projects: Vec::new(),
            worktrees_dir: None,
            branch_prefix: BranchPrefix::default(),
            most_language_servers: None,
            stop_idle_language_servers: false,
            language_servers: language_servers::every_server(),
            chat_work: ChatWork::default(),
            chat_details: ChatDetails::default(),
            project_scripts: ProjectScripts::default(),
            chat_effort: koyo_agent::protocol::model::Effort::Xhigh,
            chat_mode: None,
            claude_path: None,
            key_repeat: KeyRepeat::default(),
            devtools: crate::browser::DevtoolsPlace::default(),
            on_quit: crate::quitting::OnQuit::default(),
            external_control: crate::control::Policy::default(),
        }
    }
}

/// The folder the app keeps what it fetches in, language servers among it, unless
/// `KOYO_DATA_DIR` points elsewhere.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("KOYO_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join(APP_ID)))
}

fn default_worktrees_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join("koyo")
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
    // Tests keep no file: each starts from the defaults, and none writes over the user's.
    let path = std::env::var_os("KOYO_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join(APP_ID)))
        .map(|dir| dir.join(FILE))
        .filter(|_| !cfg!(test));
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
    // within half a second. Tests leave the file be.
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
    cx.on_action(|SetSaveScope(scope): &SetSaveScope, cx| {
        update(cx, |settings| settings.save_scope = *scope);
    });
    cx.on_action(|SetMostLanguageServers(most): &SetMostLanguageServers, cx| {
        update(cx, |settings| settings.most_language_servers = *most);
    });
    cx.on_action(|SetLanguageServer { name, scope, on }: &SetLanguageServer, cx| {
        update(cx, |settings| settings.set_server(name, scope, *on));
    });
    cx.on_action(|SetChatWork(work): &SetChatWork, cx| {
        update(cx, |settings| settings.chat_work = *work);
    });
    cx.on_action(|SetChatMode(mode): &SetChatMode, cx| {
        update(cx, |settings| settings.chat_mode = *mode);
    });
    cx.on_action(|FlipDetailsItem(item): &FlipDetailsItem, cx| {
        update(cx, |settings| settings.chat_details.flip(*item));
    });
    cx.on_action(|SetBranchPrefix(prefix): &SetBranchPrefix, cx| {
        update(cx, |settings| settings.branch_prefix = prefix.clone());
    });
    // A change still settling when the app quits is written on the way out.
    cx.on_app_quit(|cx| {
        if cx.global_mut::<Store>().pending.take().is_some()
            && let Some((path, json)) = encode(cx)
        {
            atomic_file::write(&path, &json);
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

/// Calls `changed` whenever a setting changes.
pub fn on_change<V: 'static>(
    cx: &mut Context<V>,
    changed: impl FnMut(&mut V, &mut Context<V>) + 'static,
) -> Subscription {
    cx.observe_global::<Store>(changed)
}

/// Calls `changed` whenever a setting changes, for as long as the subscription is kept.
pub fn observe_app(cx: &mut App, changed: impl FnMut(&mut App) + 'static) -> Subscription {
    cx.observe_global::<Store>(changed)
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
                atomic_file::write(&path, &json);
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
    let mut settings: Settings = serde_json::from_value(value).map_err(|error| error.to_string())?;
    settings.list_every_server();
    Ok(settings)
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

#[cfg(test)]
mod tests;
