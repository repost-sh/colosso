//! The spaces as the shell shows them (`docs/layout.md`, Spaces; `docs/sidebar.md` 4, 6): the
//! actions their marks, menus and keys dispatch, which the window's root hears, and the keys that
//! move between spaces, projects and tabs; showing a space, following the project selected into
//! its space, and moving projects among them. The swipe between spaces is `swipe.rs`, ⌥↑ and ⌥↓
//! `tab_steps.rs`, and the dialog that deletes one `delete.rs`; one's renamed in the dialog that
//! asks for a name (`modal::rename`).

use std::path::{Path, PathBuf};

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{
    Action, App, Context, Div, InteractiveElement as _, KeyBinding, Window, actions,
};

use crate::chat::Activity;
use crate::modal::rename;
use crate::projects::{MoveProject, MoveProjectToNewSpace};
use crate::settings;
use crate::shell::Shell;
use crate::spaces::Space;

pub(super) mod delete;
mod swipe;
mod tab_steps;

pub use swipe::Swipe;

actions!(spaces, [NewSpace, PreviousSpace, NextSpace]);
actions!(projects, [PreviousProject, NextProject]);
actions!(tabs, [PreviousTab, NextTab]);

/// The shell root's key context in the workspace, wherever the Settings tab isn't shown.
pub const SPACES: &str = "Spaces";

/// Where the ⌥ arrows move between projects, spaces and tabs: the spaces' context, but not where
/// the keyboard is in text. A text field and a terminal keep them, as a word back and on (⌥←,
/// ⌥→), a word's selection (⌥⇧←, ⌥⇧→) or their own (⌥↑ and ⌥↓, which no text field of Koyo's
/// binds, so that a press meant for the text never changes the tab); a terminal hands them to
/// its program. A web page, an app on the agent's desktop and a phone or simulator may have a
/// text field Koyo can't see into, so there they stay keys, which the view's key handler hands
/// on.
const OUTSIDE_TEXT: &str =
    "Spaces && !Input && !Terminal && !BrowserPage && !AgentDesktop && !Device";

/// ⌥← and ⌥→ show the project to the left and to the right, on into the space beside past the
/// first or last of the space's (`docs/sidebar.md` 6), and ⌥⇧← and ⌥⇧→ the space to the left
/// and to the right, wherever the keyboard isn't in text; ⌘⌥← and ⌘⌥→, and ⌘⌥⇧← and ⌘⌥⇧→, the
/// same from wherever it is, the chat's composer included, as nothing under the shell binds
/// them. ⌥↑ and ⌥↓ show the tab before and after in the order they're listed where the keyboard
/// isn't in text; there's no ⌘ for them, as a text field's ⌘⌥↑ and ⌘⌥↓ add a cursor. Bound to
/// the spaces' context, so they're unbound while the Settings tab is shown. ⌥← and ⌥→, and ⌥⇧←
/// and ⌥⇧→, last, so the shortcuts page shows them.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-alt-left", PreviousProject, Some(SPACES)),
        KeyBinding::new("cmd-alt-right", NextProject, Some(SPACES)),
        KeyBinding::new("alt-left", PreviousProject, Some(OUTSIDE_TEXT)),
        KeyBinding::new("alt-right", NextProject, Some(OUTSIDE_TEXT)),
        KeyBinding::new("cmd-alt-shift-left", PreviousSpace, Some(SPACES)),
        KeyBinding::new("cmd-alt-shift-right", NextSpace, Some(SPACES)),
        KeyBinding::new("alt-shift-left", PreviousSpace, Some(OUTSIDE_TEXT)),
        KeyBinding::new("alt-shift-right", NextSpace, Some(OUTSIDE_TEXT)),
        KeyBinding::new("alt-up", PreviousTab, Some(OUTSIDE_TEXT)),
        KeyBinding::new("alt-down", NextTab, Some(OUTSIDE_TEXT)),
    ]);
}

/// Asks what to call the space with the id given.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = spaces, no_json)]
pub struct RenameSpace(pub u64);

/// Asks about deleting the space with the id given, unless it's the only one.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = spaces, no_json)]
pub struct DeleteSpace(pub u64);

/// Has the window's root handle the spaces' actions: what their marks, menus and keys dispatch.
pub fn handle_actions(root: Div, cx: &mut Context<Shell>) -> Div {
    root.on_action(cx.listener(|this, _: &NewSpace, _, cx| {
            let mut made = 0;
            settings::update(cx, |settings| made = settings.add_space());
            this.slide_to_space(made, cx);
        }))
        .on_action(cx.listener(|this, _: &PreviousSpace, _, cx| this.step_space(-1, cx)))
        .on_action(cx.listener(|this, _: &NextSpace, _, cx| this.step_space(1, cx)))
        .on_action(cx.listener(|this, _: &PreviousProject, _, cx| this.step_project(-1, cx)))
        .on_action(cx.listener(|this, _: &NextProject, _, cx| this.step_project(1, cx)))
        .on_action(cx.listener(|this, _: &PreviousTab, _, cx| this.step_tab(-1, cx)))
        .on_action(cx.listener(|this, _: &NextTab, _, cx| this.step_tab(1, cx)))
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
            cx.listener(|_, RenameSpace(space): &RenameSpace, window, cx| {
                ask_space_name(*space, window, cx);
            }),
        )
        .on_action(
            cx.listener(|_, DeleteSpace(space): &DeleteSpace, window, cx| {
                delete::open(cx.weak_entity(), *space, window, cx);
            }),
        )
}

impl Shell {
    /// The projects of the space shown, by their place in the workspace, in the order the
    /// sidebar lists them. Every project is in a space (`Settings::reconcile_spaces`).
    pub fn space_projects(&self, cx: &App) -> Vec<usize> {
        let Some(space) = settings::get(cx).shown_space() else {
            return Vec::new();
        };
        (space.projects.iter())
            .filter_map(|path| self.workspace.project_index(path))
            .collect()
    }

    /// What goes on in the space at `index`, in all its projects: what its mark at the
    /// sidebar's foot says while another space is shown (`docs/sidebar.md` 4), as its
    /// projects would say were it shown.
    pub fn space_activity(&self, index: usize, cx: &App) -> Activity {
        let Some(space) = settings::get(cx).spaces.get(index) else {
            return Activity::default();
        };
        (space.projects.iter())
            .filter_map(|path| self.workspace.project_index(path))
            .map(|project| self.project_activity(&self.workspace.projects[project], cx))
            .sum()
    }

    /// Shows space `index`, and the project it showed last, its first otherwise, in place of the
    /// Settings tab. An empty space shows no project, so nothing of another space's shows, or is
    /// acted on, either.
    pub fn show_space(&mut self, index: usize, cx: &mut Context<Self>) {
        settings::update(cx, |settings| {
            settings.space = index.min(settings.spaces.len().saturating_sub(1));
        });
        let target = settings::get(cx)
            .shown_space()
            .and_then(Space::shown_project);
        let index = target.and_then(|path| self.workspace.project_index(path));
        self.workspace.select_project(index);
        self.follow_space_pages(cx);
        self.workspace.leave_settings();
        cx.notify();
    }

    /// Shows the space known by `space`, unless it's the one shown or is gone.
    pub(super) fn show_space_known_by(&mut self, space: u64, cx: &mut Context<Self>) {
        let settings = settings::get(cx);
        let index = settings.space_index(space);
        if let Some(index) = index.filter(|index| *index != settings.space) {
            self.show_space(index, cx);
        }
    }

    /// Selects the project at `index`, leaving the Settings tab, a pinned or a
    /// loose page shown for it.
    pub fn select_project(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.workspace.projects.len() {
            self.workspace.show_project(index);
            cx.notify();
        }
    }

    /// Follows the project selected, however it was: into its space if it's in another, and as
    /// the project its space shows. An empty space shown has no project to follow, so it stays
    /// shown. Followed into another space, it's the project that was asked for: its tab shows,
    /// not the loose page that space showed last, which would come back with the space's pages.
    pub(crate) fn follow_space(&mut self, cx: &mut Context<Self>) {
        let Some(active) = self.workspace.project().map(|project| &project.path) else {
            return;
        };
        let settings = settings::get(cx);
        let shown = settings.shown_space();
        if shown.is_some_and(|space| space.selected.as_deref() == Some(active.as_path())) {
            return;
        }
        let Some(space) = settings.space_of(active) else {
            return;
        };
        let elsewhere = settings.space != space;
        let active = active.clone();
        settings::update(cx, |settings| {
            settings.space = space;
            settings.spaces[space].selected = Some(active);
        });
        if elsewhere {
            self.follow_space_pages(cx);
            self.workspace.leave_apart();
        }
    }

    /// Moves `project` to the space with id `to`, or a new one, and shows it there.
    fn move_project(&mut self, project: &Path, to: Option<u64>, cx: &mut Context<Self>) {
        settings::update(cx, |settings| {
            let to = match to {
                Some(to) => settings.space_index(to),
                None => Some(settings.add_space()),
            };
            let Some(to) = to else {
                return;
            };
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
}

/// The menu of the space with id `space`, as its mark's right-click and the sidebar's offer it
/// (`docs/layout.md`, Space menu and theme): Rename Space… and Delete Space, for that space. The
/// last space can't be deleted, and a space has no theme of its own to edit.
pub fn space_menu(space: u64, cx: &App) -> NativeMenu {
    let last = settings::get(cx).spaces.len() <= 1;
    NativeMenu::new()
        .menu("Rename Space…", Box::new(RenameSpace(space)))
        .separator()
        .menu_with_disabled("Delete Space", last, Box::new(DeleteSpace(space)))
}

/// Asks what to call the space with id `space`, unless it's gone; an empty name leaves it as
/// it was.
fn ask_space_name(space: u64, window: &mut Window, cx: &mut App) {
    let settings = settings::get(cx);
    let Some(name) = (settings.space_index(space)).map(|at| settings.spaces[at].name.clone())
    else {
        return;
    };
    rename::open(
        "Rename Space",
        name,
        "Space name",
        window,
        cx,
        move |name, cx| {
            if name.is_empty() {
                return;
            }
            settings::update(cx, |settings| {
                if let Some(at) = settings.space_index(space) {
                    settings.spaces[at].name = name;
                }
            });
        },
    );
}
