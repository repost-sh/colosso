//! Browsing profiles (`docs/browser.md` 15): what pages keep of the sites they visit, their
//! cookies, sign-ins, site storage and caches, apart from another profile's. Default is always
//! there: pages opened outside any project use it, and so does a project until it's put on
//! another. The person makes as many more as they like, in Settings → Browser or from a
//! project's Set Profile menu (`actions.rs`). A project's pages, in every worktree of it, use
//! its profile, and projects on the same profile share their sign-ins. A page asks
//! [`profile`](super::profile) which profile it's of, whichever way it's opened.
//!
//! They're kept in `browser-profiles.json` in the data folder, not in `settings.json`: a build
//! that doesn't know them writes the settings with only what it knows, and would take them
//! with it.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui_kit::{App, Context, Global, Subscription};
use serde::{Deserialize, Serialize};

use crate::kept_file::{self, KeptFile};
use crate::settings;
use crate::spaces::ProjectLooks;

mod actions;

pub use actions::{DeleteProfile, NewProfile, RenameProfile, handle_actions, menu};

/// Default's id: the folder the pages opened outside any project kept before profiles had
/// names (`own(Path::new(NO_PROJECT))`), so what they're signed in to stays.
pub const DEFAULT: &str = "no-project-5131f9da27ba906e";

/// What Default is called. It can't be renamed or deleted.
pub const DEFAULT_NAME: &str = "Default";

const FILE: &str = "browser-profiles.json";
const SAVE_DELAY: Duration = Duration::from_millis(300);

/// A profile the person made, or one a project had of its own before profiles had names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Its folder's name among the engine's profiles, kept whatever it's called.
    pub id: String,
    pub name: String,
}

/// The profiles besides Default, and the one each project's pages use, as the file keeps them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profiles {
    /// In the order they're listed.
    pub made: Vec<Profile>,
    /// By the project's main checkout; a project not here is on Default.
    pub projects: BTreeMap<PathBuf, String>,
}

/// What a test that sets none up reads: no profile but Default.
static NONE: Profiles = Profiles {
    made: Vec::new(),
    projects: BTreeMap::new(),
};

/// The folder a project's pages kept before profiles had names: named after the project's
/// folder and its path, so a project brought over keeps its sign-ins.
pub fn own(project: &Path) -> String {
    // FNV-1a of the path: the name must stay what it was, or a project would lose its logins.
    let hash = project
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    let name = project
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{}-{hash:016x}", slug(&name))
}

/// `name` as the start of a profile's folder: letters, digits and dashes, lower-case.
fn slug(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(24)
        .collect::<String>()
        .to_ascii_lowercase()
}

impl Profiles {
    /// The profile the pages of `project`, a main checkout, use: the one it's on, Default when
    /// it's on none or on one that's gone. A page opened outside any project is Default's.
    pub fn of(&self, project: &Path) -> &str {
        (self.projects.get(project).map(String::as_str))
            .filter(|id| self.has(id))
            .unwrap_or(DEFAULT)
    }

    /// Whether there's a profile of this id, Default or one of those made.
    pub fn has(&self, id: &str) -> bool {
        id == DEFAULT || self.made.iter().any(|made| made.id == id)
    }

    /// Every profile, as `(id, name)`: Default, then those made, in their order.
    pub fn all(&self) -> impl Iterator<Item = (&str, &str)> {
        let made = (self.made.iter()).map(|made| (made.id.as_str(), made.name.as_str()));
        std::iter::once((DEFAULT, DEFAULT_NAME)).chain(made)
    }

    /// What the profile of this id is called; Default's name for one that's gone.
    pub fn name(&self, id: &str) -> &str {
        (self.all().find(|(of, _)| *of == id)).map_or(DEFAULT_NAME, |(_, name)| name)
    }

    /// Those of `projects`, main checkouts in the sidebar's order, on the profile of this id.
    pub fn on<'a>(
        &'a self,
        id: &'a str,
        projects: &'a [PathBuf],
    ) -> impl Iterator<Item = &'a Path> + 'a {
        (projects.iter())
            .filter(move |project| self.of(project) == id)
            .map(PathBuf::as_path)
    }

    /// Puts `project`'s pages on the profile of this id: Default's keeps nothing for it.
    pub fn set(&mut self, project: &Path, id: &str) {
        if id == DEFAULT {
            self.projects.remove(project);
        } else {
            self.projects.insert(project.to_owned(), id.to_owned());
        }
    }

    /// Makes a profile called `name`, after the others, with a folder of its own that no
    /// profile had before: one deleted keeps its folder until the engine next starts. Its id.
    pub fn make(&mut self, name: &str) -> String {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let mut stamp = since.as_nanos() as u64;
        let start = match slug(name) {
            slug if slug.is_empty() => "profile".to_owned(),
            slug => slug,
        };
        let id = loop {
            let id = format!("{start}-{stamp:016x}");
            if !self.has(&id) {
                break id;
            }
            stamp = stamp.wrapping_add(1);
        };
        let name = name.trim().to_owned();
        self.made.push(Profile {
            id: id.clone(),
            name,
        });
        id
    }

    /// Calls the profile of this id `name`. Default keeps its name, and no profile goes
    /// without one.
    pub fn rename(&mut self, id: &str, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if let Some(profile) = self.made.iter_mut().find(|made| made.id == id) {
            name.clone_into(&mut profile.name);
        }
    }

    /// Takes the profile of this id off the list, the projects on it onto Default. Default
    /// can't go: it's none of those made.
    pub fn delete(&mut self, id: &str) {
        self.made.retain(|made| made.id != id);
        self.projects.retain(|_, on| on != id);
    }

    /// Lists the profile each of `projects` kept of its own before profiles had names, called
    /// as the project is in `looks`, with the project on it: what a project's pages are signed
    /// in to stays. Only a profile that `kept` says has a folder is, so a project whose pages
    /// kept nothing starts on Default, as a new one does; a project on a profile already is
    /// left be.
    pub fn bring_over(
        &mut self,
        projects: &[PathBuf],
        looks: &ProjectLooks,
        kept: impl Fn(&str) -> bool,
    ) {
        for project in projects {
            let id = own(project);
            if self.projects.contains_key(project) || self.has(&id) || !kept(&id) {
                continue;
            }
            let name = crate::model::project_name(looks, project).to_string();
            self.made.push(Profile {
                id: id.clone(),
                name,
            });
            self.set(project, &id);
        }
    }
}

/// The profiles, and where they're kept: apart from them, so taking a write that's due changes
/// nothing those watching the profiles hear of.
struct Store {
    profiles: Profiles,
    file: RefCell<KeptFile>,
}

impl Global for Store {}

/// The profiles as they are now.
pub fn get(cx: &App) -> &Profiles {
    cx.try_global::<Store>()
        .map_or(&NONE, |store| &store.profiles)
}

/// Changes the profiles, which are kept a moment later; what lists them, Settings → Browser,
/// shows them anew, and the open pages of a project put on another follow it
/// (`Browsers::profiles_changed`).
pub fn update(cx: &mut App, change: impl FnOnce(&mut Profiles)) {
    let mut profiles = get(cx).clone();
    change(&mut profiles);
    if profiles == *get(cx) {
        return;
    }
    let store = cx.global_mut::<Store>();
    store.profiles = profiles;
    if store.file.get_mut().changed() {
        kept_file::write_later(cx, SAVE_DELAY, due);
    }
    cx.refresh_windows();
}

/// Calls `changed` whenever the profiles change, for as long as the subscription is kept.
pub fn on_change<V: 'static>(
    cx: &mut Context<V>,
    changed: impl FnMut(&mut V, &mut Context<V>) + 'static,
) -> Subscription {
    cx.observe_global::<Store>(changed)
}

/// Where to write the profiles, and their JSON, if a write is due, which then no longer is.
fn due(cx: &mut App) -> Option<(PathBuf, Vec<u8>)> {
    let store = cx.global::<Store>();
    let path = store.file.borrow_mut().take_due()?;
    Some((path, kept_file::encode(&store.profiles)?))
}

/// Reads the profiles from the data folder; and picking one for a project needs no window.
pub(super) fn init(cx: &mut App) {
    // Tests read and write none of a person's.
    let path = settings::data_dir()
        .filter(|_| !cfg!(test))
        .map(|data| data.join(FILE));
    let root = super::engine::profiles();
    let kept =
        |id: &str| (root.as_deref()).is_some_and(|root| koyo_browser_host::keeps_profile(root, id));
    open(path, kept, cx);
    // A change still settling when the app quits is written on the way out.
    kept_file::write_on_quit(cx, due);
    actions::init(cx);
}

/// Takes up the profiles kept at `path`. With none kept there yet, as the first time a build
/// with profiles starts, each project's own profile is brought over, which `kept` says has a
/// folder (`Profiles::bring_over`), and the file's written at once, so that's done once.
fn open(path: Option<PathBuf>, kept: impl Fn(&str) -> bool, cx: &mut App) {
    let read: Option<Profiles> = path.as_deref().and_then(kept_file::read);
    let bring_over = read.is_none() && path.is_some();
    let mut profiles = read.unwrap_or_default();
    if bring_over {
        let settings = settings::get(cx);
        profiles.bring_over(&settings.projects, &settings.project_looks, kept);
        kept_file::write_now(path.clone().zip(kept_file::encode(&profiles)));
    }
    let file = RefCell::new(KeptFile::new(path));
    cx.set_global(Store { profiles, file });
}

#[cfg(test)]
mod tests;
