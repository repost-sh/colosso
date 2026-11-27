mod accounts;
mod agent_desktop;
mod ago;
mod backdrop;
mod beat;
mod blocking;
mod bottom;
mod browser;
#[cfg(test)]
mod budget;
mod changes;
mod chat;
mod chat_backdrop;
mod checkout;
mod checkouts;
mod control;
mod delete_worktree;
mod devices;
mod edit_menu;
mod fade;
mod files;
mod former_name;
mod go_to_file;
mod gradient;
mod held;
mod history;
mod hosted;
mod kept_file;
mod key_repeat;
mod keyboard;
mod language;
mod launcher;
mod links;
mod list;
mod logs;
mod marks;
mod menu;
mod modal;
mod model;
#[cfg(target_os = "macos")]
mod native;
mod navigation;
mod new_worktree;
mod pane;
mod picker;
mod popup;
mod projects;
mod quitting;
mod responsive;
mod right_panel;
mod ring;
mod settings;
mod settings_view;
mod shell;
mod sight;
mod spaces;
#[cfg(target_os = "macos")]
mod stall;
mod surfaces;
mod symbols;
mod terminals;
#[cfg(any(test, feature = "visual-check"))]
mod test_support;
mod theme;
mod toggle;
mod updates;
mod viewer;
#[cfg(feature = "visual-check")]
mod visual;
mod window_options;
mod wrapped;

use std::process::ExitCode;

use gpui_kit::base::Root;
use gpui_kit::{
    Action, App, AppContext as _, Context, Menu, MenuItem, Styled as _, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, point, px, size, transparent_black,
};

use blocking::Blocking;

/// What the app sets up before its first window, the visual check's included.
fn init(cx: &mut App, may: Blocking<'_>) {
    gpui_kit::init(cx);
    settings::init(cx, may);
    updates::init(cx, may);
    settings_view::init(cx);
    key_repeat::init(cx);
    theme::init(cx);
    shell::init(cx);
    history::init(cx, may);
    launcher::init(cx);
    quitting::init(cx);
    agent_desktop::init(cx);
    accounts::init(cx);
    agent_desktop::bind_keys(cx);
    devices::init(cx);
    devices::bind_keys(cx);
    browser::init(cx, may);
    right_panel::init(cx);
    files::init(cx);
    viewer::init(cx);
    navigation::bind_keys(cx);
    shell::spaces::bind_keys(cx);
    go_to_file::init(cx);
    chat::init(cx, may);
    chat_backdrop::init(cx);
    symbols::init(cx);
    koyo_term::init(cx);
    terminals::init(cx);
    #[cfg(target_os = "macos")]
    native::init(cx);
    keyboard::init(cx);
    edit_menu::init(cx);
}

/// Opens the window again, as the Dock, a quit asked over it or a control client does, with
/// what its file holds, read off the main thread (`shell::reopen`); `opened` hears how that
/// went once it's open.
pub fn reopen_main_window(
    cx: &mut App,
    opened: impl FnOnce(&gpui_kit::Result<()>, &mut App) + 'static,
) {
    let open = |kept, cx: &mut App| open_main_window(cx, kept);
    shell::reopen(shell::KeptWindow::path(), open, opened, cx);
}

/// Opens `pages`, the pages macOS handed Koyo (`links.rs`), each as an unpinned page, the
/// last one shown, as a page opened from the launcher with no project is: a link from outside
/// belongs to no project. The window is opened again first if it was closed.
fn open_handed_pages(pages: Vec<String>, cx: &mut App) {
    in_main_window(cx, move |shell, window, cx| {
        for page in pages {
            shell.open_browser_tab(Some(page), (true, false), None, window, cx);
        }
    });
}

/// Check for Updates…, from the app's menu: the feed is read now, and Settings → About is
/// shown, whose Version row says it's checking and then what the feed answered
/// (`docs/updates.md` 1).
fn check_for_updates(cx: &mut App) {
    updates::check(cx);
    in_main_window(cx, |shell, _, cx| {
        shell.open_settings_page(Some(navigation::SettingsPage::About), cx);
    });
}

/// The menu bar (`docs/layout.md` Menu bar), set once every key is bound, so each item shows the
/// key bound to it. macOS gives the first menu the app's name, and lists the window in the one
/// titled Window. The bar adds no route for a key: `AppKit` offers a key to the window's views
/// before the menu, so an item shows only a key the window takes wherever the item is enabled. A
/// command of the window or the app is unbound, or bound with no context to a key with ⌘ or ⌃;
/// a command of a page is heard by the page's own view alone and bound in its contexts, so it's
/// disabled outside a page. Go to File is the one exception: its ⌘P is bound outside a page only
/// (`!BrowserPage`), and in a page ⌘P is the page's Print, so some binding takes ⌘P wherever the
/// keyboard is (`docs/native-layer.md` 7, `tests/menu_bar.rs`).
fn bar() -> Vec<Menu> {
    let named = |(name, action): (&'static str, Box<dyn Action>)| MenuItem::Action {
        name: name.into(),
        action,
        os_action: None,
        checked: false,
        disabled: false,
    };
    let help = |name, url: &str| MenuItem::action(name, files::OpenLinkedPage { url: url.to_owned() });
    let [worktree, project, space] = shell::sidebar::foot::new_commands().map(named);
    let edit = edit_menu::items().into_iter().chain([
        MenuItem::separator(),
        MenuItem::action("Find in Page…", browser::FindInPage),
        MenuItem::action("Find Next", browser::FindNext),
        MenuItem::action("Find Previous", browser::FindPrevious),
    ]);
    vec![
        Menu::new("Koyo").items([
            MenuItem::action("Check for Updates…", updates::CheckForUpdates),
            MenuItem::action("Settings…", navigation::OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Quit Koyo", quitting::Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Tab", launcher::NewTab),
            MenuItem::action("New Chat", shell::NewChat),
            MenuItem::action("New Terminal", shell::NewTerminal),
            MenuItem::separator(),
            worktree,
            project,
            space,
            MenuItem::separator(),
            MenuItem::action("Go to File…", go_to_file::GoToFile),
            MenuItem::action("Go to Symbol in File…", symbols::DocumentSymbols),
            MenuItem::action("Go to Symbol in Workspace…", symbols::WorkspaceSymbols),
            MenuItem::action("Save", files::Save),
            MenuItem::separator(),
            MenuItem::action("Close Tab", shell::CloseTab),
            MenuItem::action("Reopen Closed Tab", shell::ReopenClosedTab),
            MenuItem::separator(),
            MenuItem::action("Print…", browser::Print),
        ]),
        Menu::new("Edit").items(edit),
        Menu::new("View").items([
            MenuItem::action("Toggle Sidebar", shell::ToggleSidebar),
            MenuItem::action("Toggle Right Panel", shell::ToggleRightPanel),
            MenuItem::action("Toggle Bottom Panel", shell::ToggleBottomPanel),
            MenuItem::separator(),
            MenuItem::action("Files", right_panel::ShowFiles),
            MenuItem::action("Changes", right_panel::ToggleChanges),
            MenuItem::action("Downloads", browser::ToggleDownloads),
            MenuItem::separator(),
            MenuItem::action("Markdown Preview", files::TogglePreview),
            MenuItem::separator(),
            MenuItem::action("Edit Theme…", shell::EditTheme),
        ]),
        Menu::new("Page").items([
            MenuItem::action("Open Address…", launcher::OpenAddress),
            MenuItem::separator(),
            MenuItem::action("Back", browser::Back),
            MenuItem::action("Forward", browser::Forward),
            MenuItem::action("Reload", browser::Reload),
            MenuItem::separator(),
            MenuItem::action("Zoom In", browser::ZoomIn),
            MenuItem::action("Zoom Out", browser::ZoomOut),
            MenuItem::action("Actual Size", browser::ActualSize),
            MenuItem::separator(),
            MenuItem::action("Responsive Mode", browser::ToggleResponsive),
            MenuItem::action("DevTools", browser::ToggleDevTools),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", shell::Minimize),
            MenuItem::action("Zoom", shell::Zoom),
        ]),
        Menu::new("Help").items([
            help("Koyo Help", "https://github.com/repost-sh/koyo#readme"),
            help("Release Notes", "https://github.com/repost-sh/koyo/releases"),
        ]),
    ]
}

/// Has `act` act on the app's window, which is opened again first if it was closed.
fn in_main_window(
    cx: &mut App,
    act: impl FnOnce(&mut shell::Shell, &mut Window, &mut Context<shell::Shell>) + 'static,
) {
    if control::main_shell(cx).is_some() {
        return in_open_window(act, cx);
    }
    reopen_main_window(cx, move |opened, cx| match opened {
        Ok(()) => in_open_window(act, cx),
        Err(error) => log::error!("couldn't open the window again: {error:#}"),
    });
}

fn in_open_window(
    act: impl FnOnce(&mut shell::Shell, &mut Window, &mut Context<shell::Shell>),
    cx: &mut App,
) {
    let Some((window, shell)) = control::main_shell(cx) else {
        return;
    };
    let acted = window.update(cx, |_, window, cx| {
        shell.update(cx, |shell, cx| act(shell, window, cx));
    });
    if let Err(error) = acted {
        log::error!("couldn't reach the window: {error:#}");
    }
}

/// The app's window, bringing back what `kept` holds: most of the screen, so the chat, the
/// sidebar and the changes fit side by side.
fn open_main_window(cx: &mut App, kept: shell::KeptWindow) -> gpui_kit::Result<()> {
    let screen = cx
        .primary_display()
        .map_or(size(px(1440.), px(900.)), |display| {
            display.visible_bounds().size
        });
    let window = size(
        (screen.width * 0.9).min(px(1920.)),
        (screen.height * 0.9).min(px(1200.)),
    );
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(None, window, cx)),
        window_min_size: Some(size(px(820.), px(520.))),
        window_background: WindowBackgroundAppearance::Blurred,
        app_owns_titlebar_drag: true,
        titlebar: Some(TitlebarOptions {
            title: Some("Koyo".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(
                px(theme::TRAFFIC_LIGHTS_LEFT),
                px(theme::TRAFFIC_LIGHTS_TOP),
            )),
        }),
        ..window_options::base()
    };
    let opened = cx.open_window(options, |window, cx| {
        theme::follow_window_appearance(window);
        let shell = cx.new(|cx| shell::Shell::new(window, kept, cx));
        cx.new(|cx| Root::new(shell, window, cx).bg(transparent_black()))
    })?;
    popup::refresh_edge(opened.into(), cx);
    Ok(())
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("termd") => return koyo_termd::main(args),
        Some("app") => return koyo_app_control::cli_main(args),
        Some("mcp") => {
            blocking::at_launch("the data folder's move", former_name::move_data);
            return koyo_term_mcp::main(args, terminals::daemon_command());
        }
        Some(updates::INSTALL_UPDATE) => {
            return blocking::at_launch("the update's installer", |may| {
                logs::init();
                updates::main(args, may)
            });
        }
        _ => {}
    }
    blocking::at_launch("the folders' moves", |may| {
        former_name::move_logs(may);
        logs::init();
        former_name::move_data(may);
    });
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check") {
        return files::visual::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-finder") {
        return go_to_file::visual::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-lsp") {
        return language::visual::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-settings") {
        return settings_view::visual::run();
    }
    #[cfg(feature = "visual-check")]
    if let Some(draw) = std::env::args().find_map(|arg| chat::visual::check(&arg)) {
        return visual::run(draw);
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-chat-backdrop") {
        return chat_backdrop::visual::run();
    }
    #[cfg(all(target_os = "macos", feature = "visual-check"))]
    if std::env::args().any(|arg| arg == "--visual-check-native-controls") {
        return settings_view::native_visual::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-accounts") {
        return accounts::visual::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-accent") {
        return theme::visual::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-downloads") {
        return browser::check_downloads();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-sidebar") {
        return shell::sidebar::visual::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-devices-serve") {
        return devices::visual::serve::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-devices") {
        return devices::visual::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-terminal") {
        return terminals::visual::run();
    }
    #[cfg(all(target_os = "macos", feature = "visual-check"))]
    if std::env::args().any(|arg| arg == "--visual-check-switcher") {
        return shell::check_switcher();
    }
    #[cfg(target_os = "macos")]
    stall::watch_main();
    let app = gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets);
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            reopen_main_window(cx, |opened, _| {
                if let Err(error) = opened {
                    log::error!("couldn't open the window again: {error:#}");
                }
            });
        }
    });
    let (handed, links) = links::handed();
    app.on_open_urls(move |urls| handed.take(urls));
    app.run(move |cx| launch(links, cx));
    ExitCode::SUCCESS
}

/// What the app does once it runs: it sets itself up and opens its window, reading what it
/// kept on the main thread, timed (`blocking::at_launch`), and then opens the links macOS
/// handed it, the one it was started for among them.
fn launch(links: links::Links, cx: &mut App) {
    blocking::at_launch("the launch", |may| {
        init(cx, may);
        cx.on_action(|_: &updates::CheckForUpdates, cx| cx.defer(check_for_updates));
        language::init(cx);
        cx.set_menus(bar());
        control::start(cx);
        let kept = shell::KeptWindow::read(may);
        history::read_icons(&kept.addresses(), may, cx);
        #[expect(clippy::expect_used, reason = "with no window there's no app to run")]
        open_main_window(cx, kept).expect("failed to open window");
    });
    links::open_as_they_come(links, open_handed_pages, cx);
    #[cfg(all(target_os = "macos", debug_assertions, not(test)))]
    browser::native::probe::start(cx);
}

#[cfg(test)]
mod tests;
