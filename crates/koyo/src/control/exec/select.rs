//! What a selector picks (`docs/app-control.md` 3): a project by its main checkout's path or
//! its name when no other project has it; a worktree by its checkout's path, its branch when
//! that's unique, or `current`, the caller's; left out, the caller's worktree, else the one
//! selected in the window; and a pane by its id.

use std::path::{Path, PathBuf};

use koyo_app_control::{ControlError, ErrorCode};
use koyo_git::Head;

use super::super::Caller;
use crate::model::{BranchUnit, Workspace};

/// Where a tab is, as `(project, branch, tab)`.
pub(crate) type TabAt = (usize, usize, usize);

/// Which of `paths` names the folder `path` does, an absolute path: as written, else with
/// links followed, each folder looked up only when the plain comparison found none.
fn find_folder<'a>(
    mut paths: impl Iterator<Item = &'a Path> + Clone,
    path: &Path,
) -> Option<usize> {
    if !path.is_absolute() {
        return None;
    }
    paths.clone().position(|known| known == path).or_else(|| {
        let path = canonical(path);
        paths.position(|known| canonical(known) == path)
    })
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The project `selector` names.
pub(crate) fn project(workspace: &Workspace, selector: &str) -> Result<usize, ControlError> {
    let projects = &workspace.projects;
    let folders = projects.iter().map(|project| project.path.as_path());
    if let Some(at) = find_folder(folders, Path::new(selector)) {
        return Ok(at);
    }
    let named: Vec<usize> = (0..projects.len())
        .filter(|&at| projects[at].name.as_ref() == selector)
        .collect();
    match named.as_slice() {
        [at] => Ok(*at),
        [] => Err(ControlError::not_found(format!(
            "No project is called “{selector}” or has its main checkout there; project_list lists them."
        ))
        .with_details(serde_json::json!({ "candidates": paths(workspace) }))),
        many => Err(ControlError::ambiguous(
            format!("More than one project is called “{selector}”; name it by its path."),
            &(many.iter())
                .map(|&at| projects[at].path.display().to_string())
                .collect::<Vec<_>>(),
        )),
    }
}

fn paths(workspace: &Workspace) -> Vec<String> {
    (workspace.projects.iter())
        .map(|project| project.path.display().to_string())
        .collect()
}

/// The worktree `selector` names, among `project`'s when that's given, as `(project, branch)`.
/// Left out, the caller's worktree, else the selected one.
pub(crate) fn worktree(
    caller: &Caller,
    workspace: &Workspace,
    selector: Option<&str>,
    project: Option<&str>,
) -> Result<(usize, usize), ControlError> {
    let within = project
        .map(|project| self::project(workspace, project))
        .transpose()?;
    let in_scope = |at: usize| within.is_none_or(|within| within == at);
    let units: Vec<(usize, usize, &BranchUnit)> = (workspace.projects.iter().enumerate())
        .filter(|(at, _)| in_scope(*at))
        .flat_map(|(at, project)| {
            (project.branches.iter().enumerate()).map(move |(branch, unit)| (at, branch, unit))
        })
        .collect();
    let found = match selector {
        None => caller_worktree(caller, workspace)
            .filter(|(at, _)| in_scope(*at))
            .or_else(|| selected(workspace, within))
            .ok_or_else(|| ControlError::not_found("No worktree is selected in the window.")),
        Some("current") => caller_worktree(caller, workspace).ok_or_else(|| {
            ControlError::not_found(
                "This caller runs in no checkout of a project in Koyo, so it has no current worktree.",
            )
        }),
        Some(selector) => by_selector(&units, selector),
    };
    found.map_err(|error| still_reading(workspace, error))
}

/// The worktree `selector` names among `units`: by its checkout's path; else by the branch
/// it has checked out; else by the name it was made with, as its folder is named after it.
fn by_selector(
    units: &[(usize, usize, &BranchUnit)],
    selector: &str,
) -> Result<(usize, usize), ControlError> {
    let paths = units
        .iter()
        .map(|(_, _, unit)| unit.worktree.path.as_path());
    if let Some(at) = find_folder(paths, Path::new(selector)) {
        let (project, branch, _) = units[at];
        return Ok((project, branch));
    }
    let by_branch = |unit: &BranchUnit| matches!(&unit.worktree.head, Head::Branch(branch) if branch == selector);
    let slug = koyo_git::slugify(selector);
    let by_name = |unit: &BranchUnit| {
        unit.worktree
            .path
            .file_name()
            .is_some_and(|folder| *folder == *slug)
    };
    let mut named: Vec<&(usize, usize, &BranchUnit)> = units
        .iter()
        .filter(|(_, _, unit)| by_branch(unit))
        .collect();
    if named.is_empty() && !slug.is_empty() {
        named = units.iter().filter(|(_, _, unit)| by_name(unit)).collect();
    }
    match named.as_slice() {
        [(at, branch, _)] => Ok((*at, *branch)),
        [] => Err(ControlError::not_found(format!(
            "No worktree is at “{selector}”, has that branch checked out or was made with that name; project_list lists them."
        ))),
        many => Err(ControlError::ambiguous(
            format!(
                "More than one worktree has “{selector}” checked out; name it by its path or pass its project."
            ),
            &(many.iter())
                .map(|(_, _, unit)| unit.worktree.path.display().to_string())
                .collect::<Vec<_>>(),
        )),
    }
}

/// A worktree not found, said as a moment's wait mending it while git hasn't yet reported
/// a project's worktrees, as just after launch: every repository has its main checkout.
fn still_reading(workspace: &Workspace, error: ControlError) -> ControlError {
    let reading = (workspace.projects.iter())
        .any(|project| project.branches.is_empty() && project.error.is_none());
    if reading && error.code == ErrorCode::NotFound {
        return ControlError::not_found(
            "Koyo is still reading its projects' worktrees; try again in a moment.",
        )
        .retryable(true);
    }
    error
}

/// The worktree the caller runs in, as its environment or its working folder say.
fn caller_worktree(caller: &Caller, workspace: &Workspace) -> Option<(usize, usize)> {
    (caller.worktree.iter().chain(caller.cwd.iter())).find_map(|path| containing(workspace, path))
}

/// The checkout holding `path`, the deepest when one is inside another.
/// Compared as written first, then with links followed, so most calls look up no folder.
fn containing(workspace: &Workspace, path: &Path) -> Option<(usize, usize)> {
    let units = || {
        (workspace.projects.iter().enumerate()).flat_map(|(at, project)| {
            (project.branches.iter().enumerate()).map(move |(branch, unit)| (at, branch, unit))
        })
    };
    let deepest = |inside: &dyn Fn(&Path) -> bool| {
        units()
            .filter(|(_, _, unit)| inside(&unit.worktree.path))
            .max_by_key(|(_, _, unit)| unit.worktree.path.components().count())
            .map(|(at, branch, _)| (at, branch))
    };
    deepest(&|checkout| path.starts_with(checkout)).or_else(|| {
        let path = canonical(path);
        deepest(&|checkout| path.starts_with(canonical(checkout)))
    })
}

/// The selected worktree, of project `within` when that's given.
fn selected(workspace: &Workspace, within: Option<usize>) -> Option<(usize, usize)> {
    let at = within.unwrap_or(workspace.active_project);
    let project = workspace.projects.get(at)?;
    project.branches.get(project.active_branch)?;
    Some((at, project.active_branch))
}

/// The tab holding pane `id`, and so the pane's checkout.
pub(crate) fn pane(workspace: &Workspace, id: u64) -> Result<TabAt, ControlError> {
    (workspace.find_pane(|pane| pane.id == id))
        .map(|(at, _)| at)
        .ok_or_else(|| {
            ControlError::not_found(format!("No pane {id} is open; tab_list lists the panes."))
        })
}
