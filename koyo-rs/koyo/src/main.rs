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
    App, AppContext as _, Styled as _, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, point, px, size, transparent_black,
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
    app.run(launch);
    ExitCode::SUCCESS
}

/// What the app does once it runs: it sets itself up and opens its window, reading what it
/// kept on the main thread, timed (`blocking::at_launch`).
fn launch(cx: &mut App) {
    blocking::at_launch("the launch", |may| {
        init(cx, may);
        quitting::set_menu(cx);
        language::init(cx);
        control::start(cx);
        let kept = shell::KeptWindow::read(may);
        history::read_icons(&kept.addresses(), may, cx);
        #[expect(clippy::expect_used, reason = "with no window there's no app to run")]
        open_main_window(cx, kept).expect("failed to open window");
    });
    #[cfg(all(target_os = "macos", debug_assertions, not(test)))]
    browser::native::probe::start(cx);
}
