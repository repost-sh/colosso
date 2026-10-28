mod arc;
mod backdrop;
mod changes;
mod delete_worktree;
mod gradient;
mod menu;
mod modal;
mod model;
mod navigation;
mod new_worktree;
mod orca;
mod pane;
mod picker;
mod projects;
mod settings;
mod settings_view;
mod shell;
mod sidebar;
mod theme;

use gpui_kit::base::Root;
use gpui_kit::{
    AppContext as _, Bounds, Styled as _, TitlebarOptions, WindowBackgroundAppearance,
    WindowBounds, WindowOptions, point, px, size, transparent_black,
};

fn main() {
    gpui_kit::application()
        // The full Lucide set: the default bundle leaves out icons we use, which then draw nothing.
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            settings::init(cx);
            theme::init(cx);
            shell::init(cx);
            changes::init(cx);
            navigation::bind_keys(cx);
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
            cx.open_window(options, |window, cx| {
                theme::follow_window_appearance(window);
                let shell = cx.new(|cx| shell::Shell::new(window, cx));
                // gpui-kit's root paints the theme background by default, covering the blur.
                cx.new(|cx| Root::new(shell, window, cx).bg(transparent_black()))
            })
            .expect("failed to open window");
        });
}
