//! What the shell shows: the projects the user added, the checkouts git reports for each,
//! and the tabs open on each checkout. Tab contents are placeholders until agents exist.

use std::path::{Path, PathBuf};

use colosso_git::{Head, Worktree};
use gpui_kit::SharedString;
use gpui_kit::assets::IconName;

mod closed;
mod moves;
mod tab;
pub use closed::ClosedTab;
pub use tab::{Edge, Pane, SPLIT_MIN, Split, SplitAxis, Tab};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Chat,
    Terminal,
    Browser,
    Editor,
}

impl PaneKind {
    pub const ALL: [PaneKind; 4] = [
        PaneKind::Chat,
        PaneKind::Terminal,
        PaneKind::Browser,
        PaneKind::Editor,
    ];

    /// The menu entry that opens a tab of this kind.
    pub fn new_tab_label(self) -> &'static str {
        match self {
            PaneKind::Chat => "New Chat",
            PaneKind::Terminal => "New Terminal",
            PaneKind::Browser => "New Browser Tab",
            PaneKind::Editor => "New Editor Tab",
        }
    }

    pub(crate) fn new_tab_title(self) -> &'static str {
        match self {
            PaneKind::Chat => "New chat",
            PaneKind::Terminal => "zsh",
            PaneKind::Browser => "New tab",
            PaneKind::Editor => "Untitled",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            PaneKind::Chat => IconName::MessageSquare,
            PaneKind::Terminal => IconName::SquareTerminal,
            PaneKind::Browser => IconName::Globe,
            PaneKind::Editor => IconName::FileCode,
        }
    }
}

/// A checkout or worktree of a project, as git last reported it; tabs belong to it.
#[derive(Clone, Debug)]
pub struct BranchUnit {
    pub worktree: Worktree,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
}

impl BranchUnit {
    /// Closes the tab at `at`, handing it back: the one shown stays shown, or its left
    /// neighbour takes over.
    fn close(&mut self, at: usize) -> Option<Tab> {
        if at >= self.tabs.len() {
            return None;
        }
        let closed = self.tabs.remove(at);
        if self.active_tab > at || self.active_tab >= self.tabs.len() {
            self.active_tab = self.active_tab.saturating_sub(1);
        }
        Some(closed)
    }

    fn new(worktree: Worktree) -> Self {
        BranchUnit {
            worktree,
            tabs: Vec::new(),
            active_tab: 0,
        }
    }

    /// What the sidebar calls it: its branch, or the commit it is detached at.
    pub fn name(&self) -> SharedString {
        match &self.worktree.head {
            Head::Branch(branch) => branch.clone().into(),
            Head::Detached(commit) => format!("detached at {}", short(commit)).into(),
        }
    }

    pub fn is_main(&self) -> bool {
        self.worktree.is_main
    }

    /// Whether a search for `query`, lowercased, finds it by its name or its folder's.
    fn matches(&self, query: &str) -> bool {
        let folder = self.worktree.path.file_name().unwrap_or_default();
        self.name().to_lowercase().contains(query)
            || folder.to_string_lossy().to_lowercase().contains(query)
    }
}

fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

/// A git repository the user added, by its main checkout.
#[derive(Clone, Debug)]
pub struct Project {
    pub path: PathBuf,
    pub name: SharedString,
    /// Empty until git has been asked for them.
    pub branches: Vec<BranchUnit>,
    pub active_branch: usize,
    /// Why its worktrees could not be read the last time they were asked for.
    pub error: Option<SharedString>,
    /// The number of the worktree listing shown, so an older one arriving late is ignored.
    pub read: u64,
}

impl Project {
    fn new(path: PathBuf) -> Self {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        Project {
            path,
            name: name.into(),
            branches: Vec::new(),
            active_branch: 0,
            error: None,
            read: 0,
        }
    }

    /// The branch units a sidebar search for `query` shows, by index: all of them when the
    /// query is empty or names the project, otherwise those it finds.
    pub fn shown_branches(&self, query: &str) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        let all = query.is_empty() || self.name.to_lowercase().contains(&query);
        (0..self.branches.len())
            .filter(|&index| all || self.branches[index].matches(&query))
            .collect()
    }

    pub fn initials(&self) -> SharedString {
        self.name
            .split(['-', '_', ' '])
            .filter_map(|word| word.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase()
            .into()
    }

    pub fn branch(&self) -> Option<&BranchUnit> {
        self.branches.get(self.active_branch)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Workspace {
    pub projects: Vec<Project>,
    pub active_project: usize,
    /// Tabs closed lately, the newest last.
    closed: Vec<ClosedTab>,
}

impl Workspace {
    /// The projects at `paths`, their worktrees not yet read.
    pub fn new(paths: &[PathBuf]) -> Self {
        Workspace {
            projects: paths.iter().cloned().map(Project::new).collect(),
            active_project: 0,
            closed: Vec::new(),
        }
    }

    pub fn project(&self) -> Option<&Project> {
        self.projects.get(self.active_project)
    }

    pub fn branch(&self) -> Option<&BranchUnit> {
        self.project()?.branch()
    }

    pub fn branch_mut(&mut self) -> Option<&mut BranchUnit> {
        let project = self.projects.get_mut(self.active_project)?;
        project.branches.get_mut(project.active_branch)
    }

    /// The project the checkout at `worktree` belongs to.
    pub fn project_of(&self, worktree: &Path) -> Option<&Project> {
        self.projects.iter().find(|project| {
            project
                .branches
                .iter()
                .any(|unit| unit.worktree.path == worktree)
        })
    }

    pub fn project_index(&self, path: &Path) -> Option<usize> {
        self.projects
            .iter()
            .position(|project| project.path == path)
    }

    /// The main checkouts of the projects, in order, for the settings file.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.projects
            .iter()
            .map(|project| project.path.clone())
            .collect()
    }

    /// Adds the project at `path` unless it is already there, and selects it.
    pub fn add_project(&mut self, path: PathBuf) -> usize {
        let index = self.project_index(&path).unwrap_or_else(|| {
            self.projects.push(Project::new(path));
            self.projects.len() - 1
        });
        self.active_project = index;
        index
    }

    pub fn remove_project(&mut self, index: usize) {
        if index >= self.projects.len() {
            return;
        }
        self.projects.remove(index);
        if self.active_project > index || self.active_project >= self.projects.len() {
            self.active_project = self.active_project.saturating_sub(1);
        }
    }

    /// Takes what git now reports for `project`. Checkouts it still has keep their tabs, and
    /// the selected one stays selected; if it went away, the main checkout is selected.
    pub fn set_worktrees(&mut self, project: usize, worktrees: Vec<Worktree>) {
        let project = &mut self.projects[project];
        let selected = project.branch().map(|unit| unit.worktree.path.clone());
        let mut previous = std::mem::take(&mut project.branches);
        project.branches = worktrees
            .into_iter()
            .map(|worktree| {
                match previous
                    .iter()
                    .position(|unit| unit.worktree.path == worktree.path)
                {
                    Some(index) => BranchUnit {
                        worktree,
                        ..previous.swap_remove(index)
                    },
                    None => BranchUnit::new(worktree),
                }
            })
            .collect();
        project.active_branch = selected
            .and_then(|path| {
                project
                    .branches
                    .iter()
                    .position(|unit| unit.worktree.path == path)
            })
            .unwrap_or(0);
        project.error = None;
    }

    pub fn set_error(&mut self, project: usize, error: impl Into<SharedString>) {
        let project = &mut self.projects[project];
        let error = error.into();
        // Said once, rather than again each time the window comes back.
        if project.error.as_ref() != Some(&error) {
            log::warn!("couldn't read {}: {error}", project.path.display());
        }
        project.error = Some(error);
    }

    /// Selects the checkout at `path` in `project`, if git reported it.
    pub fn select_worktree(&mut self, project: usize, path: &Path) -> bool {
        let Some(branch) = self.projects[project]
            .branches
            .iter()
            .position(|unit| unit.worktree.path == path)
        else {
            return false;
        };
        self.select_branch(project, branch);
        true
    }

    pub fn select_branch(&mut self, project: usize, branch: usize) {
        self.active_project = project;
        self.projects[project].active_branch = branch;
    }

    pub fn select_tab(&mut self, project: usize, branch: usize, tab: usize) {
        self.select_branch(project, branch);
        if let Some(unit) = self.branch_mut() {
            unit.active_tab = tab;
        }
    }

    /// Steps through projects without wrapping, so a swipe past either end does nothing.
    pub fn step_project(&mut self, delta: isize) {
        let last = self.projects.len().saturating_sub(1);
        self.active_project = self.active_project.saturating_add_signed(delta).min(last);
    }

    /// Closes the tab at `tab` on `branch` of `project`, handing it back.
    pub fn close_tab(&mut self, project: usize, branch: usize, tab: usize) -> Option<Tab> {
        self.projects[project].branches[branch].close(tab)
    }

    /// Opens a tab of `kind` on the selected checkout, if there is one.
    pub fn new_tab(&mut self, kind: PaneKind) {
        self.push_tab(Tab::Single(Pane::new(kind)));
    }

    /// Opens `tab` on the selected checkout, if there is one, and shows it.
    pub fn push_tab(&mut self, tab: Tab) {
        let Some(unit) = self.branch_mut() else {
            return;
        };
        unit.tabs.push(tab);
        unit.active_tab = unit.tabs.len() - 1;
    }

    /// Opens `tab` right after the tab at `(project, branch, at)`, without showing it.
    pub fn insert_tab_after(&mut self, (project, branch, at): (usize, usize, usize), tab: Tab) {
        let Some(unit) = self.unit_mut(project, branch) else {
            return;
        };
        let at = (at + 1).min(unit.tabs.len());
        unit.tabs.insert(at, tab);
        if unit.active_tab >= at {
            unit.active_tab += 1;
        }
    }

    /// The first pane that is `found`: where its tab is, as `(project, branch, tab)`, and
    /// the pane's id.
    pub fn find_pane(&self, found: impl Fn(&Pane) -> bool) -> Option<((usize, usize, usize), u64)> {
        self.projects.iter().enumerate().find_map(|(p, project)| {
            project.branches.iter().enumerate().find_map(|(b, unit)| {
                unit.tabs.iter().enumerate().find_map(|(t, tab)| {
                    let pane = tab.panes().into_iter().find(|pane| found(pane))?;
                    Some(((p, b, t), pane.id))
                })
            })
        })
    }

    /// Where the tab shown is, as `(project, branch, tab)`.
    pub fn shown_tab(&self) -> Option<(usize, usize, usize)> {
        let project = self.project()?;
        let unit = project.branch()?;
        Some((self.active_project, project.active_branch, unit.active_tab))
    }

    pub fn unit_mut(&mut self, project: usize, branch: usize) -> Option<&mut BranchUnit> {
        self.projects.get_mut(project)?.branches.get_mut(branch)
    }

    pub fn tab_mut(&mut self, (project, branch, tab): (usize, usize, usize)) -> Option<&mut Tab> {
        self.unit_mut(project, branch)?.tabs.get_mut(tab)
    }
}

#[cfg(test)]
mod tests;
