use std::path::PathBuf;

use gpui_kit::point;

use super::*;
use crate::gradient::GradientTheme;

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
    // The sidebar reordered, as app control's project_move does: the space follows.
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
    settings.rename_project(&path("site"), "Website");
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

    let removed = settings.delete_space(second, Leftovers::MoveTo(0));
    assert!(removed.is_empty());
    assert_eq!(settings.spaces.len(), 2);
    assert_eq!(settings.spaces[0].projects, vec![path("app"), path("site")]);
    // The space shown stays shown though it moved up the list.
    assert_eq!(settings.space, 1);

    let removed = settings.delete_space(1, Leftovers::Remove);
    assert_eq!(removed, vec![path("docs")]);
    assert_eq!(settings.spaces.len(), 1);
    assert_eq!(settings.space, 0);
}

#[test]
fn the_last_space_is_never_deleted() {
    let mut settings = settings(&["app"]);
    settings.reconcile_spaces();
    assert!(settings.delete_space(0, Leftovers::Remove).is_empty());
    assert_eq!(settings.spaces.len(), 1);
    assert_eq!(settings.spaces[0].projects, vec![path("app")]);
}

#[test]
fn in_arc_each_space_keeps_its_own_colours_starting_from_the_windows() {
    let mut settings = settings(&[]);
    settings.layout = Layout::Arc;
    settings.dark_theme.dots = vec![point(100., 100.)];
    settings.reconcile_spaces();
    let second = settings.add_space();
    assert_eq!(settings.shown_theme(true), &settings.dark_theme);

    settings.space = second;
    settings.shown_theme_mut(true).dots.push(point(200., 200.));
    assert_eq!(settings.shown_theme(true).dots.len(), 2);
    // The window's, and the first space's with it, are as they were.
    assert_eq!(settings.dark_theme.dots.len(), 1);
    settings.space = 0;
    assert_eq!(settings.shown_theme(true).dots.len(), 1);
    // The Orca layout has no spaces: the window's colours are shown and changed.
    settings.space = second;
    settings.layout = Layout::Orca;
    assert_eq!(settings.shown_theme(true).dots.len(), 1);
}

#[test]
fn a_space_shows_as_its_own_colour_or_the_windows() {
    let mut settings = settings(&[]);
    settings.reconcile_spaces();
    assert_eq!(
        settings.space_colour(0, true),
        None,
        "no colour picked anywhere"
    );
    settings.dark_theme.dots = vec![point(100., 100.)];
    let window = settings.dark_theme.primary();
    assert_eq!(settings.space_colour(0, true), window);
    let own = GradientTheme {
        dots: vec![point(300., 120.)],
        ..GradientTheme::default()
    };
    settings.spaces[0].dark_theme = Some(own.clone());
    assert_eq!(settings.space_colour(0, true), own.primary());
    assert_eq!(settings.space_colour(5, true), None);
}

#[test]
fn a_project_is_renamed_and_given_its_folders_name_back() {
    let mut settings = settings(&["app"]);
    settings.rename_project(&path("app"), "  My App ");
    let name = |settings: &Settings| settings.project_looks.get(&path("app"))?.name.clone();
    assert_eq!(name(&settings).as_deref(), Some("My App"));
    settings.rename_project(&path("app"), "");
    assert_eq!(name(&settings), None);
    assert!(settings.project_looks.is_empty());
}

#[test]
fn spaces_survive_a_round_trip() {
    let mut settings = settings(&["app"]);
    settings.reconcile_spaces();
    settings.set_project_icon(&path("app"), Some("/pictures/app.png".into()));
    let json = serde_json::to_vec(&settings).unwrap();
    let read: Settings = serde_json::from_slice(&json).unwrap();
    assert_eq!(read, settings);
    // A file from when spaces had icons still reads.
    let older: Settings =
        serde_json::from_str(r#"{ "spaces": [{ "name": "Work", "icon": "rocket" }] }"#).unwrap();
    assert_eq!(older.spaces[0].name, "Work");
}
