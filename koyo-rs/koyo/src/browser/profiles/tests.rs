use std::path::{Path, PathBuf};

use gpui_kit::TestAppContext;

use super::*;
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
    // Back on Default, nothing's kept for it.
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
    // One the file names that's gone is Default.
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
    // Each keeps the folder it had, so it's signed in as it was.
    assert_eq!(profiles.of(Path::new("/work/colosso")), colosso);
    assert_eq!(profiles.name(&colosso), "colosso");
    assert_eq!(profiles.of(Path::new("/work/named")), named);
    assert_eq!(profiles.name(&named), "Client site");
    // A project whose pages kept nothing starts on Default, as a new one does.
    assert_eq!(profiles.of(Path::new("/work/agents")), DEFAULT);
    assert_eq!(profiles.made.len(), 2);
    // A project on a profile already is left be.
    profiles.set(Path::new("/work/colosso"), DEFAULT);
    profiles
        .projects
        .insert("/work/colosso".into(), named.clone());
    profiles.bring_over(&projects, &looks, |_| true);
    assert_eq!(profiles.of(Path::new("/work/colosso")), named);
}

#[gpui_kit::test]
fn the_profiles_come_back_from_their_file_and_are_brought_over_once(cx: &mut TestAppContext) {
    cx.update(settings::init);
    let colosso = PathBuf::from("/work/colosso");
    cx.update(|cx| {
        settings::update(cx, |settings| settings.projects = vec![colosso.clone()]);
    });
    let file = scratch("browser-profiles").join(FILE);
    // No file yet, as the first time this build starts: the project's own folder is brought
    // over, and kept at once.
    cx.update(|cx| open(Some(file.clone()), |_| true, cx));
    let brought = cx.read(|cx| get(cx).clone());
    assert_eq!(brought.of(&colosso), own(&colosso));
    assert_eq!(kept_file::read::<Profiles>(&file).as_ref(), Some(&brought));

    // Put on Default, with a profile made: it comes back so, and isn't brought over again.
    cx.update(|cx| {
        update(cx, |profiles| {
            profiles.set(&colosso, DEFAULT);
            profiles.make("Work");
        });
    });
    let changed = cx.read(|cx| get(cx).clone());
    kept_file::write_now(cx.update(due));
    cx.update(|cx| open(Some(file.clone()), |_| true, cx));
    assert_eq!(cx.read(|cx| get(cx).clone()), changed);
    assert_eq!(cx.read(|cx| get(cx).of(&colosso).to_owned()), DEFAULT);
}

#[gpui_kit::test]
fn an_open_page_moves_with_its_projects_profile(cx: &mut TestAppContext) {
    use gpui_kit::{AppContext as _, px, size};

    use crate::browser::BrowserView;
    use crate::browser::host::Browsers;
    use crate::test_support::in_window;

    cx.update(crate::init);
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

    // Open in Default, the project put on Work: it's to close and open again in Work.
    let open = page(cx);
    open.update(cx, BrowserView::pretend_opened);
    assert!(!moved(&open, cx));
    put_on(cx, &work);
    assert!(moved(&open, cx));
    put_on(cx, DEFAULT);
    assert!(!moved(&open, cx));

    // Not open, it opens in whichever the project's on then: nothing to move.
    let unopened = page(cx);
    put_on(cx, &work);
    assert!(!moved(&unopened, cx));
}
