//! The New Worktree dialog: a name, the branch to start from, and whether to work on that
//! branch itself rather than start a new one from it. Underneath the name it shows the branch
//! and folder creating would make. Git lists the branches and creates the worktree off the main
//! thread; the dialog stays open with git's message if creating fails.

use std::path::PathBuf;

use colosso_git::{NewWorktree, Ref, Repo, Start};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Focusable as _, ScrollHandle,
    SharedString, Subscription, WeakEntity, Window,
};

use crate::modal;
use crate::projects::read_worktrees;
use crate::settings::{self, BranchPrefix};
use crate::shell::Shell;

mod render;

const WIDTH: f32 = 480.;
const HEIGHT: f32 = 520.;

/// Opens the dialog for a new worktree of the project whose main checkout is `project`.
pub fn open(shell: WeakEntity<Shell>, project: PathBuf, window: &mut Window, cx: &mut App) {
    let behind = window.window_handle();
    modal::open(WIDTH, HEIGHT, window, cx, |window, cx| {
        NewWorktreeDialog::new(shell, project, behind, window, cx)
    });
}

/// What git said about the project when the dialog opened.
struct Project {
    repo: Repo,
    /// What new branches go under, if anything.
    prefix: Option<String>,
    /// Newest first.
    branches: Vec<Ref>,
    /// The branch new work starts from unless the user picks another.
    default: Option<Ref>,
}

struct NewWorktreeDialog {
    shell: WeakEntity<Shell>,
    project_path: PathBuf,
    project_name: SharedString,
    /// The folder this project's worktrees go in.
    parent: PathBuf,
    /// The window the dialog sits over, which hears if creating fails after the dialog was
    /// closed.
    behind: AnyWindowHandle,
    name: Entity<InputState>,
    filter: Entity<InputState>,
    /// The branch to start from, or to work on.
    picked: Option<Ref>,
    /// Work on the picked branch instead of starting a new one from it.
    existing: bool,
    branch_list: ScrollHandle,
    /// Unset until git has listed the branches.
    project: Option<Project>,
    creating: bool,
    error: Option<SharedString>,
    _subscriptions: [Subscription; 3],
}

impl NewWorktreeDialog {
    fn new(
        shell: WeakEntity<Shell>,
        project_path: PathBuf,
        behind: AnyWindowHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let project_name: SharedString = project_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
            .into();
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("What's it for?"));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter branches"));
        name.focus_handle(cx).focus(window, cx);
        let dialog_window = window.window_handle();
        let this = cx.entity().downgrade();
        let _subscriptions = [
            cx.subscribe_in(&name, window, Self::input_event),
            cx.subscribe_in(&filter, window, |this, filter, event, window, cx| {
                if let InputEvent::Change = event {
                    this.filtered(cx);
                }
                Self::input_event(this, filter, event, window, cx);
            }),
            // Up and down move through the branches from either field.
            cx.intercept_keystrokes(move |event, window, cx| {
                let keystroke = &event.keystroke;
                let step = match keystroke.key.as_str() {
                    "up" => -1,
                    "down" => 1,
                    _ => return,
                };
                if window.window_handle() != dialog_window || keystroke.modifiers.modified() {
                    return;
                }
                cx.stop_propagation();
                this.update(cx, |this, cx| this.step(step, cx)).ok();
            }),
        ];
        let prefix = settings::get(cx).branch_prefix.clone();
        let parent = settings::get(cx).worktrees_dir(&project_name);
        let load = {
            let project_path = project_path.clone();
            cx.background_spawn(async move {
                let repo = Repo::open(&project_path)?;
                let branches = repo.branches()?;
                let default = repo.default_base(&branches);
                let prefix = match prefix {
                    BranchPrefix::GitHubUser => repo.user_branch_prefix(),
                    BranchPrefix::Custom(prefix) => Some(prefix.trim().to_owned()),
                    BranchPrefix::None => None,
                }
                .filter(|prefix| !prefix.is_empty());
                Ok::<_, colosso_git::GitError>(Project {
                    repo,
                    prefix,
                    branches,
                    default,
                })
            })
        };
        cx.spawn(async move |this, cx| {
            let loaded = load.await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok(project) => {
                        this.picked = project
                            .default
                            .clone()
                            .or(project.branches.first().cloned());
                        this.project = Some(project);
                        this.reveal_picked(cx);
                    }
                    Err(error) => {
                        modal::fail(&mut this.error, "read the branches", error.message);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        NewWorktreeDialog {
            shell,
            project_path,
            project_name,
            parent,
            behind,
            name,
            filter,
            picked: None,
            existing: false,
            branch_list: ScrollHandle::new(),
            project: None,
            creating: false,
            error: None,
            _subscriptions,
        }
    }

    fn input_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => cx.notify(),
            InputEvent::PressEnter { .. } => self.create(window, cx),
            _ => {}
        }
    }

    /// The branches the filter leaves, in the order git listed them.
    fn shown(&self, cx: &App) -> Vec<&Ref> {
        let filter = self.filter.read(cx).value().trim().to_lowercase();
        let Some(project) = &self.project else {
            return Vec::new();
        };
        project
            .branches
            .iter()
            .filter(|branch| branch.to_string().to_lowercase().contains(&filter))
            .collect()
    }

    /// Keeps a branch picked as the filter changes: the picked one while it's shown, the first
    /// one shown otherwise.
    fn filtered(&mut self, cx: &mut Context<Self>) {
        let shown = self.shown(cx);
        let hidden = self
            .picked
            .as_ref()
            .is_none_or(|picked| !shown.contains(&picked));
        if hidden && let Some(first) = shown.first() {
            self.picked = Some((*first).clone());
        }
        self.reveal_picked(cx);
    }

    /// Picks the branch `step` places down the list from the picked one.
    fn step(&mut self, step: isize, cx: &mut Context<Self>) {
        let shown = self.shown(cx);
        let Some(last) = shown.len().checked_sub(1) else {
            return;
        };
        let next = match self.picked_index(&shown) {
            Some(current) => current.saturating_add_signed(step).min(last),
            None => 0,
        };
        self.picked = Some(shown[next].clone());
        self.branch_list.scroll_to_item(next);
        cx.notify();
    }

    /// Where the picked branch is among those `shown`.
    fn picked_index(&self, shown: &[&Ref]) -> Option<usize> {
        let picked = self.picked.as_ref()?;
        shown.iter().position(|branch| *branch == picked)
    }

    fn reveal_picked(&self, cx: &App) {
        if let Some(index) = self.picked_index(&self.shown(cx)) {
            self.branch_list.scroll_to_item(index);
        }
    }

    /// The request the dialog's fields make, if they make one.
    fn request(&self, cx: &App) -> Option<NewWorktree> {
        let project = self.project.as_ref()?;
        let branch = self.picked.clone()?;
        let start = if self.existing {
            Start::Existing(branch)
        } else {
            Start::NewBranch {
                base: branch,
                prefix: project.prefix.clone(),
            }
        };
        Some(NewWorktree {
            name: self.name.read(cx).value().to_string(),
            start,
            parent: self.parent.clone(),
        })
    }

    /// The request creating would make now, if the dialog is ready to make one.
    fn ready(&self, cx: &App) -> Option<NewWorktree> {
        self.request(cx)
            .filter(|request| !self.creating && !request.slug().is_empty())
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(request), Some(project)) = (self.ready(cx), self.project.as_ref()) else {
            return;
        };
        let repo = project.repo.clone();
        let project_path = self.project_path.clone();
        let (shell, behind) = (self.shell.clone(), self.behind);
        self.creating = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let created = cx
                .background_spawn(async move {
                    let worktree = repo.create_worktree(&request)?;
                    Ok::<_, colosso_git::GitError>((worktree.path, read_worktrees(repo.path())))
                })
                .await;
            // The shell hears about the worktree even if the dialog was closed meanwhile.
            let error = match created {
                Ok((path, read)) => {
                    shell
                        .update(cx, |shell, cx| {
                            shell.show_new_worktree(&project_path, &path, read, cx);
                        })
                        .ok();
                    this.update_in(cx, |_, window, cx| modal::dismiss(window, cx))
                        .ok();
                    return;
                }
                Err(error) => error,
            };
            let shown = this.update(cx, |this, cx| {
                this.creating = false;
                modal::fail(
                    &mut this.error,
                    "create the worktree",
                    error.message.clone(),
                );
                cx.notify();
            });
            // With the dialog closed meanwhile, the window behind says what went wrong.
            if shown.is_err() {
                behind
                    .update(cx, |_, window, cx| {
                        modal::alert("Couldn't create the worktree", error.message, window, cx);
                    })
                    .ok();
            }
        })
        .detach();
    }
}
