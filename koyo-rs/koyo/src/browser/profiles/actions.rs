//! What the person does with the profiles: putting a project on one from its Set Profile menu
//! or Settings → Browser, and making, renaming and deleting one, which ask first.

use std::path::{Path, PathBuf};

use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{Action, App, Div, InteractiveElement as _, Window};

use crate::arc::rename;
use crate::modal::{self, Confirm, Tone};
use crate::settings::{self, checked_menu};

/// Puts the project whose main checkout is at the path on the profile of this id.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct SetProjectProfile(pub PathBuf, pub String);

/// Has agents' pages signed in as this says.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct SetAgentPages(pub super::AgentPages);

/// Asks what to call a new profile, and puts the project whose main checkout is at the path
/// on it, when one's given.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct NewProfile(pub Option<PathBuf>);

/// Asks for the profile of this id's new name.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct RenameProfile(pub String);

/// Asks whether to delete the profile of this id, with all it keeps.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = browser, no_json)]
pub struct DeleteProfile(pub String);

/// The profiles to put the project at `project` on, the one it's on checked, then New
/// Profile…: a project tile's Set Profile, a project row's in the Orca layout, and the pick of
/// the project in Settings → Browser.
pub fn menu(project: &Path, cx: &App) -> NativeMenu {
    let profiles = super::get(cx);
    let owned = || project.to_owned();
    let ids = profiles.all().map(|(id, _)| id);
    let name = |id| profiles.name(id).to_owned();
    let pick = |id: &str| -> Box<dyn Action> { Box::new(SetProjectProfile(owned(), id.into())) };
    checked_menu(ids, profiles.of(project), name, pick)
        .separator()
        .menu("New Profile…", Box::new(NewProfile(Some(owned()))))
}

/// Picking a profile for a project, and what agents' pages are signed in as, needs no window.
pub(super) fn init(cx: &mut App) {
    cx.on_action(|SetProjectProfile(project, id): &SetProjectProfile, cx| {
        super::update(cx, |profiles| profiles.set(project, id));
    });
    cx.on_action(|SetAgentPages(agents): &SetAgentPages, cx| {
        super::update(cx, |profiles| profiles.agents = *agents);
    });
}

/// Has the window's root handle the profiles' actions that ask something first.
pub fn handle_actions(root: Div) -> Div {
    root.on_action(|NewProfile(project): &NewProfile, window, cx| {
        ask_new(project.clone(), window, cx);
    })
    .on_action(|RenameProfile(id): &RenameProfile, window, cx| {
        let name = super::get(cx).name(id).to_owned();
        let id = id.clone();
        let done = move |name: String, cx: &mut App| {
            super::update(cx, |profiles| profiles.rename(&id, &name));
        };
        rename::open("Rename Profile", name, "Profile name", window, cx, done);
    })
    .on_action(|DeleteProfile(id): &DeleteProfile, window, cx| {
        ask_delete(id.clone(), window, cx);
    })
}

/// Asks what to call a new profile; given a name, makes it and puts `project` on it.
fn ask_new(project: Option<PathBuf>, window: &mut Window, cx: &mut App) {
    let done = move |name: String, cx: &mut App| {
        if name.is_empty() {
            return;
        }
        super::update(cx, |profiles| {
            let id = profiles.make(&name);
            if let Some(project) = &project {
                profiles.set(project, &id);
            }
        });
    };
    let title = ("New Profile", "Create");
    rename::ask(title, String::new(), "Profile name", window, cx, done);
}

/// Asks whether to delete the profile of this id; confirmed, it goes from the list, its
/// projects onto Default, and its folder as the engine next starts, as one can't go from
/// under a running Chromium.
fn ask_delete(id: String, window: &mut Window, cx: &mut App) {
    let profiles = super::get(cx);
    if id == super::DEFAULT || !profiles.has(&id) {
        return;
    }
    let projects = match profiles.on(&id, &settings::get(cx).projects).count() {
        0 => String::new(),
        1 => " The project on it uses Default from now on.".to_owned(),
        on => format!(" The {on} projects on it use Default from now on."),
    };
    let question = Confirm {
        title: format!("Delete the profile “{}”?", profiles.name(&id)),
        text: format!("Its cookies, sign-ins and site data go for good.{projects}"),
        yes: "Delete",
        tone: Tone::Danger,
    };
    let delete = move |cx: &mut App| {
        super::update(cx, |profiles| profiles.delete(&id));
        crate::browser::host::forget_profile(&id, cx);
    };
    modal::confirm(question, delete, window, cx);
}
