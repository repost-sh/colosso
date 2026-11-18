//! Browser panes: pages of the browser engine packaged with the app, Chromium's own views
//! shown in Koyo's window (`docs/native-pages.md`).
//!
//! The engine (`engine`) is the browser host of `koyo-rs/koyo-browser-host` with CEF beside it,
//! packaged by `script/browser-engine` and carried in the app's bundle. The host runs in Koyo's
//! own process (`native`), which has to run from that bundle: run as a bare binary, it shows no
//! page. One host serves every pane of the app: `host` starts it as the app launches, so no
//! page waits for it, and routes what it says to the pane views. A page is a [`BrowserView`]
//! in a tab: its views are placed where the pane lays the page out (`page`), and `AppKit`
//! hands them the person's pointer and keys, of which the app hears a press and the edits it
//! binds (`input`); it can be shown at a chosen size or as a device (`responsive`); its
//! questions are asked in the app's dialogs (`dialog`) and its context menu shown as the app's
//! (`menu`); and who drives it, the person or an agent, is a lease (`control`) the person
//! always wins.

use std::path::Path;

use gpui_kit::{App, Entity, KeyBinding, actions};

pub mod agent;
#[cfg(test)]
mod budget;
pub(crate) mod control;
mod dialog;
mod dock;
mod engine;
mod extensions;
mod favicon;
mod find;
mod history;
mod host;
#[cfg(test)]
mod host_tests;
mod input;
pub(crate) mod keys;
mod lightness;
mod menu;
#[cfg(all(target_os = "macos", not(test)))]
pub(crate) mod native;
#[cfg(not(all(target_os = "macos", not(test))))]
#[path = "native_off.rs"]
pub(crate) mod native;
mod page;
mod passkeys;
pub(crate) mod pointer;
pub mod profiles;
mod responsive;
mod site;
mod tabs;
mod tone;
mod toolbar;
mod view;

pub mod fullscreen;
pub use dock::DOCK_MIN;
pub use dock::{DevtoolsPlace, Dock, SetDevtoolsDock};
pub use lightness::glass_dark;
pub use menu::handle_actions;
pub use responsive::{Responsive, devices};
pub use extensions::pinned::icons as pinned_extensions;
pub use site::{FADE as SITE_BUTTONS_FADE, buttons as site_buttons};
pub(crate) use tabs::page_brought_back;
pub use view::{BrowserEvent, BrowserView, Controls, Visit};

/// Starts the engine, so the first page opens as quickly as the next.
pub fn warm_up(cx: &mut App) {
    let browsers = host::Browsers::global(cx);
    browsers.update(cx, host::Browsers::start_if_idle);
}

/// Forgets every profile's browsing data, the open pages loading again afresh; and the pages
/// visited and the searches made, which the new-tab launcher offers.
pub fn clear_data(cx: &mut App) {
    let browsers = host::Browsers::global(cx);
    browsers.update(cx, host::Browsers::clear_data);
    crate::history::clear_browsing(cx);
}

/// The view of the page the host knows as `pane`, while it's open.
pub fn page_view(pane: koyo_browser_ipc::PaneId, cx: &mut App) -> Option<Entity<BrowserView>> {
    host::Browsers::global(cx).read(cx).view(pane)
}

/// Closes the page the engine has as its pane `pane`, which has no view: a tab an extension
/// opened that found no place among the app's.
pub fn close_page(pane: koyo_browser_ipc::PaneId, cx: &mut App) {
    let browsers = host::Browsers::global(cx);
    browsers.update(cx, |browsers, _| browsers.send(koyo_browser_ipc::AppRequest::Close { pane }));
}

/// The packaged engine's version and its Chromium's, once it runs.
pub fn versions(cx: &App) -> Option<(String, String)> {
    let browsers = cx.try_global::<host::GlobalBrowsers>()?;
    browsers.0.read(cx).versions()
}

actions!(
    browser,
    [
        Back,
        Forward,
        Reload,
        Copy,
        Cut,
        Paste,
        SelectAll,
        Undo,
        Redo,
        ToggleDevTools,
        ToggleResponsive,
        TakeControl,
        GiveBack,
        FindInPage,
        FindNext,
        FindPrevious,
        ZoomIn,
        ZoomOut,
        ActualSize
    ]
);

/// The edits a page takes while it has focus, and the keys the address bar and the find bar
/// share with it; and where `DevTools` open, picked in settings. ⌘L isn't among them: it's the
/// window's, heard wherever the keyboard is (`launcher::OpenAddress`).
pub fn init(cx: &mut App) {
    profiles::init(cx);
    cx.on_action(|SetDevtoolsDock(dock): &SetDevtoolsDock, cx| {
        crate::settings::update(cx, |settings| settings.devtools.dock = *dock);
    });
    let page = Some("BrowserPage");
    cx.bind_keys([
        KeyBinding::new("cmd-c", Copy, page),
        KeyBinding::new("cmd-x", Cut, page),
        KeyBinding::new("cmd-v", Paste, page),
        KeyBinding::new("cmd-a", SelectAll, page),
        KeyBinding::new("cmd-z", Undo, page),
        KeyBinding::new("cmd-shift-z", Redo, page),
    ]);
    for context in ["BrowserPage", "BrowserAddress", "BrowserFind"] {
        let context = Some(context);
        cx.bind_keys([
            KeyBinding::new("cmd-r", Reload, context),
            KeyBinding::new("cmd-[", Back, context),
            KeyBinding::new("cmd-]", Forward, context),
            KeyBinding::new("cmd-alt-i", ToggleDevTools, context),
            KeyBinding::new("f12", ToggleDevTools, context),
            KeyBinding::new("cmd-alt-r", ToggleResponsive, context),
            KeyBinding::new("cmd-f", FindInPage, context),
            KeyBinding::new("cmd-g", FindNext, context),
            KeyBinding::new("cmd-shift-g", FindPrevious, context),
            KeyBinding::new("cmd-=", ZoomIn, context),
            KeyBinding::new("cmd-+", ZoomIn, context),
            KeyBinding::new("cmd--", ZoomOut, context),
            KeyBinding::new("cmd-0", ActualSize, context),
        ]);
    }
}

/// Schemes of pages worth opening or copying as links.
pub const LINK_SCHEMES: [&str; 4] = ["http://", "https://", "file://", "about:"];

/// What the person typed in an address: a page to load, or words to search the web for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Address {
    Page(String),
    Search(String),
}

impl Address {
    /// The page it loads: the page itself, or the search's results.
    pub fn into_url(self) -> String {
        match self {
            Address::Page(url) => url,
            Address::Search(words) => search_url(&words),
        }
    }
}

/// What the person typed in the address bar, as a page to load. Hosts and local addresses
/// load directly; anything else is a web search.
pub fn address_to_url(text: &str) -> Option<String> {
    read_address(text).map(Address::into_url)
}

/// What the person typed in the address bar: hosts and local addresses are pages, anything
/// else words to search for.
pub fn read_address(text: &str) -> Option<Address> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if LINK_SCHEMES
        .iter()
        .chain(&["data:", "chrome://"])
        .any(|scheme| lower.starts_with(scheme))
    {
        return Some(Address::Page(text.to_owned()));
    }
    if text.contains(char::is_whitespace) {
        return Some(Address::Search(text.to_owned()));
    }
    let authority = lower.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let (name, port) = match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
            (name, true)
        }
        _ => (host, false),
    };
    let local = name == "localhost"
        || name.ends_with(".localhost")
        || Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("local"))
        || name.parse::<std::net::Ipv4Addr>().is_ok()
        || name.starts_with('[');
    if local {
        return Some(Address::Page(format!("http://{text}")));
    }
    if port || (name.contains('.') && !name.starts_with('.') && !name.ends_with('.')) {
        return Some(Address::Page(format!("https://{text}")));
    }
    Some(Address::Search(text.to_owned()))
}

/// A web search for `query`.
pub fn search_url(query: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(query.as_bytes()).collect();
    format!("https://www.google.com/search?q={encoded}")
}

/// What stands for a project where a page is none's (`docs/arc-sidebar.md` 11): a loose page
/// opened outside any project. Such pages use Default, the person's own, in every space.
pub const NO_PROJECT: &str = "no-project";

/// The profile a page opened for `project`, a main checkout or [`NO_PROJECT`], by `by` is of:
/// the one its project is on, Default outside any; an agent's, while agents' pages are kept
/// apart, the Agents profile (`profiles.rs`). Every way of opening a page asks here, so the
/// pages of a project are of one profile, and cookies, storage and cache never cross profiles.
pub fn profile<'a>(project: &Path, by: profiles::By, cx: &'a App) -> &'a str {
    profiles::get(cx).page(project, by)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_become_pages_and_everything_else_a_search() {
        for (typed, url) in [
            ("https://example.com/a?b", "https://example.com/a?b"),
            ("example.com", "https://example.com"),
            ("docs.rs/gpui", "https://docs.rs/gpui"),
            ("localhost:3000", "http://localhost:3000"),
            ("localhost", "http://localhost"),
            ("127.0.0.1:8080/x", "http://127.0.0.1:8080/x"),
            ("app.localhost:5173", "http://app.localhost:5173"),
            ("myhost:8080", "https://myhost:8080"),
            ("about:blank", "about:blank"),
            ("  example.com  ", "https://example.com"),
        ] {
            assert_eq!(address_to_url(typed).as_deref(), Some(url), "{typed}");
        }
        assert_eq!(
            address_to_url("rust borrow checker").as_deref(),
            Some("https://www.google.com/search?q=rust+borrow+checker")
        );
        assert_eq!(
            address_to_url("gpui").as_deref(),
            Some("https://www.google.com/search?q=gpui")
        );
        assert_eq!(
            address_to_url("c++ & co").as_deref(),
            Some("https://www.google.com/search?q=c%2B%2B+%26+co")
        );
        assert_eq!(address_to_url("   "), None);
    }
}
