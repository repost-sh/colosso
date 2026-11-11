mod accounts;
mod agent_desktop;
mod ago;
mod arc;
mod atomic_file;
mod backdrop;
mod bottom;
mod browser;
mod changes;
mod chat;
mod checkout;
mod checkouts;
mod control;
mod delete_worktree;
mod devices;
mod fade;
mod files;
mod former_name;
mod go_to_file;
mod gradient;
mod held;
mod history;
mod kept_file;
mod key_repeat;
mod language;
mod launcher;
mod list;
mod logs;
mod menu;
mod modal;
mod model;
#[cfg(target_os = "macos")]
mod native;
mod navigation;
mod new_worktree;
mod orca;
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
mod sidebar;
mod spaces;
mod symbols;
mod terminals;
#[cfg(any(test, feature = "visual-check"))]
mod test_support;
mod theme;
mod toggle;
mod viewer;
#[cfg(feature = "visual-check")]
mod visual;
mod wrapped;

use std::process::ExitCode;

use gpui_kit::base::Root;
use gpui_kit::{
    App, AppContext as _, Styled as _, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, point, px, size, transparent_black,
};

/// What the app sets up before its first window, the visual check's included.
fn init(cx: &mut App) {
    gpui_kit::init(cx);
    settings::init(cx);
    settings_view::init(cx);
    key_repeat::init(cx);
    theme::init(cx);
    shell::init(cx);
    history::init(cx);
    launcher::init(cx);
    quitting::init(cx);
    agent_desktop::init(cx);
    accounts::init(cx);
    agent_desktop::bind_keys(cx);
    devices::init(cx);
    devices::bind_keys(cx);
    browser::init(cx);
    right_panel::init(cx);
    files::init(cx);
    viewer::init(cx);
    navigation::bind_keys(cx);
    go_to_file::init(cx);
    chat::init(cx);
    symbols::init(cx);
    koyo_term::init(cx);
    terminals::init(cx);
    #[cfg(target_os = "macos")]
    native::init(cx);
}

/// The app's window: most of the screen, so the chat, the sidebar and the changes fit side
/// by side.
fn open_main_window(cx: &mut App) -> gpui_kit::Result<()> {
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
        window_bounds: Some(WindowBounds::centered(window, cx)),
        window_min_size: Some(size(px(820.), px(520.))),
        window_background: WindowBackgroundAppearance::Blurred,
        // The rails are custom chrome, so the shell moves the window itself.
        app_owns_titlebar_drag: true,
        titlebar: Some(TitlebarOptions {
            title: Some("Koyo".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(
                px(theme::TRAFFIC_LIGHTS_LEFT),
                px(theme::TRAFFIC_LIGHTS_TOP),
            )),
        }),
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        theme::follow_window_appearance(window);
        let shell = cx.new(|cx| shell::Shell::new(window, cx));
        // gpui-kit's root paints the theme background by default, covering the blur.
        cx.new(|cx| Root::new(shell, window, cx).bg(transparent_black()))
    })?;
    popup::refresh_edge(opened.into(), cx);
    Ok(())
}

fn main() -> ExitCode {
    // `koyo termd`: the terminal daemon, which the app runs from its own executable so
    // the two never come apart. It keeps its own log, and needs nothing of the app's.
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("termd") => return koyo_termd::main(args),
        // `koyo app`: the app's operations from a shell, called on the running app's
        // control socket (docs/app-control.md).
        Some("app") => return koyo_app_control::cli_main(args),
        // `koyo mcp`: the terminals' MCP server an agent starts over stdio, which starts
        // the daemon from this executable when none runs, in the folder Colosso's moves to.
        Some("mcp") => {
            former_name::move_data();
            return koyo_term_mcp::main(args, terminals::daemon_command());
        }
        _ => {}
    }
    former_name::move_logs();
    logs::init();
    former_name::move_data();
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
    if std::env::args().any(|arg| arg == "--visual-check-browser") {
        return browser::visual::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-settings") {
        return settings_view::visual::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-chat") {
        return chat::visual::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-chat-theme") {
        return chat::visual::run_theme();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-chat-markdown") {
        return chat::visual::run_markdown();
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
    if std::env::args().any(|arg| arg == "--visual-check-arc") {
        return arc::visual::run();
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
    let app = gpui_kit::application()
        // The full Lucide set: the default bundle leaves out icons we use, which then draw nothing.
        .with_assets(gpui_kit::assets::AllAssets);
    // The dock icon clicked once the window's closed opens it again. A minimised window
    // isn't visible to macOS either, which restores it itself: hence the check.
    app.on_reopen(|cx| {
        if cx.windows().is_empty()
            && let Err(error) = open_main_window(cx)
        {
            log::error!("couldn't open the window again: {error:#}");
        }
    });
    app.run(|cx| {
        init(cx);
        quitting::set_menu(cx);
        // Not in `init`, which tests and the visual check share: they run servers of
        // their own, or none.
        language::init(cx);
        // Not in `init` either: tests drive app control without its socket.
        control::start(cx);
        #[expect(clippy::expect_used, reason = "with no window there's no app to run")]
        open_main_window(cx).expect("failed to open window");
        // Ready before the first page asks for it.
        browser::warm_up(cx);
    });
    ExitCode::SUCCESS
}
