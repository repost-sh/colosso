use super::*;

fn worktree(path: &str, branch: &str) -> Worktree {
    Worktree {
        path: path.into(),
        head: Head::Branch(branch.into()),
        is_main: path == "/src/app",
    }
}

/// One project, `/src/app`, with its main checkout and the worktrees named.
pub(super) fn workspace(worktrees: &[&str]) -> Workspace {
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
                title: "chat".into(),
                ..Pane::new(PaneKind::Chat)
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
fn a_closed_tab_comes_back_where_it_was_and_shows() {
    let mut workspace = unit_with_tabs(3, 0);
    let ids = |workspace: &Workspace| {
        let tabs = &workspace.branch().unwrap().tabs;
        tabs.iter().map(|tab| tab.panes()[0].id).collect::<Vec<_>>()
    };
    let before = ids(&workspace);
    let tab = workspace.close_tab(0, 0, 1).unwrap();
    workspace.remember_closed(ClosedTab {
        checkout: "/src/app".into(),
        at: 1,
        tab,
        addresses: Vec::new(),
        files: Vec::new(),
        changes: Vec::new(),
    });
    let (closed, at) = workspace.take_closed().unwrap();
    assert_eq!(at, (0, 0, 1));
    workspace.restore_tab(at, closed.tab);
    assert_eq!(ids(&workspace), before);
    assert_eq!(workspace.branch().unwrap().active_tab, 1);
    assert!(
        workspace.take_closed().is_none(),
        "each closed tab comes back once"
    );
}

#[test]
fn closed_tabs_of_a_checkout_gone_since_are_let_go() {
    let mut workspace = unit_with_tabs(2, 0);
    for (checkout, at) in [("/src/app", 0), ("/work/gone", 0)] {
        let tab = workspace.close_tab(0, 0, 0).unwrap();
        workspace.remember_closed(ClosedTab {
            checkout: checkout.into(),
            at,
            tab,
            addresses: Vec::new(),
            files: Vec::new(),
            changes: Vec::new(),
        });
    }
    // The newest is of a checkout no longer listed, so the one before it comes back.
    let (_, at) = workspace.take_closed().unwrap();
    assert_eq!(at, (0, 0, 0));
    assert!(workspace.take_closed().is_none());
}

#[test]
fn only_the_newest_closed_tabs_are_kept() {
    let mut workspace = unit_with_tabs(1, 0);
    let tab = workspace.branch().unwrap().tabs[0].clone();
    for at in 0..closed::KEPT + 5 {
        workspace.remember_closed(ClosedTab {
            checkout: "/src/app".into(),
            at,
            tab: tab.clone(),
            addresses: Vec::new(),
            files: Vec::new(),
            changes: Vec::new(),
        });
    }
    let mut kept = 0;
    while workspace.take_closed().is_some() {
        kept += 1;
    }
    assert_eq!(kept, closed::KEPT);
}

#[test]
fn a_tab_moves_to_another_checkout_and_shows_there() {
    let mut workspace = workspace(&["feat"]);
    for title in ["one", "two"] {
        workspace
            .unit_mut(0, 0)
            .unwrap()
            .tabs
            .push(Tab::Single(Pane {
                title: title.into(),
                ..Pane::new(PaneKind::Chat)
            }));
    }
    workspace
        .unit_mut(0, 1)
        .unwrap()
        .tabs
        .push(Tab::Single(Pane {
            title: "three".into(),
            ..Pane::new(PaneKind::Chat)
        }));
    let titles = |workspace: &Workspace, branch: usize| -> Vec<String> {
        (workspace.projects[0].branches[branch].tabs.iter())
            .map(|tab| tab.first().title.to_string())
            .collect()
    };
    assert!(workspace.move_tab_to_checkout((0, 0, 1), (0, 1), 0));
    assert_eq!(titles(&workspace, 0), ["one"]);
    assert_eq!(titles(&workspace, 1), ["two", "three"]);
    assert_eq!(
        workspace.shown_tab(),
        Some((0, 1, 0)),
        "it shows where it went"
    );
    // Past the end, it goes last; on its own checkout, nothing moves.
    assert!(workspace.move_tab_to_checkout((0, 0, 0), (0, 1), 9));
    assert_eq!(titles(&workspace, 1), ["two", "three", "one"]);
    assert!(!workspace.move_tab_to_checkout((0, 1, 0), (0, 1), 2));
    assert!(!workspace.move_tab_to_checkout((0, 1, 0), (0, 5), 0));
}

#[test]
fn projects_are_called_what_they_were_named_in_koyo_or_their_folder() {
    let mut workspace = Workspace::new(&["/src/app".into(), "/src/site".into()]);
    let mut looks = crate::spaces::ProjectLooks::new();
    looks.insert(
        "/src/site".into(),
        crate::spaces::ProjectLook {
            name: Some("Website".into()),
            icon: None,
        },
    );
    workspace.name_projects(&looks);
    assert_eq!(workspace.projects[0].name, "app");
    assert_eq!(workspace.projects[1].name, "Website");
    workspace.name_projects(&crate::spaces::ProjectLooks::new());
    assert_eq!(workspace.projects[1].name, "site");
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
    assert_eq!(workspace.active_project(), Some(2));
    assert_eq!(workspace.projects.len(), 3);
}

#[test]
fn showing_no_project_answers_none_everywhere_until_one_is_selected() {
    let mut workspace = Workspace::new(&["/a".into(), "/b".into()]);
    workspace.select_project(None);
    assert!(workspace.project().is_none());
    assert!(workspace.branch().is_none() && workspace.shown_tab().is_none());
    workspace.select_project(Some(5));
    assert!(workspace.project().is_none(), "past the list is none too");
    workspace.select_project(Some(1));
    assert_eq!(workspace.active_project(), Some(1));
}

#[test]
fn removing_the_selected_last_project_selects_the_one_before() {
    let mut workspace = Workspace::new(&["/a".into(), "/b".into()]);
    workspace.select_project(Some(1));
    workspace.remove_project(1);
    assert_eq!(workspace.active_project(), Some(0));
    workspace.remove_project(0);
    assert!(workspace.project().is_none());
}

#[test]
fn a_workspace_without_projects_has_nothing_selected() {
    let mut workspace = Workspace::default();
    workspace.new_tab(PaneKind::Chat);
    assert!(workspace.branch().is_none());
}
