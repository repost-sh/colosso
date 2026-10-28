//! Keeps the workspace in step with git: reads each project's worktrees off the main thread,
//! adds and removes projects, and saves the list. Git is asked again on launch, whenever the
//! window comes back to the front, and after the app changes a project's worktrees itself.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use colosso_git::{Repo, Worktree};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{
    Action, App, AppContext as _, ClipboardItem, Context, Div, InteractiveElement, MouseButton,
    MouseDownEvent, PathPromptOptions, Window, actions,
};

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

/// Has `element` open the menu `build` makes where it is right-clicked.
pub fn with_menu<E: InteractiveElement>(element: E, build: impl Fn() -> NativeMenu + 'static) -> E {
    element.on_mouse_down(
        MouseButton::Right,
        move |event: &MouseDownEvent, window, cx| {
            // The rails have a menu of their own.
            cx.stop_propagation();
            build().show(event.position, window, cx);
        },
    )
}

/// The menu a project's row opens, for its main checkout at `path`.
pub fn project_menu(path: &Path) -> NativeMenu {
    let menu = NativeMenu::new()
        .menu("New Worktree…", Box::new(NewWorktreeIn(path.to_owned())))
        .separator();
    path_items(menu, path)
        .separator()
        .menu("Remove Project", Box::new(RemoveProject(path.to_owned())))
}

/// The menu a checkout's row opens. Git can't delete a main checkout.
pub fn worktree_menu(path: &Path, is_main: bool) -> NativeMenu {
    let menu = path_items(NativeMenu::new(), path);
    if is_main {
        return menu;
    }
    menu.separator().menu(
        "Delete Worktree…",
        Box::new(DeleteWorktree(path.to_owned())),
    )
}

/// What any folder's menu offers.
fn path_items(menu: NativeMenu, path: &Path) -> NativeMenu {
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
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
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
                new_worktree::open(cx.weak_entity(), project.path.clone(), window, cx)
            }
        }))
        .on_action(
            cx.listener(|_, NewWorktreeIn(project): &NewWorktreeIn, window, cx| {
                new_worktree::open(cx.weak_entity(), project.clone(), window, cx)
            }),
        )
        .on_action(
            cx.listener(|this, DeleteWorktree(path): &DeleteWorktree, window, cx| {
                if let Some(project) = this.workspace.project_of(path) {
                    let project = project.path.clone();
                    delete_worktree::open(cx.weak_entity(), project, path.clone(), window, cx)
                }
            }),
        )
        .on_action(
            cx.listener(|this, RemoveProject(path): &RemoveProject, _, cx| {
                this.remove_project(path, cx)
            }),
        )
        .on_action(
            cx.listener(|_, RevealInFinder(path): &RevealInFinder, _, cx| cx.reveal_path(path)),
        )
        .on_action(cx.listener(|_, CopyPath(path): &CopyPath, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(path.display().to_string()))
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
    worktrees: colosso_git::Result<Vec<Worktree>>,
}

/// Lists the worktrees of the project whose main checkout is at `path`. Call it off the main
/// thread.
pub fn read_worktrees(path: &Path) -> WorktreeRead {
    static STARTED: AtomicU64 = AtomicU64::new(0);
    // Numbered before git looks, so a listing that started earlier never wins over a later one.
    let number = STARTED.fetch_add(1, Ordering::Relaxed) + 1;
    WorktreeRead {
        number,
        worktrees: Repo::open(path).and_then(|repo| repo.worktrees()),
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
            Ok(worktrees) => self.workspace.set_worktrees(project, worktrees),
            Err(error) => self.workspace.set_error(project, error.message),
        }
        cx.notify();
    }

    /// Asks for a folder and adds the repository it is in, by its main checkout.
    pub fn add_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = pick_folder("Add Project", cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(folder) = picked.await else {
                return;
            };
            let opened = cx
                .background_spawn(async move {
                    let repo = Repo::open(&folder)?;
                    let worktrees = repo.worktrees()?;
                    Ok::<_, colosso_git::Error>((repo.path().to_owned(), worktrees))
                })
                .await;
            this.update_in(cx, |this, window, cx| match opened {
                Ok((path, worktrees)) => {
                    let project = this.workspace.add_project(path);
                    this.workspace.set_worktrees(project, worktrees);
                    this.save_projects(cx);
                    cx.notify();
                }
                Err(error) => modal::alert("Couldn't add the project", error.message, window, cx),
            })
            .ok();
        })
        .detach();
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

    fn save_projects(&self, cx: &mut Context<Self>) {
        let paths = self.workspace.paths();
        settings::update(cx, |settings| settings.projects = paths);
    }
}
