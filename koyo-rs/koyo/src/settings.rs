//! The user's settings, kept in `settings.json` under the app's folder in Application Support
//! (`KOYO_CONFIG_DIR` points elsewhere). Each module reads its part at startup and writes
//! changes back through `update`, which saves once they settle.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, Context, Global, SharedString, Subscription, Task, Window};
use serde::{Deserialize, Serialize};

mod accounts;
mod chat_activity;
mod chat_backdrop;
mod chat_details;
mod chat_mode;
mod chat_verbosity;
mod chat_width;
mod devices;
mod file;
mod fonts;
mod language_servers;
mod layout;
mod loose_pages;
mod migration;
mod picks;
mod project_scripts;
mod save_scope;
#[cfg(target_os = "macos")]
mod watch;

pub use accounts::{AccountSettings, SetAccountsThreshold};
pub use chat_activity::{ChatActivity, SetChatActivity};
pub use chat_backdrop::{ChatBackdrop, Quality, Surface};
pub use chat_details::{ChatDetails, DetailsItem, FlipDetailsItem};
pub use chat_mode::{SetChatMode, chat_mode_label, chat_mode_menu};
pub use chat_verbosity::{Preset, SetChatVerbosity, Shows, Step, Verbosity};
pub use chat_width::{ChatWidth, SetChatWidth};
pub use devices::MOST_SIMULATORS_AT_ONCE;
#[cfg(any(test, feature = "visual-check"))]
pub use fonts::LARGEST_CHAT_SIZE;
pub use fonts::{
    CHAT_SIZES, Fonts, SetChatTextSize, SetCodeFont, SetInterfaceFont, chat_size_menu,
};
pub use language_servers::{LanguageServers, Scope, SetLanguageServer};
pub use layout::{GLASS_OWN, GLASS_STOPS, Layout, SetLayout};
pub use loose_pages::{LoosePages, SetLoosePages};
pub use picks::{SetBranchPrefix, SetMostLanguageServers, SetPreviewFiles};
pub use project_scripts::{ProjectScript, ProjectScripts};
pub use save_scope::{SaveScope, SetSaveScope};

use crate::blocking::{self, Blocking};
use crate::gradient::GradientTheme;
use crate::key_repeat::KeyRepeat;
use crate::theme::{self, ThemePreference};

const FILE: &str = "settings.json";
/// How long changes must settle before they are written, so a drag saves once.
pub(crate) const SAVE_DELAY: Duration = Duration::from_millis(500);

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "a settings file: each switch is one"
)]
pub struct Settings {
    pub appearance: ThemePreference,
    pub light_theme: GradientTheme,
    pub dark_theme: GradientTheme,
    /// What text is in, and how big a chat's is.
    pub fonts: Fonts,
    pub layout: Layout,
    /// How much of the window's glass the card lets through, along its slider
    /// (`card_opacity`).
    pub card_glass: f32,
    /// How blurred the window's glass is, along its slider (`blur_scale`).
    pub glass_blur: f32,
    pub sidebar_width: f32,
    pub changes_files_width: f32,
    /// The Files panel's width, its tree's.
    pub files_tree_width: f32,
    /// Whether a file picked in the tree, by a click or the arrow keys, opens in a preview
    /// tab the next one picked takes over, rather than in a tab of its own.
    pub preview_files: bool,
    /// What ⌘S saves: the file shown, or every file with unsaved edits in the window.
    pub save_scope: SaveScope,
    /// How a markdown file opens: as it reads, as GitHub shows it, or as its code. Opened at a
    /// place in its text, a search result say, it opens as code either way.
    pub markdown_opens_as: crate::files::MarkdownMode,
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
    /// The spaces the Arc sidebar shows the projects in (`docs/arc-sidebar.md` 9), and the
    /// one it shows, by its place among them.
    pub spaces: Vec<crate::spaces::Space>,
    pub space: usize,
    /// What projects are called and show as in Koyo, by their main checkout.
    pub project_looks: crate::spaces::ProjectLooks,
    /// Whether the person put the Arc sidebar's empty zone for pinned tabs away
    /// (`docs/arc-sidebar.md` 11): it shows again only while a page's tab is dragged.
    pub pinned_zone_dismissed: bool,
    /// Whether each space lists loose pages of its own or every space the same ones.
    pub loose_pages: LoosePages,
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
    pub chat_verbosity: Verbosity,
    /// What the details card over a chat shows, and whether it's up.
    pub chat_details: ChatDetails,
    /// Whether a chat's agent keeps a plan of its task with Koyo's progress tool, which the chat's
    /// details card shows (`docs/chat.md` 5.5).
    pub chat_progress: bool,
    /// How wide a chat's timeline and composer get.
    pub chat_width: ChatWidth,
    /// Whether scrolling back up through a chat's section brings the prompt it answers down at the
    /// chat's top once that prompt's own place has gone by.
    pub floating_prompts: bool,
    /// What the composer's edge does while the agent or a subagent works.
    pub chat_activity: ChatActivity,
    /// The wallpaper behind a chat (`docs/chat-backdrop.md`).
    pub chat_backdrop: ChatBackdrop,
    /// The commands a person keeps for each project.
    pub project_scripts: ProjectScripts,
    /// How hard the agent works in a chat not set to anything else, as far as its model can.
    pub chat_effort: koyo_agent::protocol::model::Effort,
    /// What a new chat may do without asking; with none, what Claude's own settings say.
    pub chat_mode: Option<koyo_agent::protocol::model::Mode>,
    /// Where the `claude` program is; with none, it's looked for.
    pub claude_path: Option<String>,
    /// Whether a chat's agent drives Koyo with Koyo's own tools: its MCP server, registered in
    /// each session a chat opens, and trusted as an agent in a Koyo terminal is.
    pub koyo_tools: bool,
    /// How a held arrow key repeats.
    pub key_repeat: KeyRepeat,
    /// Where a page's `DevTools` open, and the room they take docked.
    pub devtools: crate::browser::DevtoolsPlace,
    /// The search engine words typed as an address go to.
    pub search_engine: crate::browser::SearchEngine,
    /// Whether the new-tab launcher asks the search engine what it suggests for what's typed.
    pub search_suggestions: bool,
    /// Whether a download asks where it goes, in the system's save panel; off, or for an
    /// agent's page, it goes to `downloads_dir`.
    pub downloads_ask: bool,
    /// The folder downloads go to without asking; the Downloads folder when none.
    pub downloads_dir: Option<PathBuf>,
    /// Whether a page that goes full screen, as a video does, takes the whole screen; off, it
    /// fills its pane (`browser/fullscreen.rs`).
    pub fullscreen_fills_screen: bool,
    /// What quitting does with the terminals still running.
    pub on_quit: crate::quitting::OnQuit,
    /// Whether agents have a desktop of their own to drive apps on (`docs/agent-desktop.md`):
    /// on, Koyo keeps one more desktop in Mission Control while it runs. Only the person
    /// changes it, in Settings.
    pub agent_desktop: bool,
    /// How many of Koyo's simulators may run at once (`docs/devices.md` 10): an agent's
    /// `device_open` past it is refused, the person's own Start asks first. Only the person
    /// changes it, in Settings → Devices. Read through [`Settings::simulators_at_once`].
    pub simulators_at_once: u8,
    /// Whether clients outside Koyo's terminals may control the app, on which port the
    /// MCP server listens for them, and the clients the person allowed. Only the person
    /// changes it, in Settings: no app-control operation reads or sets it.
    #[serde(rename = "externalControl")]
    pub external_control: crate::control::Policy,
    /// Claude accounts: whether Koyo manages the login, and auto-switch (`docs/claude-accounts.md`).
    pub accounts: AccountSettings,
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
            card_glass: GLASS_OWN,
            glass_blur: GLASS_OWN,
            sidebar_width: theme::SIDEBAR_WIDTH,
            changes_files_width: theme::CHANGES_FILES_WIDTH,
            files_tree_width: theme::FILES_TREE_WIDTH,
            preview_files: true,
            save_scope: SaveScope::default(),
            markdown_opens_as: crate::files::MarkdownMode::Preview,
            inlay_hints: true,
            semantic_colouring: true,
            sidebar_open_on_launch: true,
            projects: Vec::new(),
            spaces: Vec::new(),
            space: 0,
            project_looks: crate::spaces::ProjectLooks::new(),
            pinned_zone_dismissed: false,
            loose_pages: LoosePages::default(),
            worktrees_dir: None,
            branch_prefix: BranchPrefix::default(),
            most_language_servers: None,
            stop_idle_language_servers: false,
            language_servers: language_servers::every_server(),
            chat_verbosity: Verbosity::default(),
            chat_details: ChatDetails::default(),
            chat_progress: true,
            chat_width: ChatWidth::default(),
            floating_prompts: true,
            chat_activity: ChatActivity::default(),
            chat_backdrop: ChatBackdrop::default(),
            project_scripts: ProjectScripts::default(),
            chat_effort: koyo_agent::protocol::model::Effort::Xhigh,
            chat_mode: None,
            claude_path: None,
            koyo_tools: true,
            key_repeat: KeyRepeat::default(),
            devtools: crate::browser::DevtoolsPlace::default(),
            search_engine: crate::browser::SearchEngine::Google,
            search_suggestions: true,
            downloads_ask: false,
            downloads_dir: None,
            fullscreen_fills_screen: true,
            on_quit: crate::quitting::OnQuit::default(),
            agent_desktop: false,
            simulators_at_once: 2,
            external_control: crate::control::Policy::default(),
            accounts: AccountSettings::default(),
        }
    }
}

/// The folder the app keeps what it fetches in, language servers among it: the one its
/// daemons and relays find, wherever `KOYO_DATA_DIR` points or the app's own.
pub fn data_dir() -> Option<PathBuf> {
    koyo_daemon::data_dir()
}

fn default_worktrees_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join("koyo")
        .join("workspaces")
}

/// The settings, and their file's bookkeeping apart from them in a cell: noting a save or a
/// read of the file tells none of the settings' observers anything changed.
struct Store {
    settings: Settings,
    path: Option<PathBuf>,
    file: RefCell<OnDisk>,
}

#[derive(Default)]
struct OnDisk {
    /// The save waiting for changes to settle; replacing it cancels it.
    pending: Option<Task<()>>,
    /// Whether a change made in the app is still to be written, so the file doesn't undo it.
    unsaved: bool,
    /// When the file was last changed as the app knows it, read or written.
    seen: Option<SystemTime>,
}

impl Global for Store {}

/// Loads the settings, falling back to the defaults for a missing or unreadable file, and
/// watches the file for an edit made outside the app.
pub fn init(cx: &mut App, may: Blocking<'_>) {
    let path = std::env::var_os("KOYO_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| koyo_daemon::app_folder(&dir)))
        .map(|dir| dir.join(FILE))
        .filter(|_| !cfg!(test));
    let mut settings: Settings = (path.as_deref())
        .map(|path| file::load(path, may))
        .unwrap_or_default();
    settings.reconcile_spaces();
    let seen = path.as_deref().and_then(|path| file::modified(path, may));
    let file = RefCell::new(OnDisk {
        seen,
        ..OnDisk::default()
    });
    #[cfg(target_os = "macos")]
    if let Some(path) = &path {
        watch::start(path.clone(), seen, cx);
    }
    cx.set_global(Store {
        settings,
        path,
        file,
    });
    cx.on_action(|SetLayout(layout): &SetLayout, cx| {
        update(cx, |settings| settings.layout = *layout);
    });
    cx.on_action(|SetPreviewFiles(on): &SetPreviewFiles, cx| {
        update(cx, |settings| settings.preview_files = *on);
    });
    cx.on_action(
        |SetAccountsThreshold(threshold): &SetAccountsThreshold, cx| {
            update(cx, |settings| settings.accounts.threshold = *threshold);
        },
    );
    cx.on_action(|SetSaveScope(scope): &SetSaveScope, cx| {
        update(cx, |settings| settings.save_scope = *scope);
    });
    cx.on_action(|SetLoosePages(pages): &SetLoosePages, cx| {
        update(cx, |settings| settings.loose_pages = *pages);
    });
    cx.on_action(
        |SetMostLanguageServers(most): &SetMostLanguageServers, cx| {
            update(cx, |settings| settings.most_language_servers = *most);
        },
    );
    cx.on_action(
        |SetLanguageServer { name, scope, on }: &SetLanguageServer, cx| {
            update(cx, |settings| settings.set_server(name, scope, *on));
        },
    );
    cx.on_action(|SetChatVerbosity(verbosity): &SetChatVerbosity, cx| {
        update(cx, |settings| settings.chat_verbosity = *verbosity);
    });
    cx.on_action(|SetChatWidth(width): &SetChatWidth, cx| {
        update(cx, |settings| settings.chat_width = *width);
    });
    cx.on_action(|SetChatActivity(activity): &SetChatActivity, cx| {
        update(cx, |settings| settings.chat_activity = *activity);
    });
    cx.on_action(|SetChatTextSize(size): &SetChatTextSize, cx| {
        update(cx, |settings| settings.fonts.chat_size = *size);
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
    crate::kept_file::write_on_quit(cx, file::due_at_quit);
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
        if let Some((path, json)) = cx.update(file::encode) {
            let landed = cx.update(|cx| crate::kept_file::write(path.clone(), json, cx));
            let _ = landed.await;
            let seen = blocking::spawn(cx.background_executor(), move |may| {
                file::modified(&path, may)
            })
            .await;
            cx.update(|cx| {
                let mut file = cx.global::<Store>().file.borrow_mut();
                (file.unsaved, file.seen) = (false, seen);
            });
        }
    });
    let mut file = cx.global::<Store>().file.borrow_mut();
    (file.pending, file.unsaved) = (Some(save), true);
}

#[cfg(test)]
pub(crate) mod tests;
