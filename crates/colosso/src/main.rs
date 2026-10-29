mod arc;
mod backdrop;
mod changes;
mod checkout;
mod checkouts;
mod delete_worktree;
mod files;
mod gradient;
mod logs;
mod menu;
mod modal;
mod model;
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
#[cfg(test)]
mod test_support;
mod theme;

use std::process::ExitCode;

use gpui_kit::base::Root;
use gpui_kit::{
    App, AppContext as _, Bounds, Styled as _, TitlebarOptions, WindowBackgroundAppearance,
    WindowBounds, WindowOptions, point, px, size, transparent_black,
};

/// What the app sets up before its first window, the visual check's included.
fn init(cx: &mut App) {
    gpui_kit::init(cx);
    settings::init(cx);
    theme::init(cx);
    shell::init(cx);
    right_panel::init(cx);
    navigation::bind_keys(cx);
}

fn main() -> ExitCode {
    logs::init();
    #[cfg(feature = "visual-check")]
    if std::env::args().any(|arg| arg == "--visual-check") {
        return files::visual::run();
    }
    gpui_kit::application()
        // The full Lucide set: the default bundle leaves out icons we use, which then draw nothing.
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            init(cx);
            // Most of the screen, so the chat, the sidebar and the changes fit side by side.
            let screen = cx
                .primary_display()
                .map_or(size(px(1440.), px(900.)), |display| display.visible_bounds().size);
            let window = size(
                (screen.width * 0.9).min(px(1920.)),
                (screen.height * 0.9).min(px(1200.)),
            );
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, window, cx))),
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
            #[expect(clippy::expect_used, reason = "with no window there's no app to run")]
            cx.open_window(options, |window, cx| {
                theme::follow_window_appearance(window);
                let shell = cx.new(|cx| shell::Shell::new(window, cx));
                // gpui-kit's root paints the theme background by default, covering the blur.
                cx.new(|cx| Root::new(shell, window, cx).bg(transparent_black()))
            })
            .expect("failed to open window");
        });
    ExitCode::SUCCESS
}
