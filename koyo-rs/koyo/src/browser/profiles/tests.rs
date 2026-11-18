use std::path::{Path, PathBuf};

use gpui_kit::TestAppContext;

use super::*;
use crate::blocking::in_test;
use crate::browser::NO_PROJECT;
use crate::spaces::ProjectLook;
use crate::test_support::scratch;

fn paths(projects: &[&str]) -> Vec<PathBuf> {
    projects.iter().map(PathBuf::from).collect()
}

#[test]
fn default_is_the_folder_pages_outside_any_project_kept() {
    assert_eq!(own(Path::new(NO_PROJECT)), DEFAULT);
    let profiles = Profiles::default();
    assert_eq!(profiles.of(Path::new(NO_PROJECT)), DEFAULT);
    assert_eq!(profiles.name(DEFAULT), "Default");
}

#[test]
fn each_project_had_a_stable_profile_of_its_own() {
    let a = own(Path::new("/work/koyo"));
    assert_eq!(a, own(Path::new("/work/koyo")));
    assert_ne!(a, own(Path::new("/work/other")));
    assert!(a.starts_with("koyo-"));
    assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
}

#[test]
fn a_project_is_on_default_until_put_on_another() {
    let mut profiles = Profiles::default();
    let projects = paths(&["/work/app"]);
    let app = Path::new("/work/app");
    assert_eq!(profiles.of(app), DEFAULT);
    let work = profiles.make("Work");
    profiles.set(app, &work);
    assert_eq!(profiles.of(app), work);
    assert_eq!(profiles.on(&work, &projects).collect::<Vec<_>>(), [app]);
    assert_eq!(profiles.on(DEFAULT, &projects).count(), 0);
    profiles.set(app, DEFAULT);
    assert_eq!(profiles.of(app), DEFAULT);
    assert!(profiles.projects.is_empty());
}

#[test]
fn a_profile_made_has_a_folder_no_other_had() {
    let mut profiles = Profiles::default();
    let a = profiles.make("Work ✨");
    let b = profiles.make("Work ✨");
    let blank = profiles.make("  ✨ ");
    assert_ne!(a, b);
    assert!(a.starts_with("work-") && blank.starts_with("profile-"));
    for id in [&a, &b, &blank] {
        assert!(
            id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "{id}"
        );
    }
    assert_eq!(profiles.name(&a), "Work ✨");
    let names: Vec<&str> = profiles.all().map(|(_, name)| name).collect();
    assert_eq!(names, ["Default", "Work ✨", "Work ✨", "✨"]);
}

#[test]
fn default_keeps_its_name_and_stays() {
    let mut profiles = Profiles::default();
    let work = profiles.make("Work");
    profiles.rename(DEFAULT, "Mine");
    profiles.rename(&work, "  ");
    assert_eq!(profiles.name(DEFAULT), "Default");
    assert_eq!(profiles.name(&work), "Work");
    profiles.rename(&work, " Client ");
    assert_eq!(profiles.name(&work), "Client");
    profiles.delete(DEFAULT);
    assert!(profiles.has(DEFAULT));
}

#[test]
fn a_profile_deleted_puts_its_projects_on_default() {
    let mut profiles = Profiles::default();
    let (a, b) = (Path::new("/work/a"), Path::new("/work/b"));
    let work = profiles.make("Work");
    let home = profiles.make("Home");
    profiles.set(a, &work);
    profiles.set(b, &home);
    profiles.delete(&work);
    assert!(!profiles.has(&work));
    assert_eq!(profiles.of(a), DEFAULT);
    assert_eq!(profiles.of(b), home);
    profiles.projects.insert(b.to_owned(), "gone-1".into());
    assert_eq!(profiles.of(b), DEFAULT);
}

#[test]
fn each_projects_own_profile_is_brought_over_with_its_sign_ins() {
    let projects = paths(&["/work/colosso", "/work/agents", "/work/named"]);
    let looks = ProjectLooks::from([(
        PathBuf::from("/work/named"),
        ProjectLook {
            name: Some("Client site".into()),
            ..ProjectLook::default()
        },
    )]);
    let colosso = own(Path::new("/work/colosso"));
    let named = own(Path::new("/work/named"));
    let kept = [colosso.clone(), named.clone(), DEFAULT.into()];
    let mut profiles = Profiles::default();
    profiles.bring_over(&projects, &looks, |id| kept.iter().any(|kept| kept == id));
    assert_eq!(profiles.of(Path::new("/work/colosso")), colosso);
    assert_eq!(profiles.name(&colosso), "colosso");
    assert_eq!(profiles.of(Path::new("/work/named")), named);
    assert_eq!(profiles.name(&named), "Client site");
    assert_eq!(profiles.of(Path::new("/work/agents")), DEFAULT);
    assert_eq!(profiles.made.len(), 2);
    profiles.set(Path::new("/work/colosso"), DEFAULT);
    profiles
        .projects
        .insert("/work/colosso".into(), named.clone());
    profiles.bring_over(&projects, &looks, |_| true);
    assert_eq!(profiles.of(Path::new("/work/colosso")), named);
}

#[test]
fn agents_pages_are_the_projects_or_the_agents_profiles_as_the_person_has_them() {
    let mut profiles = Profiles::default();
    let app = Path::new("/work/app");
    let work = profiles.make("Work");
    profiles.set(app, &work);
    assert_eq!(profiles.agents, AgentPages::Shared);
    assert_eq!(profiles.page(app, By::Agent), work);
    assert_eq!(profiles.page(app, By::Person), work);
    profiles.agents = AgentPages::Separate;
    assert_eq!(profiles.page(app, By::Agent), AGENTS);
    assert_eq!(profiles.page(Path::new(NO_PROJECT), By::Agent), AGENTS);
    assert_eq!(profiles.page(app, By::Person), work);
    assert_eq!(profiles.name(AGENTS), "Agents");
    assert!(!profiles.has(AGENTS));
    assert!(profiles.all().all(|(id, _)| id != AGENTS));
    profiles.set(app, AGENTS);
    assert_eq!(profiles.of(app), DEFAULT);
    profiles.rename(AGENTS, "Bots");
    profiles.delete(AGENTS);
    assert_eq!(profiles.name(AGENTS), "Agents");
    assert!(
        AGENTS
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    );
    let json = serde_json::to_value(&profiles).unwrap();
    assert_eq!(json["agents"], "separate");
    let back: Profiles = serde_json::from_value(json).unwrap();
    assert_eq!(back.agents, AgentPages::Separate);
    let before: Profiles = serde_json::from_str(r#"{"made": [], "projects": {}}"#).unwrap();
    assert_eq!(before.agents, AgentPages::Shared);
}

#[gpui_kit::test]
fn the_profiles_come_back_from_their_file_and_are_brought_over_once(cx: &mut TestAppContext) {
    cx.update(crate::test_support::init_settings);
    let colosso = PathBuf::from("/work/colosso");
    cx.update(|cx| {
        settings::update(cx, |settings| settings.projects = vec![colosso.clone()]);
    });
    let file = scratch("browser-profiles").join(FILE);
    cx.update(|cx| in_test(|may| open(Some(file.clone()), |_| true, may, cx)));
    cx.run_until_parked();
    let brought = cx.read(|cx| get(cx).clone());
    assert_eq!(brought.of(&colosso), own(&colosso));
    assert_eq!(in_test(|may| kept_file::read::<Profiles>(&file, may)).as_ref(), Some(&brought));

    cx.update(|cx| {
        update(cx, |profiles| {
            profiles.set(&colosso, DEFAULT);
            profiles.make("Work");
        });
    });
    let changed = cx.read(|cx| get(cx).clone());
    cx.update(|cx| kept_file::write_due(due(cx), cx));
    cx.run_until_parked();
    cx.update(|cx| in_test(|may| open(Some(file.clone()), |_| true, may, cx)));
    assert_eq!(cx.read(|cx| get(cx).clone()), changed);
    assert_eq!(cx.read(|cx| get(cx).of(&colosso).to_owned()), DEFAULT);
}

#[gpui_kit::test]
fn an_open_page_moves_with_its_projects_profile(cx: &mut TestAppContext) {
    use gpui_kit::{AppContext as _, px, size};

    use crate::browser::BrowserView;
    use crate::browser::host::Browsers;
    use crate::test_support::in_window;

    cx.update(crate::test_support::init);
    let app = Path::new("/work/app");
    let page = |cx: &mut TestAppContext| {
        in_window(size(px(800.), px(600.)), cx, |window, cx| {
            cx.new(|cx| BrowserView::new("https://example.com/".into(), app, window, cx))
        })
        .1
    };
    let put_on = |cx: &mut TestAppContext, id: &str| {
        cx.update(|cx| update(cx, |profiles| profiles.set(app, id)));
    };
    let epoch = cx.update(|cx| Browsers::global(cx).read(cx).epoch());
    let moved = |view: &gpui_kit::Entity<BrowserView>, cx: &mut TestAppContext| {
        view.read_with(cx, |view, cx| view.profile_moved(epoch, cx))
    };
    let work = cx.update(|cx| {
        let mut work = String::new();
        update(cx, |profiles| work = profiles.make("Work"));
        work
    });

    let open = page(cx);
    open.update(cx, BrowserView::pretend_opened);
    assert!(!moved(&open, cx));
    put_on(cx, &work);
    assert!(moved(&open, cx));
    put_on(cx, DEFAULT);
    assert!(!moved(&open, cx));

    let unopened = page(cx);
    put_on(cx, &work);
    assert!(!moved(&unopened, cx));

    let agents = |cx: &mut TestAppContext, pages: AgentPages| {
        cx.update(|cx| update(cx, |profiles| profiles.agents = pages));
    };
    let (agents_page, persons_page) = (page(cx), page(cx));
    agents_page.update(cx, |view, cx| {
        view.made_by(By::Agent);
        view.pretend_opened(cx);
    });
    persons_page.update(cx, BrowserView::pretend_opened);
    assert!(!moved(&agents_page, cx) && !moved(&persons_page, cx));
    agents(cx, AgentPages::Separate);
    assert!(moved(&agents_page, cx));
    assert!(!moved(&persons_page, cx));
    agents(cx, AgentPages::Shared);
    assert!(!moved(&agents_page, cx));
}
