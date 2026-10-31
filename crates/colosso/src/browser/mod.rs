//! Browser panes: pages the browser engine packaged with the app renders, in a process of
//! its own.
//!
//! The engine (`engine`) is the browser host of `crates/browser-host` with CEF beside it,
//! packaged by `script/browser-engine` and carried with the app. One host serves every pane
//! of the app: `host` starts it with the first page and routes what it says to the pane
//! views. A page is a [`BrowserView`] in a tab: its frames are shared surfaces the host
//! paints into, shown in a native view over the pane (`frames`, `page`); the person's
//! pointer and keys go to it (`input`); its questions are asked in the app's dialogs
//! (`dialog`) and its context menu shown as the app's (`menu`); and who drives it, the
//! person or an agent, is a lease (`control`) the person always wins.

use std::path::Path;

use gpui_kit::{App, KeyBinding, actions};

mod control;
mod dialog;
mod dock;
mod engine;
mod find;
mod frames;
mod history;
mod host;
#[cfg(test)]
mod host_tests;
mod input;
mod keys;
mod menu;
mod page;
mod toolbar;
mod view;
#[cfg(feature = "visual-check")]
pub mod visual;

#[cfg(test)]
pub use dock::DOCK_MIN;
pub use dock::{DevtoolsPlace, Dock, SetDevtoolsDock};
pub use menu::handle_actions;
pub use view::{BrowserEvent, BrowserView};

/// Stops the engine, deletes every project's browsing data, and starts it again for the
/// pages open.
pub fn clear_data(cx: &mut App) {
    let browsers = host::Browsers::global(cx);
    browsers.update(cx, host::Browsers::clear_data);
}

/// The packaged engine's version and its Chromium's, once it runs.
pub fn versions(cx: &App) -> Option<(String, String)> {
    let browsers = cx.try_global::<host::GlobalBrowsers>()?;
    browsers.0.read(cx).versions()
}

/// Starts the engine ahead of the first page, while the person types its address.
pub fn warm_up(cx: &mut App) {
    let browsers = host::Browsers::global(cx);
    browsers.update(cx, host::Browsers::start_if_idle);
}

actions!(
    browser,
    [
        Back,
        Forward,
        Reload,
        FocusAddress,
        Copy,
        Cut,
        Paste,
        SelectAll,
        Undo,
        Redo,
        SendTab,
        SendBackTab,
        SendEscape,
        ToggleDevTools,
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

/// The keys a page owns while it has focus, and those the address bar shares with it; and
/// where `DevTools` open, picked in settings.
pub fn init(cx: &mut App) {
    cx.on_action(|SetDevtoolsDock(dock): &SetDevtoolsDock, cx| {
        crate::settings::update(cx, |settings| settings.devtools.dock = *dock);
    });
    let page = Some("BrowserPage");
    cx.bind_keys([
        KeyBinding::new("tab", SendTab, page),
        KeyBinding::new("shift-tab", SendBackTab, page),
        KeyBinding::new("escape", SendEscape, page),
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
            KeyBinding::new("cmd-l", FocusAddress, context),
            KeyBinding::new("cmd-[", Back, context),
            KeyBinding::new("cmd-]", Forward, context),
            KeyBinding::new("cmd-alt-i", ToggleDevTools, context),
            KeyBinding::new("f12", ToggleDevTools, context),
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

/// What the person typed in the address bar, as a page to load. Hosts and local addresses
/// load directly; anything else is a web search.
pub fn address_to_url(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    // Lowercased above, so the suffix check is as the lint asks for, case-insensitive.
    if LINK_SCHEMES
        .iter()
        .chain(&["data:", "chrome://"])
        .any(|scheme| lower.starts_with(scheme))
    {
        return Some(text.to_owned());
    }
    if text.contains(char::is_whitespace) {
        return Some(search_url(text));
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
        return Some(format!("http://{text}"));
    }
    if port || (name.contains('.') && !name.starts_with('.') && !name.ends_with('.')) {
        return Some(format!("https://{text}"));
    }
    Some(search_url(text))
}

/// A web search for `query`.
pub fn search_url(query: &str) -> String {
    let encoded: String = form_urlencoded::byte_serialize(query.as_bytes()).collect();
    format!("https://www.google.com/search?q={encoded}")
}

/// The profile a project's panes share: cookies, storage and cache never cross projects.
pub fn profile(project: &Path) -> String {
    // FNV-1a of the path: the name must stay the same across releases, or a project would
    // lose its logins.
    let hash = project
        .to_string_lossy()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
    let name = project
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(24)
        .collect::<String>()
        .to_ascii_lowercase();
    format!("{name}-{hash:016x}")
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

    #[test]
    fn each_project_has_its_own_stable_profile() {
        let a = profile(Path::new("/work/colosso"));
        assert_eq!(a, profile(Path::new("/work/colosso")));
        assert_ne!(a, profile(Path::new("/work/other")));
        assert!(a.starts_with("colosso-"));
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }
}
