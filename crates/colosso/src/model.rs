//! Placeholder workspace data the shell renders until real projects and agents exist.

use gpui_kit::SharedString;
use gpui_kit::assets::IconName;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarMode {
    Orca,
    Arc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Chat,
    Terminal,
    Browser,
    Editor,
}

impl PaneKind {
    pub fn icon(self) -> IconName {
        match self {
            PaneKind::Chat => IconName::MessageSquare,
            PaneKind::Terminal => IconName::SquareTerminal,
            PaneKind::Browser => IconName::Globe,
            PaneKind::Editor => IconName::FileCode,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pane {
    pub kind: PaneKind,
    pub title: SharedString,
}

#[derive(Clone, Debug)]
pub enum Tab {
    Single(Pane),
    Split(Pane, Pane),
}

impl Tab {
    pub fn title(&self) -> SharedString {
        match self {
            Tab::Single(pane) => pane.title.clone(),
            Tab::Split(left, right) => format!("{} | {}", left.title, right.title).into(),
        }
    }

    pub fn icon(&self) -> IconName {
        match self {
            Tab::Single(pane) => pane.kind.icon(),
            Tab::Split(..) => IconName::Columns2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchKind {
    Checkout,
    Worktree,
}

/// A checkout or worktree of a project; tabs belong to it.
#[derive(Clone, Debug)]
pub struct BranchUnit {
    pub name: SharedString,
    pub kind: BranchKind,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
}

#[derive(Clone, Debug)]
pub struct Project {
    pub name: SharedString,
    pub branches: Vec<BranchUnit>,
    pub active_branch: usize,
}

impl Project {
    pub fn initials(&self) -> SharedString {
        self.name
            .split(['-', '_', ' '])
            .filter_map(|word| word.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase()
            .into()
    }
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub projects: Vec<Project>,
    pub active_project: usize,
    pub mode: SidebarMode,
}

impl Workspace {
    pub fn project(&self) -> &Project {
        &self.projects[self.active_project]
    }

    pub fn branch(&self) -> &BranchUnit {
        let project = self.project();
        &project.branches[project.active_branch]
    }

    fn branch_mut(&mut self) -> &mut BranchUnit {
        let project = &mut self.projects[self.active_project];
        &mut project.branches[project.active_branch]
    }

    pub fn select_branch(&mut self, project: usize, branch: usize) {
        self.active_project = project;
        self.projects[project].active_branch = branch;
    }

    pub fn select_tab(&mut self, project: usize, branch: usize, tab: usize) {
        self.select_branch(project, branch);
        self.branch_mut().active_tab = tab;
    }

    /// Steps through projects without wrapping, so a swipe past either end does nothing.
    pub fn step_project(&mut self, delta: isize) {
        let last = self.projects.len().saturating_sub(1);
        self.active_project = self.active_project.saturating_add_signed(delta).min(last);
    }

    pub fn close_tab(&mut self, project: usize, branch: usize, tab: usize) {
        let unit = &mut self.projects[project].branches[branch];
        if tab >= unit.tabs.len() {
            return;
        }
        unit.tabs.remove(tab);
        if unit.active_tab > tab || unit.active_tab >= unit.tabs.len() {
            unit.active_tab = unit.active_tab.saturating_sub(1);
        }
    }

    pub fn new_chat(&mut self) {
        let unit = self.branch_mut();
        unit.tabs.push(Tab::Single(Pane {
            kind: PaneKind::Chat,
            title: "New chat".into(),
        }));
        unit.active_tab = unit.tabs.len() - 1;
    }

    pub fn sample() -> Self {
        fn pane(kind: PaneKind, title: &'static str) -> Pane {
            Pane {
                kind,
                title: title.into(),
            }
        }
        fn chat(title: &'static str) -> Tab {
            Tab::Single(pane(PaneKind::Chat, title))
        }
        fn branch(name: &'static str, kind: BranchKind, tabs: Vec<Tab>) -> BranchUnit {
            BranchUnit {
                name: name.into(),
                kind,
                tabs,
                active_tab: 0,
            }
        }

        Workspace {
            projects: vec![
                Project {
                    name: "cross-border".into(),
                    active_branch: 0,
                    branches: vec![
                        branch(
                            "main",
                            BranchKind::Checkout,
                            vec![
                                chat("Payee onboarding"),
                                Tab::Single(pane(PaneKind::Browser, "localhost:3000")),
                                Tab::Split(
                                    pane(PaneKind::Chat, "Callbacks"),
                                    pane(PaneKind::Browser, "Docs"),
                                ),
                            ],
                        ),
                        branch(
                            "feat-payouts",
                            BranchKind::Worktree,
                            vec![chat("Payout batching")],
                        ),
                        branch(
                            "fix-callbacks",
                            BranchKind::Worktree,
                            vec![
                                chat("Retry storm"),
                                Tab::Single(pane(PaneKind::Browser, "Grafana")),
                            ],
                        ),
                        branch(
                            "audit-logs",
                            BranchKind::Worktree,
                            vec![Tab::Split(
                                pane(PaneKind::Chat, "Audit trail"),
                                pane(PaneKind::Terminal, "zsh"),
                            )],
                        ),
                    ],
                },
                Project {
                    name: "monorepo".into(),
                    active_branch: 0,
                    branches: vec![
                        branch(
                            "main",
                            BranchKind::Checkout,
                            vec![
                                chat("Release notes"),
                                Tab::Single(pane(PaneKind::Editor, "Cargo.toml")),
                            ],
                        ),
                        branch(
                            "cli-explorer",
                            BranchKind::Worktree,
                            vec![chat("Histogram panel")],
                        ),
                    ],
                },
                Project {
                    name: "brazil-stablecoin".into(),
                    active_branch: 0,
                    branches: vec![branch(
                        "main",
                        BranchKind::Checkout,
                        vec![chat("Pix settlement")],
                    )],
                },
            ],
            active_project: 0,
            mode: SidebarMode::Orca,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_with_tabs(count: usize, active: usize) -> Workspace {
        let mut workspace = Workspace::sample();
        let unit = workspace.branch_mut();
        unit.tabs = (0..count)
            .map(|_| {
                Tab::Single(Pane {
                    kind: PaneKind::Chat,
                    title: "chat".into(),
                })
            })
            .collect();
        unit.active_tab = active;
        workspace
    }

    #[test]
    fn closing_a_tab_before_the_active_one_keeps_the_same_tab_active() {
        let mut workspace = unit_with_tabs(3, 2);
        workspace.close_tab(0, 0, 0);
        assert_eq!(workspace.branch().active_tab, 1);
    }

    #[test]
    fn closing_the_last_active_tab_activates_its_left_neighbour() {
        let mut workspace = unit_with_tabs(3, 2);
        workspace.close_tab(0, 0, 2);
        assert_eq!(workspace.branch().active_tab, 1);
    }

    #[test]
    fn stepping_projects_stops_at_both_ends() {
        let mut workspace = Workspace::sample();
        workspace.step_project(-1);
        assert_eq!(workspace.active_project, 0);
        workspace.step_project(10);
        assert_eq!(workspace.active_project, workspace.projects.len() - 1);
    }
}
