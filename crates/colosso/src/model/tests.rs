use super::*;

fn worktree(path: &str, branch: &str) -> Worktree {
    Worktree {
        path: path.into(),
        head: Head::Branch(branch.into()),
        is_main: path == "/src/app",
    }
}

/// One project, `/src/app`, with its main checkout and the worktrees named.
fn workspace(worktrees: &[&str]) -> Workspace {
    let mut workspace = Workspace::new(&["/src/app".into()]);
    let listed = std::iter::once(worktree("/src/app", "main"))
        .chain(
            worktrees
                .iter()
                .map(|name| worktree(&format!("/work/{name}"), name)),
        )
        .collect();
    workspace.set_worktrees(0, listed);
    workspace
}

fn unit_with_tabs(count: usize, active: usize) -> Workspace {
    let mut workspace = workspace(&[]);
    let unit = workspace.branch_mut().unwrap();
    unit.tabs = (0..count)
        .map(|_| {
            Tab::Single(Pane {
                kind: PaneKind::Chat,
                title: "chat".into(),
                file: None,
            })
        })
        .collect();
    unit.active_tab = active;
    workspace
}

#[test]
fn searching_shows_the_worktrees_whose_branch_or_folder_matches() {
    let workspace = workspace(&["fix-login", "Payout-Retries"]);
    let project = workspace.project().unwrap();
    assert_eq!(project.shown_branches(""), [0, 1, 2]);
    assert_eq!(project.shown_branches("  payout "), [2]);
    assert_eq!(project.shown_branches("MAIN"), [0]);
    assert_eq!(project.shown_branches("nothing"), Vec::<usize>::new());
}

#[test]
fn searching_for_a_project_shows_all_its_worktrees() {
    let workspace = workspace(&["fix-login"]);
    assert_eq!(workspace.project().unwrap().shown_branches("ap"), [0, 1]);
}

#[test]
fn closing_a_tab_before_the_active_one_keeps_the_same_tab_active() {
    let mut workspace = unit_with_tabs(3, 2);
    workspace.close_tab(0, 0, 0);
    assert_eq!(workspace.branch().unwrap().active_tab, 1);
}

#[test]
fn closing_the_last_active_tab_activates_its_left_neighbour() {
    let mut workspace = unit_with_tabs(3, 2);
    workspace.close_tab(0, 0, 2);
    assert_eq!(workspace.branch().unwrap().active_tab, 1);
}

#[test]
fn stepping_projects_stops_at_both_ends() {
    let mut workspace = Workspace::new(&["/a".into(), "/b".into(), "/c".into()]);
    workspace.step_project(-1);
    assert_eq!(workspace.active_project, 0);
    workspace.step_project(10);
    assert_eq!(workspace.active_project, 2);
}

#[test]
fn rereading_worktrees_keeps_tabs_and_selection() {
    let mut workspace = workspace(&["feat", "fix"]);
    workspace.select_branch(0, 2);
    workspace.new_tab(PaneKind::Terminal);
    workspace.set_worktrees(
        0,
        vec![
            worktree("/src/app", "main"),
            worktree("/work/new", "new"),
            worktree("/work/fix", "fix"),
        ],
    );
    let unit = workspace.branch().unwrap();
    assert_eq!(unit.name(), "fix");
    assert_eq!(unit.tabs.len(), 1);
    assert_eq!(workspace.project().unwrap().branches.len(), 3);
}

#[test]
fn a_selected_worktree_that_went_away_falls_back_to_the_main_checkout() {
    let mut workspace = workspace(&["feat"]);
    workspace.select_branch(0, 1);
    workspace.set_worktrees(0, vec![worktree("/src/app", "main")]);
    assert!(workspace.branch().unwrap().is_main());
}

#[test]
fn a_detached_checkout_is_named_after_its_commit() {
    let unit = BranchUnit::new(Worktree {
        head: Head::Detached("0123456789abcdef".into()),
        ..worktree("/work/x", "x")
    });
    assert_eq!(unit.name(), "detached at 0123456");
}

#[test]
fn adding_a_project_twice_selects_the_first() {
    let mut workspace = Workspace::new(&["/a".into(), "/b".into()]);
    assert_eq!(workspace.add_project("/a".into()), 0);
    assert_eq!(workspace.add_project("/c".into()), 2);
    assert_eq!(workspace.active_project, 2);
    assert_eq!(workspace.projects.len(), 3);
}

#[test]
fn removing_the_selected_last_project_selects_the_one_before() {
    let mut workspace = Workspace::new(&["/a".into(), "/b".into()]);
    workspace.active_project = 1;
    workspace.remove_project(1);
    assert_eq!(workspace.active_project, 0);
    workspace.remove_project(0);
    assert!(workspace.project().is_none());
}

#[test]
fn a_workspace_without_projects_has_nothing_selected() {
    let mut workspace = Workspace::default();
    workspace.new_tab(PaneKind::Chat);
    assert!(workspace.branch().is_none());
}
