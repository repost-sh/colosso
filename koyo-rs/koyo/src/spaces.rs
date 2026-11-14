//! Spaces (`docs/arc-sidebar.md` 6, 9): the Arc sidebar shows the projects of one space at a
//! time, and the window takes that space's colours. Each project is in exactly one space, and
//! there's always a space. What a project is called and shows as in Koyo, whatever its folder,
//! and the order and folding of its worktrees in the sidebar, are kept here too. All of it lives
//! in the settings file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gpui_kit::Hsla;
use serde::{Deserialize, Serialize};

use crate::gradient::GradientTheme;
use crate::settings::{Layout, Settings};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Space {
    /// What it's known by whatever its place among the spaces, which a space deleted before it
    /// changes: the pages open in it that are no project's go by it (`docs/arc-sidebar.md` 11).
    /// None, 0, until the spaces are next put right, which gives it one.
    pub id: u64,
    pub name: String,
    /// The main checkouts of its projects, in the order its tiles show them.
    pub projects: Vec<PathBuf>,
    /// The project it shows, so coming back to the space comes back to it.
    pub selected: Option<PathBuf>,
    /// Its own colours, for the light and the dark appearance; the window's until it has some.
    pub light_theme: Option<GradientTheme>,
    pub dark_theme: Option<GradientTheme>,
}

impl Default for Space {
    fn default() -> Self {
        Space::named("Space 1")
    }
}

impl Space {
    pub fn named(name: impl Into<String>) -> Self {
        Space {
            id: 0,
            name: name.into(),
            projects: Vec::new(),
            selected: None,
            light_theme: None,
            dark_theme: None,
        }
    }

    /// The project it shows: the one it showed last, its first otherwise; none while it has none.
    pub fn shown_project(&self) -> Option<&Path> {
        (self.selected.as_deref()).or_else(|| self.projects.first().map(PathBuf::as_path))
    }

    /// Its own theme for the appearance, if it has one.
    pub fn theme(&self, dark: bool) -> Option<&GradientTheme> {
        if dark {
            self.dark_theme.as_ref()
        } else {
            self.light_theme.as_ref()
        }
    }

    fn theme_slot(&mut self, dark: bool) -> &mut Option<GradientTheme> {
        if dark {
            &mut self.dark_theme
        } else {
            &mut self.light_theme
        }
    }

    pub fn holds(&self, project: &Path) -> bool {
        self.projects.iter().any(|held| held == project)
    }
}

/// What a project is called and shows as in Koyo, whatever its folder, and how its worktrees
/// show in the sidebar (`docs/arc-sidebar.md` 5).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectLook {
    /// The name the person gave it; its folder's otherwise.
    pub name: Option<String>,
    /// An image the person picked; the git host's avatar otherwise.
    pub icon: Option<PathBuf>,
    /// Its worktrees in the order the person dragged them into, by their folders; git's order
    /// until they do, and for any git lists that isn't here, after these.
    pub worktrees: Vec<PathBuf>,
    /// Its worktrees collapsed in the Arc sidebar, by their folders.
    pub collapsed: BTreeSet<PathBuf>,
}

impl ProjectLook {
    fn is_empty(&self) -> bool {
        *self == ProjectLook::default()
    }
}

/// What happens to the projects of a space that's deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leftovers {
    /// They go to the space at this place in the list, before the deleted one goes.
    MoveTo(usize),
    /// They're taken out of Koyo.
    Remove,
}

impl Settings {
    /// Puts the spaces right after the projects changed or the file was read: there's a space,
    /// one is shown, each project is in one space exactly, and none holds a project that's
    /// gone. A project no space holds joins the space shown.
    pub fn reconcile_spaces(&mut self) {
        if self.spaces.is_empty() {
            self.spaces.push(Space::default());
        }
        self.space = self.space.min(self.spaces.len() - 1);
        self.name_spaces();
        let mut seen: Vec<PathBuf> = Vec::new();
        for space in &mut self.spaces {
            space.projects.retain(|project| {
                let keep = self.projects.contains(project) && !seen.contains(project);
                if keep {
                    seen.push(project.clone());
                }
                keep
            });
            if (space.selected.as_ref()).is_some_and(|selected| !space.projects.contains(selected))
            {
                space.selected = space.projects.first().cloned();
            }
        }
        let homeless: Vec<PathBuf> = (self.projects.iter())
            .filter(|project| !seen.contains(project))
            .cloned()
            .collect();
        let shown = &mut self.spaces[self.space];
        shown.projects.extend(homeless);
        if shown.selected.is_none() {
            shown.selected = shown.projects.first().cloned();
        }
        let projects = &self.projects;
        for space in &mut self.spaces {
            Self::order_space(projects, space);
        }
        self.project_looks
            .retain(|project, look| projects.contains(project) && !look.is_empty());
    }

    /// Gives each space that has none, or another's, an id of its own: one more than the
    /// highest there is, in the order of the spaces, so a file from before ids gets the same
    /// ones each time it's read.
    fn name_spaces(&mut self) {
        let mut taken: Vec<u64> = Vec::new();
        let mut next = self.spaces.iter().map(|space| space.id).max().unwrap_or(0);
        for space in &mut self.spaces {
            if space.id == 0 || taken.contains(&space.id) {
                next += 1;
                space.id = next;
            }
            taken.push(space.id);
        }
    }

    /// The id of the space the Arc sidebar shows; none of any space's, 0, with none shown.
    pub fn shown_space_id(&self) -> u64 {
        self.shown_space().map_or(0, |space| space.id)
    }

    /// The space the Arc sidebar shows.
    pub fn shown_space(&self) -> Option<&Space> {
        self.spaces.get(self.space)
    }

    pub fn shown_space_mut(&mut self) -> Option<&mut Space> {
        self.spaces.get_mut(self.space)
    }

    /// Where the space holding `project` is in the list.
    pub fn space_of(&self, project: &Path) -> Option<usize> {
        self.spaces.iter().position(|space| space.holds(project))
    }

    /// Orders `space`'s projects as the sidebar lists them (`projects`), so Arc's tiles and the
    /// Orca list agree, and a project moved in one moves in the other.
    fn order_space(projects: &[PathBuf], space: &mut Space) {
        space
            .projects
            .sort_by_key(|project| projects.iter().position(|listed| listed == project));
    }

    /// Moves `project` to the space at `to`, among its projects in the sidebar's order; the
    /// space it left shows another of its own.
    pub fn move_project(&mut self, project: &Path, to: usize) {
        if to >= self.spaces.len() || self.spaces[to].holds(project) {
            return;
        }
        for space in &mut self.spaces {
            space.projects.retain(|held| held != project);
            if space.selected.as_deref() == Some(project) {
                space.selected = space.projects.first().cloned();
            }
        }
        let space = &mut self.spaces[to];
        space.projects.push(project.to_owned());
        if space.selected.is_none() {
            space.selected = Some(project.to_owned());
        }
        Self::order_space(&self.projects, space);
    }

    /// Adds a space after the others, named as the next one is, and gives back where it is.
    pub fn add_space(&mut self) -> usize {
        let name = self.next_space_name();
        self.spaces.push(Space::named(name));
        self.name_spaces();
        self.spaces.len() - 1
    }

    /// "Space N", the lowest N no space is called.
    fn next_space_name(&self) -> String {
        // As many numbers as spaces, and one more: one of them is free.
        (1..=self.spaces.len() + 1)
            .map(|n| format!("Space {n}"))
            .find(|name| self.spaces.iter().all(|space| &space.name != name))
            .unwrap_or_default()
    }

    /// Deletes the space at `index`, its projects going as `leftovers` says, unless it's the last
    /// one; gives back the projects to take out of Koyo. The space shown stays shown, or the one
    /// before it once it's gone.
    pub fn delete_space(&mut self, index: usize, leftovers: Leftovers) -> Vec<PathBuf> {
        if self.spaces.len() <= 1 || index >= self.spaces.len() {
            return Vec::new();
        }
        let projects = self.spaces[index].projects.clone();
        let removed = match leftovers {
            Leftovers::MoveTo(to) if to != index && to < self.spaces.len() => {
                for project in &projects {
                    self.move_project(project, to);
                }
                Vec::new()
            }
            Leftovers::MoveTo(_) => return Vec::new(),
            Leftovers::Remove => projects,
        };
        self.spaces.remove(index);
        if self.space > index || self.space >= self.spaces.len() {
            self.space = self.space.saturating_sub(1);
        }
        removed
    }

    /// The colour the space at `index` shows as, its dot in the switcher and by its name
    /// (`docs/arc-sidebar.md` 7): its theme's first colour, the window's while it has none of its
    /// own; none while that has no colour either.
    pub fn space_colour(&self, index: usize, dark: bool) -> Option<Hsla> {
        let space = self.spaces.get(index)?;
        space
            .theme(dark)
            .unwrap_or_else(|| self.theme(dark))
            .primary()
    }

    /// The gradient theme the window shows for the appearance: the space's own in the Arc
    /// layout, once it has one, and the window's otherwise.
    pub fn shown_theme(&self, dark: bool) -> &GradientTheme {
        let space = (self.shown_space())
            .filter(|_| self.layout == Layout::Arc)
            .and_then(|space| space.theme(dark));
        space.unwrap_or_else(|| self.theme(dark))
    }

    /// The theme a change to the colours goes to: in the Arc layout, the space shown's, which
    /// starts as the window's; the window's otherwise.
    pub fn shown_theme_mut(&mut self, dark: bool) -> &mut GradientTheme {
        if self.layout != Layout::Arc || self.space >= self.spaces.len() {
            return self.theme_mut(dark);
        }
        let window = self.theme(dark).clone();
        self.spaces[self.space]
            .theme_slot(dark)
            .get_or_insert(window)
    }

    /// The image picked for `project`, if any.
    pub fn project_icon(&self, project: &Path) -> Option<&Path> {
        self.project_looks.get(project)?.icon.as_deref()
    }

    /// Names `project` `name` in Koyo; an empty name gives it back its folder's.
    pub fn rename_project(&mut self, project: &Path, name: &str) {
        let name = name.trim();
        let look = self.project_looks.entry(project.to_owned()).or_default();
        look.name = (!name.is_empty()).then(|| name.to_owned());
        self.drop_empty_look(project);
    }

    /// Has `project` show the image at `icon`, or the git host's avatar with none.
    pub fn set_project_icon(&mut self, project: &Path, icon: Option<PathBuf>) {
        self.project_looks
            .entry(project.to_owned())
            .or_default()
            .icon = icon;
        self.drop_empty_look(project);
    }

    /// The order the person put `project`'s worktrees in, by their folders: none until they
    /// move one.
    pub fn worktree_order(&self, project: &Path) -> &[PathBuf] {
        let look = self.project_looks.get(project);
        look.map(|look| look.worktrees.as_slice())
            .unwrap_or_default()
    }

    /// Lists `project`'s worktrees in `order`, by their folders, from now on.
    pub fn set_worktree_order(&mut self, project: &Path, order: Vec<PathBuf>) {
        self.project_looks
            .entry(project.to_owned())
            .or_default()
            .worktrees = order;
        self.drop_empty_look(project);
    }

    /// Whether `project`'s worktree at `worktree` is collapsed in the Arc sidebar.
    pub fn is_collapsed(&self, project: &Path, worktree: &Path) -> bool {
        (self.project_looks.get(project)).is_some_and(|look| look.collapsed.contains(worktree))
    }

    /// Collapses `project`'s worktree at `worktree` in the Arc sidebar, or opens it again.
    pub fn toggle_collapsed(&mut self, project: &Path, worktree: &Path) {
        let collapsed = &mut self
            .project_looks
            .entry(project.to_owned())
            .or_default()
            .collapsed;
        if !collapsed.remove(worktree) {
            collapsed.insert(worktree.to_owned());
        }
        self.drop_empty_look(project);
    }

    /// Whether anything's kept of a worktree of `project`'s that git no longer lists, as
    /// `listed`: what `keep_worktrees` forgets.
    pub fn keeps_gone_worktrees(&self, project: &Path, listed: &[PathBuf]) -> bool {
        self.project_looks.get(project).is_some_and(|look| {
            (look.worktrees.iter().chain(&look.collapsed)).any(|kept| !listed.contains(kept))
        })
    }

    /// Forgets the order and folding of `project`'s worktrees git no longer lists, as `listed`.
    pub fn keep_worktrees(&mut self, project: &Path, listed: &[PathBuf]) {
        let Some(look) = self.project_looks.get_mut(project) else {
            return;
        };
        look.worktrees.retain(|worktree| listed.contains(worktree));
        look.collapsed.retain(|worktree| listed.contains(worktree));
        self.drop_empty_look(project);
    }

    fn drop_empty_look(&mut self, project: &Path) {
        if self
            .project_looks
            .get(project)
            .is_some_and(ProjectLook::is_empty)
        {
            self.project_looks.remove(project);
        }
    }
}

/// The projects' looks, as the settings file keeps them.
pub type ProjectLooks = BTreeMap<PathBuf, ProjectLook>;

#[cfg(test)]
mod tests;
