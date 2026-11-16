//! Extensions (`docs/extensions.md`), against an engine whose framework `script/cef` built
//! with Koyo's patches: one loaded from a folder is listed for its profile, its badge is
//! told, a press on its icon runs its action on the page with that page as the current tab
//! of a `normal` window, the tab it opens becomes a pane and the tab it closes is told, and
//! it's pinned, turned off and removed.
use std::path::Path;

use koyo_browser_ipc::{AppRequest, Extension, ExtensionChange, HostEvent};

use crate::support::{Session, VIEWPORT, close, launch, serve};

/// The profile the page is of.
const PROFILE: &str = "smoke";

/// Waits for the list of the profile's extensions that `want` accepts of the smoke test's
/// own extension, none when it isn't listed.
fn listed(session: &mut Session, what: &str, want: impl Fn(Option<&Extension>) -> bool) -> Option<Extension> {
    let mut found = None;
    session.wait(what, |event| match event {
        HostEvent::Extensions { profile, extensions } if profile == PROFILE => {
            let ours = extensions.iter().find(|extension| extension.name == "Koyo Smoke");
            found = ours.cloned();
            want(ours)
        }
        _ => false,
    });
    found
}

#[test]
#[ignore = "Needs a packaged browser engine built by script/cef; see the module docs"]
fn an_extension_is_listed_acts_on_the_page_opens_a_tab_and_is_pinned_and_removed() {
    let launch = launch("extensions");
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/smoke/extension");
    let switches = format!("load-extension={}", folder.display());
    let mut session = Session::start_with(&launch, &[("KOYO_NATIVE_SWITCHES", &switches)]);
    let address = serve("<title>page</title><body>hi</body>");
    (session.host)
        .send(&AppRequest::Open {
            pane: 1,
            url: address.clone(),
            viewport: VIEWPORT,
            profile: PROFILE.into(),
        })
        .expect("open");

    // The page loads, the extension is listed for its profile, and the badge it set for
    // every tab is told, in whichever order.
    let (mut loaded, mut ours, mut badge) = (false, None, false);
    session.wait("the page, the extension and its badge", |event| {
        match event {
            HostEvent::Page { pane: 1, state } => loaded = state.title == "page" && !state.loading,
            HostEvent::Extensions { profile, extensions } if profile == PROFILE => {
                ours = extensions.iter().find(|extension| extension.name == "Koyo Smoke").cloned();
            }
            HostEvent::ExtensionAction { profile, pane: None, action, .. } if profile == PROFILE => {
                badge |= action.badge_text == "7";
            }
            _ => {}
        }
        loaded && ours.is_some() && badge
    });
    let ours = ours.expect("listed");
    assert!(ours.enabled && ours.removable && !ours.pinned, "{ours:?}");
    let id = ours.id;

    // A press on its icon: Chrome runs the action on the page. The extension says what it
    // was pressed on, that the page is the current tab, and that its window is a normal one;
    // and the tab it opens is a pane the host gave a number, which loads.
    (session.host)
        .send(&AppRequest::Active { pane: 1 })
        .and_then(|()| {
            session.host.send(&AppRequest::ExtensionAct {
                pane: 1,
                extension: id.clone(),
                at: [200., 200.],
                corner: koyo_browser_ipc::Corner::Right,
            })
        })
        .expect("act");
    let (mut said, mut opened, mut got_tab) = (None, None, false);
    session.wait("the action run and its tab opened", |event| {
        match event {
            HostEvent::ExtensionAction { pane: None, action, .. } if action.title.starts_with('{') => {
                said = serde_json::from_str::<serde_json::Value>(&action.title).ok();
            }
            HostEvent::ExtensionAction { pane: Some(1), action, .. } => {
                got_tab |= action.badge_text == "tab";
            }
            HostEvent::TabOpened { pane, opener: 1, url, active: false } => {
                opened = Some((*pane, url.clone()));
            }
            _ => {}
        }
        said.is_some() && opened.is_some() && got_tab
    });
    let said = said.expect("said");
    assert_eq!(said["pressed"], address, "{said}");
    assert_eq!(said["current"], address, "the page pressed for is the current tab: {said}");
    assert_eq!(said["window"], "normal", "{said}");
    let (tab, url) = opened.expect("opened");
    assert!(tab >= 1 << 40, "a pane number of the host's own: {tab}");
    assert_eq!(url, format!("{address}opened"));
    session.wait("the opened tab's page to load", |event| {
        matches!(event, HostEvent::Page { pane, state } if *pane == tab && state.url == url && !state.loading)
    });

    // Pressed again, the extension closes the tab it opened: the app is told the tab went
    // without its asking, and that the pane closed.
    (session.host)
        .send(&AppRequest::ExtensionAct {
            pane: 1,
            extension: id.clone(),
            at: [200., 200.],
            corner: koyo_browser_ipc::Corner::Right,
        })
        .expect("act again");
    let (mut went, mut closed) = (false, false);
    session.wait("the tab the extension closed", |event| {
        match event {
            HostEvent::TabClosed { pane } => went |= *pane == tab,
            HostEvent::Closed { pane } => closed |= *pane == tab,
            _ => {}
        }
        went && closed
    });

    // Pinned, it says so; turned off, it's listed as off; removed, it's gone.
    let change = |session: &mut Session, change| {
        (session.host)
            .send(&AppRequest::ChangeExtension {
                profile: PROFILE.into(),
                extension: id.clone(),
                change,
            })
            .expect("change");
    };
    change(&mut session, ExtensionChange::Pinned { on: true });
    listed(&mut session, "the extension pinned", |ours| ours.is_some_and(|ours| ours.pinned));
    change(&mut session, ExtensionChange::Enabled { on: false });
    listed(&mut session, "the extension turned off", |ours| ours.is_some_and(|ours| !ours.enabled));
    change(&mut session, ExtensionChange::Remove);
    listed(&mut session, "the extension gone", |ours| ours.is_none());

    close(session, &launch);
}
