//! What the Arc sidebar does (`docs/arc-sidebar.md`): its actions, which its buttons and menus
//! dispatch and the window's root hears, and the shell's side of spaces: showing one, following
//! the project selected into its space, and moving projects among them.

use std::path::{Path, PathBuf};

use gpui_kit::{Action, Context, Div, InteractiveElement as _, MouseDownEvent, Window, actions};
use koyo_browser_ipc::Go;

use super::{clear, delete_space, rename};
use crate::browser::BrowserView;
use crate::chat::Activity;
use crate::navigation::Route;
use crate::settings;
use crate::shell::Shell;

actions!(
    arc,
    [
        PageBack,
        PageForward,
        PageReload,
        NewSpace,
        RenameSpace,
        DeleteSpace
    ]
);

/// Moves the project whose main checkout is at the path to the space at the place given.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = arc, no_json)]
pub struct MoveProject(pub PathBuf, pub usize);

/// Makes a space and moves the project whose main checkout is at the path into it.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = arc, no_json)]
pub struct MoveProjectToNewSpace(pub PathBuf);

/// Asks what to call the project in Koyo.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = arc, no_json)]
pub struct RenameProject(pub PathBuf);

/// Asks for a picture to show for the project.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = arc, no_json)]
pub struct ChangeProjectIcon(pub PathBuf);

/// Shows the git host's avatar for the project again.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = arc, no_json)]
pub struct ResetProjectIcon(pub PathBuf);

/// Asks to close every tab of the checkout at the path.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = arc, no_json)]
pub struct ClearWorktree(pub PathBuf);

/// Has the window's root handle the Arc sidebar's actions.
pub fn handle_actions(root: Div, cx: &mut Context<Shell>) -> Div {
    root.on_action(cx.listener(|this, _: &PageBack, _, cx| this.steer_page(Go::Back, cx)))
        .on_action(cx.listener(|this, _: &PageForward, _, cx| this.steer_page(Go::Forward, cx)))
        .on_action(cx.listener(|this, _: &PageReload, _, cx| {
            if let Some(page) = this.shown_page(cx) {
                page.update(cx, BrowserView::reload_or_stop);
            }
        }))
        // The new space comes in as a swipe would bring it.
        .on_action(cx.listener(|this, _: &NewSpace, _, cx| {
            let mut made = 0;
            settings::update(cx, |settings| made = settings.add_space());
            this.slide_to_space(made, cx);
        }))
        .on_action(
            cx.listener(|this, MoveProject(project, to): &MoveProject, _, cx| {
                this.move_project(project, Some(*to), cx);
            }),
        )
        .on_action(cx.listener(
            |this, MoveProjectToNewSpace(project): &MoveProjectToNewSpace, _, cx| {
                this.move_project(project, None, cx);
            },
        ))
        .on_action(
            cx.listener(|this, RenameProject(project): &RenameProject, window, cx| {
                this.ask_project_name(project.clone(), window, cx);
            }),
        )
        .on_action(cx.listener(
            |this, ChangeProjectIcon(project): &ChangeProjectIcon, window, cx| {
                this.pick_project_icon(project.clone(), window, cx);
            },
        ))
        .on_action(
            cx.listener(|_, ResetProjectIcon(project): &ResetProjectIcon, _, cx| {
                settings::update(cx, |settings| settings.set_project_icon(project, None));
            }),
        )
        .on_action(cx.listener(|_, _: &RenameSpace, window, cx| {
            let Some(name) = settings::get(cx)
                .shown_space()
                .map(|space| space.name.clone())
            else {
                return;
            };
            rename::open(
                "Rename Space",
                name,
                "Space name",
                window,
                cx,
                |name, cx| {
                    // A space keeps its name rather than go without one.
                    if name.is_empty() {
                        return;
                    }
                    settings::update(cx, |settings| {
                        if let Some(space) = settings.shown_space_mut() {
                            space.name = name;
                        }
                    });
                },
            );
        }))
        .on_action(cx.listener(|_, _: &DeleteSpace, window, cx| {
            delete_space::open(cx.weak_entity(), window, cx);
        }))
        .on_action(
            cx.listener(|this, ClearWorktree(path): &ClearWorktree, window, cx| {
                let Some((project, branch)) = this.workspace.unit_at(path) else {
                    return;
                };
                let unit = &this.workspace.projects[project].branches[branch];
                let (tabs, name) = (unit.tabs.len(), unit.name());
                clear::open(cx.weak_entity(), path.clone(), &name, tabs, window, cx);
            }),
        )
}

impl Shell {
    /// The projects of the space the Arc sidebar shows, by their place in the workspace, in the
    /// order its tiles show them. Every project is in a space (`Settings::reconcile_spaces`).
    pub fn space_projects(&self, cx: &gpui_kit::App) -> Vec<usize> {
        let Some(space) = settings::get(cx).shown_space() else {
            return Vec::new();
        };
        (space.projects.iter())
            .filter_map(|path| self.workspace.project_index(path))
            .collect()
    }

    /// How the space at `index` is doing, the most urgent of its projects: what its mark at the
    /// sidebar's foot says while another space is shown (`docs/arc-sidebar.md` 7), from what
    /// the tiles of its projects would say were it shown.
    pub fn space_activity(&self, index: usize, cx: &gpui_kit::App) -> Activity {
        let Some(space) = settings::get(cx).spaces.get(index) else {
            return Activity::Quiet;
        };
        (space.projects.iter())
            .filter_map(|path| self.workspace.project_index(path))
            .map(|project| self.project_activity(&self.workspace.projects[project], cx))
            .max()
            .unwrap_or(Activity::Quiet)
    }

    /// Shows space `index`, and the project it showed last, its first otherwise. An empty space
    /// shows no project, so nothing of another space's shows, or is acted on, either.
    pub fn show_space(&mut self, index: usize, cx: &mut Context<Self>) {
        settings::update(cx, |settings| {
            settings.space = index.min(settings.spaces.len().saturating_sub(1));
        });
        let space = settings::get(cx).shown_space();
        let target = space.and_then(|space| {
            (space.selected.as_deref()).or_else(|| space.projects.first().map(PathBuf::as_path))
        });
        let index = target.and_then(|path| self.workspace.project_index(path));
        self.workspace.select_project(index);
        self.navigate(Route::Workspace, cx);
        cx.notify();
    }

    /// Selects the project at `index` from its tile.
    pub fn select_project(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.workspace.projects.len() {
            self.workspace.select_project(Some(index));
            self.navigate(Route::Workspace, cx);
            cx.notify();
        }
    }

    /// Follows the project selected, however it was: into its space if it's in another, and as
    /// the project its space shows. An empty space shown has no project to follow, so it stays
    /// shown.
    pub(crate) fn follow_space(&mut self, cx: &mut Context<Self>) {
        let Some(active) = self.workspace.project().map(|project| &project.path) else {
            return;
        };
        let settings = settings::get(cx);
        let Some(space) = settings.space_of(active) else {
            return;
        };
        let followed = settings.space == space
            && settings.spaces[space].selected.as_deref() == Some(active.as_path());
        if followed {
            return;
        }
        let active = active.clone();
        settings::update(cx, |settings| {
            settings.space = space;
            settings.spaces[space].selected = Some(active);
        });
    }

    /// Moves `project` to the space at `to`, or a new one, and shows it there.
    fn move_project(&mut self, project: &Path, to: Option<usize>, cx: &mut Context<Self>) {
        settings::update(cx, |settings| {
            let to = to.unwrap_or_else(|| settings.add_space());
            settings.move_project(project, to);
            if let Some(space) = settings.spaces.get_mut(to) {
                space.selected = Some(project.to_owned());
            }
        });
        if let Some(space) = settings::get(cx).space_of(project) {
            self.show_space(space, cx);
        }
    }

    /// Takes `projects` out of Koyo, as a deleted space's are.
    pub(crate) fn remove_projects(&mut self, projects: &[PathBuf], cx: &mut Context<Self>) {
        for project in projects {
            if let Some(index) = self.workspace.project_index(project) {
                self.workspace.remove_project(index);
            }
        }
        self.save_projects(cx);
        let shown = settings::get(cx).space;
        self.show_space(shown, cx);
    }

    fn ask_project_name(&mut self, project: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = (self.workspace.project_index(&project))
            .map(|index| self.workspace.projects[index].name.to_string())
        else {
            return;
        };
        // The shell names its projects anew as the settings change.
        rename::open(
            "Rename Project",
            name,
            "Name in Koyo",
            window,
            cx,
            move |name, cx| {
                settings::update(cx, |settings| settings.rename_project(&project, &name));
            },
        );
    }

    fn pick_project_icon(&mut self, project: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let picked = crate::projects::pick_path("Use as Icon", false, cx);
        cx.spawn_in(window, async move |_, cx| {
            let Some(picture) = picked.await else {
                return;
            };
            cx.update(|_, cx| {
                settings::update(cx, |settings| {
                    settings.set_project_icon(&project, Some(picture));
                });
            })
            .ok();
        })
        .detach();
    }

    /// The space menu (`docs/arc-sidebar.md` 6), where the sidebar was right-clicked.
    pub(super) fn show_space_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        crate::picker::close(cx);
        // Edit Theme Color opens the picker where the menu was.
        self.rails_menu_at = event.position;
        let last = settings::get(cx).spaces.len() <= 1;
        gpui_kit::component::native_menu::NativeMenu::new()
            .menu("Rename Space…", Box::new(RenameSpace))
            .menu("Edit Theme Color…", Box::new(crate::shell::EditTheme))
            .separator()
            .menu_with_disabled("Delete Space", last, Box::new(DeleteSpace))
            .show(event.position, window, cx);
    }
}
