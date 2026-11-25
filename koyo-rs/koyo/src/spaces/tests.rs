use std::path::PathBuf;

use super::*;

fn path(name: &str) -> PathBuf {
    PathBuf::from(format!("/src/{name}"))
}

/// Settings with the projects named, in no space yet.
fn settings(projects: &[&str]) -> Settings {
    Settings {
        projects: projects.iter().map(|name| path(name)).collect(),
        ..Settings::default()
    }
}

/// The id of the space at `at`.
fn id(settings: &Settings, at: usize) -> u64 {
    settings.spaces[at].id
}

#[test]
fn projects_from_before_spaces_all_join_a_first_space() {
    let mut settings = settings(&["app", "site"]);
    settings.reconcile_spaces();
    assert_eq!(settings.spaces.len(), 1);
    assert_eq!(settings.spaces[0].name, "Space 1");
    assert_eq!(settings.spaces[0].projects, vec![path("app"), path("site")]);
    assert_eq!(settings.spaces[0].selected, Some(path("app")));
    assert_eq!(settings.space, 0);
}

#[test]
fn a_space_lists_its_projects_in_the_sidebars_order() {
    let mut settings = settings(&["app", "site", "docs"]);
    settings.reconcile_spaces();
    let second = settings.add_space();
    settings.move_project(&path("docs"), second);
    settings.move_project(&path("app"), second);
    assert_eq!(
        settings.spaces[second].projects,
        vec![path("app"), path("docs")],
        "as the sidebar lists them, not as they came"
    );
    settings.projects = vec![path("docs"), path("site"), path("app")];
    settings.reconcile_spaces();
    assert_eq!(
        settings.spaces[second].projects,
        vec![path("docs"), path("app")]
    );
}

#[test]
fn a_new_project_joins_the_space_shown() {
    let mut settings = settings(&["app"]);
    settings.reconcile_spaces();
    settings.space = settings.add_space();
    settings.projects.push(path("site"));
    settings.reconcile_spaces();
    assert_eq!(settings.spaces[0].projects, vec![path("app")]);
    assert_eq!(settings.spaces[1].projects, vec![path("site")]);
    assert_eq!(settings.spaces[1].selected, Some(path("site")));
}

#[test]
fn a_removed_project_leaves_its_space_and_its_look() {
    let mut settings = settings(&["app", "site"]);
    settings.reconcile_spaces();
    settings.toggle_folded(&path("site"));
    settings.spaces[0].selected = Some(path("site"));
    settings.projects.retain(|project| *project != path("site"));
    settings.reconcile_spaces();
    assert_eq!(settings.spaces[0].projects, vec![path("app")]);
    assert_eq!(settings.spaces[0].selected, Some(path("app")));
    assert!(settings.project_looks.is_empty());
}

#[test]
fn a_project_held_twice_stays_in_the_first_space_only() {
    let mut settings = settings(&["app"]);
    settings.spaces = vec![Space::named("A"), Space::named("B")];
    settings.spaces[0].projects.push(path("app"));
    settings.spaces[1].projects.push(path("app"));
    settings.reconcile_spaces();
    assert_eq!(settings.spaces[0].projects, vec![path("app")]);
    assert!(settings.spaces[1].projects.is_empty());
}

#[test]
fn a_project_moves_to_another_space_and_its_old_space_shows_another() {
    let mut settings = settings(&["app", "site"]);
    settings.reconcile_spaces();
    let to = settings.add_space();
    settings.move_project(&path("app"), to);
    assert_eq!(settings.spaces[0].projects, vec![path("site")]);
    assert_eq!(settings.spaces[0].selected, Some(path("site")));
    assert_eq!(settings.spaces[1].projects, vec![path("app")]);
    assert_eq!(settings.spaces[1].selected, Some(path("app")));
}

#[test]
fn new_spaces_take_the_lowest_free_number() {
    let mut settings = settings(&[]);
    settings.reconcile_spaces();
    settings.add_space();
    settings.spaces[0].name = "Work".into();
    let third = settings.add_space();
    assert_eq!(settings.spaces[third].name, "Space 1");
    assert_eq!(settings.spaces[1].name, "Space 2");
}

#[test]
fn deleting_a_space_moves_its_projects_or_removes_them() {
    let mut settings = settings(&["app", "site", "docs"]);
    settings.reconcile_spaces();
    let second = settings.add_space();
    settings.move_project(&path("site"), second);
    let third = settings.add_space();
    settings.move_project(&path("docs"), third);
    settings.space = third;
    let (first, second, third) = (id(&settings, 0), id(&settings, second), id(&settings, third));

    let removed = settings.delete_space(second, Leftovers::MoveTo(first));
    assert!(removed.is_empty());
    assert_eq!(settings.spaces.len(), 2);
    assert_eq!(settings.spaces[0].projects, vec![path("app"), path("site")]);
    assert_eq!(settings.shown_space_id(), third);

    assert!(
        settings.delete_space(third, Leftovers::MoveTo(third)).is_empty(),
        "not into itself"
    );
    assert!(
        settings.delete_space(third, Leftovers::MoveTo(second)).is_empty(),
        "not into a space that's gone"
    );
    assert_eq!(settings.spaces.len(), 2);

    let removed = settings.delete_space(third, Leftovers::Remove);
    assert_eq!(removed, vec![path("docs")]);
    assert_eq!(settings.spaces.len(), 1);
    assert_eq!(settings.shown_space_id(), first);
}

#[test]
fn a_space_is_deleted_by_its_id_wherever_it_has_moved() {
    let mut settings = settings(&["app", "site"]);
    settings.reconcile_spaces();
    let second = settings.add_space();
    settings.move_project(&path("site"), second);
    let (first, second) = (id(&settings, 0), id(&settings, second));
    settings.move_space(1, 0);
    let removed = settings.delete_space(second, Leftovers::MoveTo(first));
    assert!(removed.is_empty());
    assert_eq!(settings.spaces.len(), 1);
    assert_eq!(settings.spaces[0].id, first);
    assert_eq!(settings.spaces[0].projects, vec![path("app"), path("site")]);
}

#[test]
fn a_space_moves_among_the_others_the_space_shown_staying_shown_and_the_order_kept() {
    let mut settings = settings(&[]);
    settings.reconcile_spaces();
    settings.add_space();
    settings.add_space();
    let names = |settings: &Settings| -> Vec<String> {
        (settings.spaces.iter())
            .map(|space| space.name.clone())
            .collect()
    };
    let shown = |settings: &Settings| settings.shown_space().map(|space| space.name.clone());
    settings.space = 1;

    settings.move_space(0, 2);
    assert_eq!(names(&settings), ["Space 2", "Space 3", "Space 1"]);
    assert_eq!(shown(&settings).as_deref(), Some("Space 2"), "the space shown stays");
    settings.move_space(0, 1);
    assert_eq!(names(&settings), ["Space 3", "Space 2", "Space 1"]);
    assert_eq!(shown(&settings).as_deref(), Some("Space 2"), "moved itself");
    settings.move_space(2, 0);
    assert_eq!(names(&settings), ["Space 1", "Space 3", "Space 2"]);
    assert_eq!(shown(&settings).as_deref(), Some("Space 2"));
    settings.move_space(1, 3);
    assert_eq!(names(&settings), ["Space 1", "Space 3", "Space 2"], "no such place");

    let json = serde_json::to_vec(&settings).unwrap();
    let read: Settings = serde_json::from_slice(&json).unwrap();
    assert_eq!(names(&read), ["Space 1", "Space 3", "Space 2"], "for the next launch");
    assert_eq!(shown(&read).as_deref(), Some("Space 2"));
}

#[test]
fn the_last_space_is_never_deleted() {
    let mut settings = settings(&["app"]);
    settings.reconcile_spaces();
    let only = id(&settings, 0);
    assert!(settings.delete_space(only, Leftovers::Remove).is_empty());
    assert_eq!(settings.spaces.len(), 1);
    assert_eq!(settings.spaces[0].projects, vec![path("app")]);
}

#[test]
fn a_projects_worktrees_keep_their_order_and_folding_until_git_no_longer_lists_them() {
    let mut settings = settings(&["app"]);
    let (app, feat, fix) = (path("app"), path("app-feat"), path("app-fix"));
    assert!(settings.worktree_order(&app).is_empty(), "git's order");
    settings.set_worktree_order(&app, vec![fix.clone(), app.clone(), feat.clone()]);
    settings.toggle_collapsed(&app, &feat);
    settings.toggle_collapsed(&app, &fix);
    assert!(settings.is_collapsed(&app, &feat));
    assert!(!settings.is_collapsed(&app, &app));
    settings.toggle_collapsed(&app, &fix);
    assert!(!settings.is_collapsed(&app, &fix), "opened again");
    settings.reconcile_spaces();
    assert_eq!(
        settings.worktree_order(&app),
        [fix.clone(), app.clone(), feat.clone()]
    );
    settings.keep_worktrees(&app, &[app.clone(), fix.clone()]);
    assert_eq!(settings.worktree_order(&app), [fix.clone(), app.clone()]);
    assert!(!settings.is_collapsed(&app, &feat));
    settings.set_worktree_order(&app, Vec::new());
    assert!(settings.project_looks.is_empty());
}

#[test]
fn spaces_survive_a_round_trip() {
    let mut settings = settings(&["app"]);
    settings.reconcile_spaces();
    settings.set_worktree_order(&path("app"), vec![path("app-feat"), path("app")]);
    settings.toggle_collapsed(&path("app"), &path("app-feat"));
    settings.toggle_folded(&path("app"));
    let json = serde_json::to_vec(&settings).unwrap();
    let read: Settings = serde_json::from_slice(&json).unwrap();
    assert_eq!(read, settings);
    let older: Settings =
        serde_json::from_str(r#"{ "spaces": [{ "name": "Work", "icon": "rocket" }] }"#).unwrap();
    assert_eq!(older.spaces[0].name, "Work");
    let older: Settings =
        serde_json::from_str(r#"{ "project_looks": { "/src/app": { "name": "App" } } }"#).unwrap();
    assert!(older.worktree_order(&path("app")).is_empty());
    assert!(!older.is_collapsed(&path("app"), &path("app")));
    assert!(!older.is_folded(&path("app")));
}

#[test]
fn a_project_folds_and_opens_again_and_forgets_it_once_open() {
    let mut settings = settings(&["app"]);
    let app = path("app");
    assert!(!settings.is_folded(&app));
    settings.toggle_folded(&app);
    assert!(settings.is_folded(&app));
    settings.toggle_folded(&app);
    assert!(!settings.is_folded(&app), "open again");
    assert!(settings.project_looks.is_empty(), "nothing's kept of an open project");
}
