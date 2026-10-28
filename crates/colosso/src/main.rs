use gpui_kit::{
    AppContext as _, Bounds, Context, Hsla, InteractiveElement as _, IntoElement, MouseButton,
    MouseDownEvent, ParentElement as _, Render, Styled as _, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowOptions, div, hsla, point, px, size,
};

const TITLE_BAR_HEIGHT: f32 = 40.;
const LEFT_RAIL_WIDTH: f32 = 48.;
const RAIL_GUTTER: f32 = 8.;
const CARD_RADIUS: f32 = 10.;
const TRAFFIC_LIGHT_SIZE: f32 = 14.;

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1180.), px(800.)),
                    cx,
                ))),
                window_min_size: Some(size(px(820.), px(520.))),
                window_background: WindowBackgroundAppearance::Blurred,
                // The rails are custom chrome, so the shell moves the window itself.
                app_owns_titlebar_drag: true,
                titlebar: Some(TitlebarOptions {
                    title: Some("Colosso".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(
                        px(12.),
                        px((TITLE_BAR_HEIGHT - TRAFFIC_LIGHT_SIZE) / 2.),
                    )),
                }),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Shell))
                .expect("failed to open window");
        });
}

struct Shell;

impl Shell {
    // The window's blur shows through this tint, so the alpha sets how glassy the rails look.
    fn rail_tint() -> Hsla {
        hsla(0., 0., 0.08, 0.55)
    }

    fn card_fill() -> Hsla {
        hsla(0., 0., 0.11, 1.)
    }

    fn hairline() -> Hsla {
        hsla(0., 0., 1., 0.08)
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let top_rail = div()
            .id("top-rail")
            .h(px(TITLE_BAR_HEIGHT))
            .flex_shrink_0()
            .on_mouse_down(MouseButton::Left, |event: &MouseDownEvent, window, _| {
                if event.click_count == 2 {
                    window.titlebar_double_click();
                } else {
                    window.start_window_move();
                }
            });

        let left_rail = div().w(px(LEFT_RAIL_WIDTH)).flex_shrink_0();

        let card = div()
            .flex_1()
            .mr(px(RAIL_GUTTER))
            .mb(px(RAIL_GUTTER))
            .rounded(px(CARD_RADIUS))
            .border_1()
            .border_color(Self::hairline())
            .bg(Self::card_fill());

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(Self::rail_tint())
            .child(top_rail)
            .child(div().flex_1().flex().child(left_rail).child(card))
    }
}
