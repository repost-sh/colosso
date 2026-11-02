//! The projects in the sidebar: list, add, select, remove, reorder.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::shared::{NoInput, ProjectInfo, count};
use crate::{Hints, Operation, Scope};

/// The projects, in sidebar order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectListOutput {
    /// Each project with its worktrees.
    pub projects: Vec<ProjectInfo>,
}

/// One project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOutput {
    /// The project, as `project_list` lists it.
    pub project: ProjectInfo,
}

/// A project to act on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectInput {
    /// The project: its main checkout's path, or its name when no other project has it.
    pub project: String,
}

/// `project_list`: the projects and their worktrees.
#[derive(Debug)]
pub struct ProjectList;

impl Operation for ProjectList {
    const NAME: &'static str = "project_list";
    const TITLE: &'static str = "List projects";
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "The projects in the sidebar, in order, each with its worktrees: path, branch, whether it's the main checkout or selected, and how many tabs and terminals it has.";
    const HINTS: Hints = Hints::READ;
    type Input = NoInput;
    type Output = ProjectListOutput;

    fn summary(output: &ProjectListOutput) -> String {
        let worktrees: usize = output.projects.iter().map(|p| p.worktrees.len()).sum();
        let selected = output
            .projects
            .iter()
            .find(|project| project.selected)
            .map_or_else(String::new, |project| {
                format!("; {} selected", project.name)
            });
        format!(
            "{} with {}{selected}",
            count(output.projects.len(), "project", "projects"),
            count(worktrees, "worktree", "worktrees"),
        )
    }
}

/// The folder to add.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectAddInput {
    /// An absolute path to a git repository, or a folder inside one: its main checkout is
    /// added.
    pub path: String,
}

/// `project_add`: a repository into the sidebar.
#[derive(Debug)]
pub struct ProjectAdd;

impl Operation for ProjectAdd {
    const NAME: &'static str = "project_add";
    const TITLE: &'static str = "Add project";
    const SUBJECT: &'static [&'static str] = &["path"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Adds a git repository to the sidebar and selects it. The folder must be a repository or inside one, whose main checkout is added; one already there is selected.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = ProjectAddInput;
    type Output = ProjectOutput;

    fn summary(output: &ProjectOutput) -> String {
        format!(
            "Added and selected {} ({})",
            output.project.name, output.project.path
        )
    }
}

/// `project_select`: show a project.
#[derive(Debug)]
pub struct ProjectSelect;

impl Operation for ProjectSelect {
    const NAME: &'static str = "project_select";
    const TITLE: &'static str = "Select project";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str =
        "Shows a project in the window, on the worktree it showed last.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = ProjectInput;
    type Output = ProjectOutput;

    fn summary(output: &ProjectOutput) -> String {
        format!("Selected {} ({})", output.project.name, output.project.path)
    }
}

/// What `project_remove` took out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRemoveOutput {
    /// The removed project's main checkout. Its files are untouched.
    pub removed: String,
}

/// `project_remove`: out of the sidebar, files kept.
#[derive(Debug)]
pub struct ProjectRemove;

impl Operation for ProjectRemove {
    const NAME: &'static str = "project_remove";
    const TITLE: &'static str = "Remove project";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Takes a project out of the sidebar, closing its tabs. Its files and worktrees stay on disk, and project_add brings it back.";
    const HINTS: Hints = Hints::DESTRUCTIVE;
    type Input = ProjectInput;
    type Output = ProjectRemoveOutput;

    fn summary(output: &ProjectRemoveOutput) -> String {
        format!(
            "Removed {} from the sidebar; its files stay",
            output.removed
        )
    }
}

/// Where to move a project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectMoveInput {
    /// The project: its main checkout's path, or its name when no other project has it.
    pub project: String,
    /// Its place in the sidebar after the move, from 0; past the end means last.
    pub to: u32,
}

/// The sidebar's order after a move.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOrder {
    /// The projects' main checkouts, in sidebar order.
    pub projects: Vec<String>,
}

/// `project_move`: reorder the sidebar.
#[derive(Debug)]
pub struct ProjectMove;

impl Operation for ProjectMove {
    const NAME: &'static str = "project_move";
    const TITLE: &'static str = "Move project";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str =
        "Moves a project to another place in the sidebar. Answers the new order.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = ProjectMoveInput;
    type Output = ProjectOrder;

    fn summary(output: &ProjectOrder) -> String {
        format!("Sidebar order: {}", output.projects.join(", "))
    }
}
