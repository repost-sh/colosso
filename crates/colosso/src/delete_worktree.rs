//! Deleting a worktree. Git first says what deleting would lose: uncommitted changes, ignored
//! files, and commits that no branch, remote branch or tag holds. The dialog says so before
//! anything is deleted, and only offers to take the branch along by default when nothing goes
//! with it. Git checks again just before deleting, and refuses if more would go by then.

use std::path::{Path, PathBuf};

use colosso_git::{Head, Loss, Repo};
use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, WeakEntity, Window, div, px,
};

use crate::modal::{self, Button, Tone};
use crate::projects::{display_path, read_worktrees};
use crate::shell::{Shell, icon};
use crate::theme;

const WIDTH: f32 = 420.;
/// How many changed files the dialog names before summing up the rest.
const LISTED_CHANGES: usize = 6;
/// How many ignored files and folders it names.
const LISTED_IGNORED: usize = 3;
/// Every line in the dialog is one of these high, so its height is known before it opens.
const LINE: f32 = 20.;
const SMALL_LINE: f32 = 18.;
const GAP: f32 = 16.;

/// Asks git what deleting the worktree at `path`, of the project at `project`, would lose,
/// then asks the user over `window`.
pub fn open(
    shell: WeakEntity<Shell>,
    project: PathBuf,
    path: PathBuf,
    window: &mut Window,
    cx: &mut App,
) {
    ask(shell, project, path, None, window.window_handle(), cx);
}

/// Inspects the worktree and opens the dialog over `parent`, saying `notice` if set.
fn ask(
    shell: WeakEntity<Shell>,
    project: PathBuf,
    path: PathBuf,
    notice: Option<SharedString>,
    parent: AnyWindowHandle,
    cx: &mut App,
) {
    let inspect = cx.background_spawn({
        let (project, path) = (project.clone(), path.clone());
        async move { Repo::open(&project)?.inspect_removal(&path) }
    });
    cx.spawn(async move |cx| {
        let inspected = inspect.await;
        parent
            .update(cx, |_, window, cx| match inspected {
                Ok(loss) => {
                    let dialog = DeleteWorktreeDialog {
                        // Only a branch made with its worktree goes with it by default, and
                        // never one holding commits that aren't anywhere else.
                        delete_branch: loss.made_here && loss.unmerged == 0,
                        shell,
                        project,
                        path,
                        loss,
                        parent,
                        deleting: false,
                        error: notice,
                    };
                    modal::open(WIDTH, dialog.height(), window, cx, |_, _| dialog);
                }
                Err(error) => modal::alert("Couldn't read the worktree", error.message, window, cx),
            })
            .ok();
    })
    .detach();
}

struct DeleteWorktreeDialog {
    shell: WeakEntity<Shell>,
    project: PathBuf,
    path: PathBuf,
    /// What git said deleting loses, which is what deleting is held to.
    loss: Loss,
    /// The window the dialog sits over, which hears how deleting went even if the dialog was
    /// closed meanwhile.
    parent: AnyWindowHandle,
    delete_branch: bool,
    deleting: bool,
    error: Option<SharedString>,
}

impl DeleteWorktreeDialog {
    /// Commits that go with the worktree itself, which only a detached one has.
    fn lost_commits(&self) -> usize {
        match self.loss.head {
            Head::Detached(_) => self.loss.unmerged,
            Head::Branch(_) => 0,
        }
    }

    fn listed_changes(&self) -> usize {
        self.loss.changes.len().min(LISTED_CHANGES)
    }

    fn unlisted_changes(&self) -> usize {
        self.loss.changes.len() - self.listed_changes()
    }

    /// The window height that fits what the dialog says.
    fn height(&self) -> f32 {
        let title = LINE + SMALL_LINE;
        let changes = match self.loss.changes.len() {
            0 => LINE,
            _ => {
                let lines = self.listed_changes() + usize::from(self.unlisted_changes() > 0);
                26. + LINE + 4. + lines as f32 * SMALL_LINE
            }
        };
        let commits = if self.lost_commits() > 0 {
            8. + LINE
        } else {
            0.
        };
        let ignored = if self.loss.ignored.is_empty() {
            0.
        } else {
            8. + SMALL_LINE
        };
        let branch = match self.loss.head.branch() {
            Some(_) => GAP + LINE + SMALL_LINE,
            None => 0.,
        };
        2. * modal::PADDING
            + title
            + GAP
            + changes
            + commits
            + ignored
            + branch
            + GAP
            + 4.
            + modal::FOOTER
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.deleting {
            return;
        }
        self.deleting = true;
        self.error = None;
        cx.notify();
        let (project, path, loss) = (self.project.clone(), self.path.clone(), self.loss.clone());
        let (shell, parent, delete_branch) = (self.shell.clone(), self.parent, self.delete_branch);
        cx.spawn_in(window, async move |this, cx| {
            let (deleted, read, still_there) = cx
                .background_spawn({
                    let (project, path) = (project.clone(), path.clone());
                    async move {
                        let deleted = Repo::open(&project)
                            .and_then(|repo| repo.remove_worktree(&path, &loss, delete_branch));
                        // The worktree may be gone even if its branch couldn't go, so the
                        // shell looks again whatever happened.
                        (deleted, read_worktrees(&project), path.exists())
                    }
                })
                .await;
            shell
                .update(cx, |shell, cx| shell.show_worktrees(&project, read, cx))
                .ok();
            this.update_in(cx, |_, window, cx| modal::dismiss(window, cx))
                .ok();
            let Err(error) = deleted else {
                return;
            };
            // Told over the window behind: the dialog's is gone, and the async context with it.
            parent
                .update(cx, |_, window, cx| {
                    if still_there {
                        // Asked again with what git says now, and why it stopped.
                        ask(shell, project, path, Some(error.message.into()), parent, cx);
                    } else {
                        modal::alert(
                            "The worktree is gone, its branch isn't",
                            error.message,
                            window,
                            cx,
                        );
                    }
                })
                .ok();
        })
        .detach();
    }

    fn changes(&self, p: theme::Palette) -> impl IntoElement {
        let count = self.loss.changes.len();
        if count == 0 {
            return div()
                .h(px(LINE))
                .flex()
                .items_center()
                .gap_2()
                .text_color(p.muted)
                .child(icon(IconName::CircleCheck, 14.))
                .child("Nothing uncommitted is lost.");
        }
        let unlisted = self.unlisted_changes();
        div()
            .p(px(12.))
            .flex()
            .flex_col()
            .rounded(px(8.))
            .border_1()
            .border_color(p.hairline)
            .bg(p.raised)
            .child(warning(p).mb(px(4.)).child(match count {
                1 => "1 uncommitted change is lost".to_owned(),
                count => format!("{count} uncommitted changes are lost"),
            }))
            .children(self.loss.changes.iter().take(LISTED_CHANGES).map(|file| {
                small_line()
                    .pl(px(22.))
                    .font_family(theme::MONO)
                    .text_color(p.muted)
                    .child(file.clone())
            }))
            .when(unlisted > 0, |list| {
                list.child(
                    small_line()
                        .pl(px(22.))
                        .text_color(p.muted)
                        .child(format!("and {unlisted} more")),
                )
            })
    }

    /// Names the ignored files that go with the folder, which git's status leaves out.
    fn ignored(&self, p: theme::Palette) -> impl IntoElement {
        let ignored = &self.loss.ignored;
        let mut names = ignored
            .iter()
            .take(LISTED_IGNORED)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if ignored.len() > LISTED_IGNORED {
            names.push_str(&format!(" and {} more", ignored.len() - LISTED_IGNORED));
        }
        small_line()
            .gap_2()
            .text_color(p.muted)
            .child(icon(IconName::EyeOff, 12.))
            .child(format!("Ignored files go too: {names}"))
    }

    fn branch_switch(
        &self,
        branch: &str,
        p: theme::Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (note, warn) = match (self.loss.unmerged, self.loss.made_here) {
            (0, true) => (
                "Every commit on it is on another branch too.".to_owned(),
                false,
            ),
            (0, false) => ("It was here before the worktree.".to_owned(), false),
            (1, _) => ("Its last commit is on no other branch.".to_owned(), true),
            (count, _) => (
                format!("{count} of its commits are on no other branch."),
                true,
            ),
        };
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .h(px(LINE))
                            .flex()
                            .items_center()
                            .gap_1()
                            .child("Delete branch")
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(theme::MONO)
                                    .child(branch.to_owned()),
                            ),
                    )
                    .child(
                        small_line()
                            .text_color(if warn && self.delete_branch {
                                p.danger
                            } else {
                                p.muted
                            })
                            .child(note),
                    ),
            )
            .child(
                Switch::new("delete-worktree-branch")
                    .checked(self.delete_branch)
                    .color(if warn { p.danger } else { p.accent })
                    .disabled(self.deleting)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.delete_branch = *checked;
                        cx.notify();
                    })),
            )
    }
}

fn warning(p: theme::Palette) -> gpui_kit::Div {
    div()
        .h(px(LINE))
        .flex()
        .items_center()
        .gap_2()
        .text_color(p.danger)
        .child(icon(IconName::TriangleAlert, 14.))
}

fn small_line() -> gpui_kit::Div {
    div()
        .h(px(SMALL_LINE))
        .flex()
        .items_center()
        .text_xs()
        .truncate()
}

/// The folder's name, for the title.
fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Render for DeleteWorktreeDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let lost_commits = self.lost_commits();
        let branch = self.loss.head.branch().map(str::to_owned);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(LINE))
                    .flex()
                    .items_center()
                    .child(modal::heading(format!(
                        "Delete {}?",
                        folder_name(&self.path)
                    ))),
            )
            .child(
                // Cut from the start, so the folder's own name stays in view.
                div()
                    .h(px(SMALL_LINE))
                    .line_height(px(SMALL_LINE))
                    .text_xs()
                    .font_family(theme::MONO)
                    .text_color(p.muted)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_start()
                    .child(display_path(&self.path)),
            )
            .child(div().mt(px(GAP)).child(self.changes(p)))
            .when(lost_commits > 0, |dialog| {
                dialog.child(warning(p).mt(px(8.)).child(match lost_commits {
                    1 => "1 commit on no branch is lost".to_owned(),
                    count => format!("{count} commits on no branch are lost"),
                }))
            })
            .when(!self.loss.ignored.is_empty(), |dialog| {
                dialog.child(div().mt(px(8.)).child(self.ignored(p)))
            })
            .when_some(branch, |dialog, branch| {
                dialog.child(div().mt(px(GAP)).child(self.branch_switch(&branch, p, cx)))
            })
            .child(div().flex_1())
            .child(
                modal::footer(self.error.clone(), p)
                    .child(modal::cancel("delete-worktree-cancel"))
                    .child(
                        Button::new("delete-worktree-confirm", "Delete", Tone::Danger)
                            .busy(self.deleting)
                            .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
                    ),
            )
    }
}
