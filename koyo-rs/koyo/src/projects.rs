//! Keeps the workspace in step with git: reads each project's worktrees off the main thread,
//! adds and removes projects, and saves the list. Git is asked again on launch, whenever the
//! window comes back to the front, and after the app changes a project's worktrees itself.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{
    Action, App, AppContext as _, ClipboardItem, Context, Div, InteractiveElement, MouseButton,
    MouseDownEvent, PathPromptOptions, SharedString, Task, Window, actions,
};
use koyo_git::{Repo, Worktree};

use crate::model::PaneKind;
use crate::settings;
use crate::shell::Shell;
use crate::{delete_worktree, modal, new_worktree};

actions!(projects, [AddProject, NewWorktree]);

/// Starts a worktree of the project whose main checkout is at the path.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = projects, no_json)]
pub struct NewWorktreeIn(pub PathBuf);

/// Takes the project whose main checkout is at the path out of the sidebar.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = projects, no_json)]
pub struct RemoveProject(pub PathBuf);

/// Asks to stop every terminal of the checkout at the path.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = projects, no_json)]
pub struct StopTerminals(pub PathBuf);

/// Asks to delete the worktree at the path.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = projects, no_json)]
pub struct DeleteWorktree(pub PathBuf);

#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = projects, no_json)]
pub struct RevealInFinder(pub PathBuf);

#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = projects, no_json)]
pub struct CopyPath(pub PathBuf);

/// Has `element` open the menu `build` makes where it is right-clicked, rather than any menu of
/// what it's in.
pub fn with_menu<E: InteractiveElement>(
    element: E,
    build: impl Fn(&mut Window, &mut App) -> NativeMenu + 'static,
) -> E {
    element.on_mouse_down(
        MouseButton::Right,
        move |event: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            build(window, cx).show(event.position, window, cx);
        },
    )
}

/// The menu a project's row opens, for its main checkout at `path`.
pub fn project_menu(path: &Path) -> NativeMenu {
    project_items(NativeMenu::new(), path)
}

/// What any project's menu offers, after `menu`'s own items: a new worktree, the folder's
/// items, and removing it.
pub fn project_items(menu: NativeMenu, path: &Path) -> NativeMenu {
    let menu = menu
        .menu("New Worktree…", Box::new(NewWorktreeIn(path.to_owned())))
        .separator();
    path_items(menu, path)
        .separator()
        .menu("Remove Project", Box::new(RemoveProject(path.to_owned())))
}

/// The menu a checkout's row opens. Git can't delete a main checkout.
pub fn worktree_menu(path: &Path, is_main: bool, terminals: bool) -> NativeMenu {
    let mut menu = path_items(NativeMenu::new(), path);
    if terminals {
        menu = (menu.separator()).menu("Stop Terminals…", Box::new(StopTerminals(path.to_owned())));
    }
    if is_main {
        return menu;
    }
    menu.separator().menu(
        "Delete Worktree…",
        Box::new(DeleteWorktree(path.to_owned())),
    )
}

/// What any folder's menu offers.
pub fn path_items(menu: NativeMenu, path: &Path) -> NativeMenu {
    menu.menu(
        "Reveal in Finder",
        Box::new(RevealInFinder(path.to_owned())),
    )
    .menu("Copy Path", Box::new(CopyPath(path.to_owned())))
}

/// Asks the user for a folder, `prompt` naming the button that picks it.
pub fn pick_folder(
    prompt: &'static str,
    cx: &mut App,
) -> impl Future<Output = Option<PathBuf>> + use<> {
    pick_path(prompt, true, cx)
}

/// Asks the user for a file, or a folder if `folder`, `prompt` naming the button that picks it.
pub fn pick_path(
    prompt: &'static str,
    folder: bool,
    cx: &mut App,
) -> impl Future<Output = Option<PathBuf>> + use<> {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: !folder,
        directories: folder,
        multiple: false,
        prompt: Some(prompt.into()),
    });
    async move { picked.await.ok()?.ok()??.into_iter().next() }
}

/// Has the shell's root handle the project and worktree actions.
pub fn handle_actions(root: Div, cx: &mut Context<Shell>) -> Div {
    root.on_action(cx.listener(|this, _: &AddProject, window, cx| this.add_project(window, cx)))
        .on_action(cx.listener(|this, _: &NewWorktree, window, cx| {
            if let Some(project) = this.workspace.project() {
                new_worktree::open(cx.weak_entity(), project.path.clone(), window, cx);
            }
        }))
        .on_action(
            cx.listener(|_, NewWorktreeIn(project): &NewWorktreeIn, window, cx| {
                new_worktree::open(cx.weak_entity(), project.clone(), window, cx);
            }),
        )
        .on_action(
            cx.listener(|this, DeleteWorktree(path): &DeleteWorktree, window, cx| {
                if let Some(project) = this.workspace.project_of(path) {
                    let project = project.path.clone();
                    delete_worktree::open(cx.weak_entity(), project, path.clone(), window, cx);
                }
            }),
        )
        .on_action(
            cx.listener(|this, StopTerminals(path): &StopTerminals, window, cx| {
                this.stop_terminals_of(path, window, cx);
            }),
        )
        .on_action(
            cx.listener(|this, RemoveProject(path): &RemoveProject, _, cx| {
                this.remove_project(path, cx);
            }),
        )
        .on_action(
            cx.listener(|_, RevealInFinder(path): &RevealInFinder, _, cx| cx.reveal_path(path)),
        )
        .on_action(cx.listener(|_, CopyPath(path): &CopyPath, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(path.display().to_string()));
        }))
}

/// `path` as the user would type it, with `~` for their home folder.
pub fn display_path(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_owned)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// What git listed as a project's worktrees, numbered in the order the listings started.
pub struct WorktreeRead {
    number: u64,
    worktrees: koyo_git::Result<Vec<Worktree>>,
    /// The GitHub user the person's own branches are prefixed with there, if one's known.
    user: Option<String>,
}

/// Lists the worktrees of the project whose main checkout is at `path`. Call it off the main
/// thread.
pub fn read_worktrees(path: &Path) -> WorktreeRead {
    static STARTED: AtomicU64 = AtomicU64::new(0);
    // Numbered before git looks, so a listing that started earlier never wins over a later one.
    let number = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
    let repo = Repo::open(path);
    WorktreeRead {
        number,
        user: repo.as_ref().ok().and_then(user_prefix),
        worktrees: repo.and_then(|repo| repo.worktrees()),
    }
}

/// The GitHub user the person's own branches are prefixed with in `repo`, as a new worktree's
/// are (`BranchPrefix::GitHubUser`): looked up once a run, as git's config and the GitHub CLI's
/// it comes from hardly change, and asking them takes a few programs each time. Repositories
/// looked up at once don't wait on each other.
pub(crate) fn user_prefix(repo: &Repo) -> Option<String> {
    static KNOWN: LazyLock<Mutex<HashMap<PathBuf, Option<String>>>> = LazyLock::new(Mutex::default);
    if let Some(user) = KNOWN.lock().ok()?.get(repo.path()) {
        return user.clone();
    }
    let user = repo.user_branch_prefix();
    KNOWN
        .lock()
        .ok()?
        .insert(repo.path().to_owned(), user.clone());
    user
}

impl settings::BranchPrefix {
    /// The prefix the person's own branches get, `github_user` answering for their GitHub user;
    /// none when it's empty.
    pub fn own<'a>(&'a self, github_user: Option<&'a str>) -> Option<&'a str> {
        match self {
            settings::BranchPrefix::GitHubUser => github_user,
            settings::BranchPrefix::Custom(prefix) => Some(prefix.trim()),
            settings::BranchPrefix::None => None,
        }
        .filter(|prefix| !prefix.is_empty())
    }
}

impl Shell {
    /// Asks git for every project's worktrees again.
    pub fn reload_projects(&mut self, cx: &mut Context<Self>) {
        for path in self.workspace.paths() {
            self.reload_project(path, cx);
        }
    }

    /// Asks git for the worktrees of the project at `path` again.
    pub fn reload_project(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn({
                    let path = path.clone();
                    async move { read_worktrees(&path) }
                })
                .await;
            this.update(cx, |this, cx| this.show_worktrees(&path, read, cx))
                .ok();
        })
        .detach();
    }

    /// Puts what git reported for the project at `path` in the sidebar, unless the project
    /// was removed while git was busy or a listing that started later is already shown.
    pub fn show_worktrees(&mut self, path: &Path, read: WorktreeRead, cx: &mut Context<Self>) {
        let Some(project) = self.workspace.project_index(path) else {
            return;
        };
        let shown = &mut self.workspace.projects[project].read;
        if read.number <= *shown {
            return;
        }
        *shown = read.number;
        match read.worktrees {
            Ok(worktrees) => {
                self.workspace.projects[project].user = read.user.map(SharedString::from);
                self.set_worktrees(project, worktrees, cx);
                let checkouts: Vec<(PathBuf, PathBuf)> = (self.workspace.projects.iter())
                    .flat_map(|project| {
                        let main = &project.path;
                        project
                            .branches
                            .iter()
                            .map(|unit| (unit.worktree.path.clone(), main.clone()))
                    })
                    .collect();
                crate::history::know_checkouts(checkouts, cx);
                // The window's tabs first, where they were; then what the daemon and the
                // host keep that no pane of them shows.
                self.restore_window(cx);
                self.restore_terminals(cx);
                self.follow_chats(cx);
            }
            Err(error) => self.workspace.set_error(project, error.message),
        }
        cx.notify();
    }

    /// Takes what git now reports as the worktrees of the project at `project`, in the order
    /// the person put them in, and forgets the order and folding of those it no longer lists.
    fn set_worktrees(&mut self, project: usize, worktrees: Vec<Worktree>, cx: &mut Context<Self>) {
        let path = &self.workspace.projects[project].path;
        let order = settings::get(cx).worktree_order(path);
        self.workspace.set_worktrees(project, worktrees, order);
        let project = &self.workspace.projects[project];
        let listed = project.worktree_paths();
        if settings::get(cx).keeps_gone_worktrees(&project.path, &listed) {
            let path = project.path.clone();
            settings::update(cx, |settings| settings.keep_worktrees(&path, &listed));
        }
    }

    /// Moves the worktree of the project at `project` from `from` to where the one at `to` is,
    /// in both layouts' lists, and keeps the order for the next launch (`docs/arc-sidebar.md`
    /// 5, 9).
    pub fn move_worktree(
        &mut self,
        project: usize,
        from: usize,
        to: usize,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.move_worktree(project, from, to) {
            return;
        }
        let project = &self.workspace.projects[project];
        let (path, order) = (project.path.clone(), project.worktree_paths());
        settings::update(cx, |settings| settings.set_worktree_order(&path, order));
        cx.notify();
    }

    /// Asks for a folder and adds the repository it is in, by its main checkout.
    pub fn add_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = pick_folder("Add Project", cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(folder) = picked.await else {
                return;
            };
            let Ok(adding) = this.update(cx, |this, cx| this.add_project_at(folder, true, cx))
            else {
                return;
            };
            if let Err(error) = adding.await {
                this.update_in(cx, |_, window, cx| {
                    modal::alert("Couldn't add the project", error, window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// Adds the repository `folder` is in, by its main checkout, once git has listed its
    /// worktrees off the main thread, and selects it when `select`. Done with the main
    /// checkout's path, or with why it couldn't be added.
    pub fn add_project_at(
        &mut self,
        folder: PathBuf,
        select: bool,
        cx: &mut Context<Self>,
    ) -> Task<Result<PathBuf, String>> {
        cx.spawn(async move |this, cx| {
            let (path, worktrees) = cx
                .background_spawn(async move {
                    let repo = Repo::open(&folder)?;
                    let worktrees = repo.worktrees()?;
                    Ok::<_, koyo_git::GitError>((repo.path().to_owned(), worktrees))
                })
                .await
                .map_err(|error| error.message)?;
            this.update(cx, |this, cx| {
                let selected = this.workspace.active_project();
                let project = this.workspace.add_project(path.clone());
                if !select {
                    this.workspace.select_project(selected);
                }
                this.set_worktrees(project, worktrees, cx);
                this.save_projects(cx);
                cx.notify();
            })
            .map_err(|error| format!("The window closed: {error}"))?;
            Ok(path)
        })
    }

    /// Shows the worktree just created at `path` in the project at `project`, with a chat open
    /// on it.
    pub fn show_new_worktree(
        &mut self,
        project: &Path,
        path: &Path,
        read: WorktreeRead,
        cx: &mut Context<Self>,
    ) {
        self.show_worktrees(project, read, cx);
        let Some(project) = self.workspace.project_index(project) else {
            return;
        };
        if self.workspace.select_worktree(project, path) {
            self.new_tab(PaneKind::Chat, cx);
        }
    }

    /// Takes the project out of the sidebar. Its files and worktrees stay as they are.
    pub fn remove_project(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(project) = self.workspace.project_index(path) {
            self.workspace.remove_project(project);
            self.save_projects(cx);
            cx.notify();
        }
    }

    /// Saves the projects, a new one joining the space shown and one gone leaving its space.
    pub(crate) fn save_projects(&mut self, cx: &mut Context<Self>) {
        let paths = self.workspace.paths();
        settings::update(cx, |settings| {
            settings.projects = paths;
            settings.reconcile_spaces();
        });
    }
}
