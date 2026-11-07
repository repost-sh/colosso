//! A project's worktrees: the branches to start one from, creating, selecting and deleting
//! them. Terminals list theirs with `worktree_list`, in the terminals' catalog.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::shared::{WorktreeInfo, count};
use crate::{Hints, Operation, Scope};

/// Whose branches to list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeBranchesInput {
    /// The project: its main checkout's path, or its name when unique. Left out, yours, else
    /// the selected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

/// A branch a worktree can start from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    /// Its name; a remote one with its remote, as `origin/main`.
    pub name: String,
    /// Whether it's a remote branch.
    pub remote: bool,
    /// Whether the main checkout has it checked out.
    pub current: bool,
}

/// The branches and how new worktrees are named.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeBranchesOutput {
    /// Local branches, then remote ones.
    pub branches: Vec<Branch>,
    /// The branch new worktrees start from unless told otherwise.
    pub default_base: String,
    /// What new branches are named under (`<prefix>/<name>`); empty for none.
    pub prefix: String,
    /// The folder new worktrees of this project go in.
    pub worktrees_dir: String,
}

/// `worktree_branches`: what a new worktree can start from.
#[derive(Debug)]
pub struct WorktreeBranches;

impl Operation for WorktreeBranches {
    const NAME: &'static str = "worktree_branches";
    const TITLE: &'static str = "List branches";
    const SUBJECT: &'static [&'static str] = &["project"];
    const SCOPE: Scope = Scope::Read;
    const DESCRIPTION: &'static str = "A project's local and remote branches, the default base for new worktrees, the branch prefix and the folder new worktrees go in.";
    const HINTS: Hints = Hints::READ;
    type Input = WorktreeBranchesInput;
    type Output = WorktreeBranchesOutput;

    fn summary(output: &WorktreeBranchesOutput) -> String {
        format!(
            "{}; new worktrees start from {}",
            count(output.branches.len(), "branch", "branches"),
            output.default_base
        )
    }
}

/// The worktree to create.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeCreateInput {
    /// The project: its main checkout's path, or its name when unique. Left out, yours, else
    /// the selected one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The worktree's name, as the New Worktree dialog takes it: its branch is
    /// `<prefix>/<slug>` and its folder the slug in the project's worktrees folder.
    pub name: String,
    /// The branch to start from; the project's default base when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Work on `base` itself instead of a new branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing: Option<bool>,
    /// Show the new worktree, switching the window to it. Left out, the person's choice in
    /// Settings → MCP server decides, and by default it's only listed in the sidebar. Pass
    /// true only when the person asked to see it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub select: Option<bool>,
}

/// A worktree just made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewWorktree {
    /// Its checkout's path.
    pub path: String,
    /// Its branch.
    pub branch: String,
}

/// What `worktree_create` made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCreateOutput {
    /// The new worktree.
    pub worktree: NewWorktree,
}

/// `worktree_create`: a new checkout.
#[derive(Debug)]
pub struct WorktreeCreate;

impl Operation for WorktreeCreate {
    const NAME: &'static str = "worktree_create";
    const TITLE: &'static str = "Create worktree";
    const SUBJECT: &'static [&'static str] = &["name"];
    const SCOPE: Scope = Scope::Manage;
    const DESCRIPTION: &'static str = "Creates a worktree of a project as the New Worktree dialog does: a branch `<prefix>/<slug>` from `base` (or `base` itself with `existing`), checked out in the project's worktrees folder, and lists it in the sidebar. The window switches to it, with a chat open on it, only with `select: true`, or with `select` left out when the person chose in Settings to see what agents open.";
    const HINTS: Hints = Hints::CHANGE;
    type Input = WorktreeCreateInput;
    type Output = WorktreeCreateOutput;

    fn summary(output: &WorktreeCreateOutput) -> String {
        format!(
            "Created worktree {} on {}",
            output.worktree.path, output.worktree.branch
        )
    }
}

/// The worktree to switch to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeSelectInput {
    /// The worktree: its checkout path, its branch name (unique across projects, or within
    /// `project`), or `current` for yours.
    pub worktree: String,
    /// The project a branch name is looked up in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

/// The worktree the window shows now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSelectOutput {
    /// Its project's main checkout.
    pub project: String,
    /// The worktree, as `project_list` lists it.
    pub worktree: WorktreeInfo,
}

/// `worktree_select`: switch to a checkout.
#[derive(Debug)]
pub struct WorktreeSelect;

impl Operation for WorktreeSelect {
    const NAME: &'static str = "worktree_select";
    const TITLE: &'static str = "Select worktree";
    const SUBJECT: &'static [&'static str] = &["worktree"];
    const SCOPE: Scope = Scope::Navigate;
    const DESCRIPTION: &'static str =
        "Switches the window to a worktree, selecting its project too.";
    const HINTS: Hints = Hints::IDEMPOTENT;
    type Input = WorktreeSelectInput;
    type Output = WorktreeSelectOutput;

    fn summary(output: &WorktreeSelectOutput) -> String {
        format!(
            "Showing {}{}",
            output.worktree.path,
            output
                .worktree
                .branch
                .as_deref()
                .map_or_else(String::new, |branch| format!(" on {branch}"))
        )
    }
}

/// The worktree to delete.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeDeleteInput {
    /// The worktree: its checkout path, its branch name, or `current` for yours. Never the
    /// main checkout.
    pub worktree: String,
    /// Delete it even with uncommitted changes or a branch that isn't merged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
}

/// What `worktree_delete` deleted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeDeleteOutput {
    /// The deleted checkout's path.
    pub deleted: String,
    /// The branch it had checked out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

/// `worktree_delete`: a checkout off the disk, confirmed by the person.
#[derive(Debug)]
pub struct WorktreeDelete;

impl Operation for WorktreeDelete {
    const NAME: &'static str = "worktree_delete";
    const TITLE: &'static str = "Delete worktree";
    const SUBJECT: &'static [&'static str] = &["worktree"];
    const SCOPE: Scope = Scope::Destructive;
    const DESCRIPTION: &'static str = "Deletes a worktree's checkout and closes its tabs and terminals, once the person confirms in the window; the prompt names unpushed commits, changed files and running terminals. Refuses the main checkout. Declined or unanswered in 2 minutes, it fails user_declined.";
    const HINTS: Hints = Hints::DESTRUCTIVE;
    const CONFIRM: bool = true;
    type Input = WorktreeDeleteInput;
    type Output = WorktreeDeleteOutput;

    fn summary(output: &WorktreeDeleteOutput) -> String {
        format!("Deleted worktree {}", output.deleted)
    }
}
