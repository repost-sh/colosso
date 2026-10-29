mod arc;
mod backdrop;
mod bottom;
mod changes;
mod checkout;
mod checkouts;
mod delete_worktree;
mod files;
mod go_to_file;
mod gradient;
mod key_repeat;
mod language;
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
mod projects;
mod right_panel;
mod settings;
mod settings_view;
mod shell;
mod sidebar;
#[cfg(any(test, feature = "visual-check"))]
mod test_support;
mod theme;
#[cfg(feature = "visual-check")]
mod visual;

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
    key_repeat::init(cx);
    theme::init(cx);
    shell::init(cx);
    right_panel::init(cx);
    files::init(cx);
    navigation::bind_keys(cx);
    go_to_file::init(cx);
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
            title: Some("Colosso".into()),
            appears_transparent: true,
            traffic_light_position: Some(point(
                px(12.),
                px((theme::TITLE_BAR_HEIGHT - theme::TRAFFIC_LIGHT_SIZE) / 2.),
            )),
        }),
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        theme::follow_window_appearance(window);
        let shell = cx.new(|cx| shell::Shell::new(window, cx));
        // gpui-kit's root paints the theme background by default, covering the blur.
        cx.new(|cx| Root::new(shell, window, cx).bg(transparent_black()))
    })
    .map(|_| ())
}

fn main() -> ExitCode {
    logs::init();
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
    #[cfg(all(target_os = "macos", feature = "visual-check"))]
    if std::env::args().any(|arg| arg == "--visual-check-native-controls") {
        return settings_view::native_visual::run();
    }
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check-accent") {
        return theme::visual::run();
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
        // Not in `init`, which tests and the visual check share: they run servers of
        // their own, or none.
        language::init(cx);
        #[expect(clippy::expect_used, reason = "with no window there's no app to run")]
        open_main_window(cx).expect("failed to open window");
    });
    ExitCode::SUCCESS
}
