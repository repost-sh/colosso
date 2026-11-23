//! Appearance: light, dark or the system's, the gradient glass of each, how much of it the card
//! lets through and how blurred it is, the wallpaper behind chats, and the fonts.

use gpui_kit::App;
use gpui_kit::component::ActiveTheme as _;

use super::components::{MenuButton, PillButton, SettingRow, SettingsSection, slider};
use super::native_slider::NativeSlider;
use super::{chat_backdrop, fonts};
use crate::gradient::{self, GradientTheme};
use crate::picker;
use crate::settings::GLASS_STOPS;
use crate::theme;

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    let preference = theme::preference(cx);
    let appearance = if cx.theme().mode.is_dark() {
        "dark"
    } else {
        "light"
    };
    let glass_is_default = *gradient::current(cx) == GradientTheme::default();
    vec![
        SettingsSection::new("Theme")
            .row(
                SettingRow::new("Appearance")
                    .description("Follow the system, or keep Koyo light or dark.")
                    .control(MenuButton::new("appearance", preference.label(), |cx| {
                        theme::preference(cx).menu()
                    })),
            )
            .row(
                SettingRow::new("Glass")
                    .description(format!(
                        "The gradient tinting the window's glass in the {appearance} appearance. \
                     Light and dark each keep their own."
                    ))
                    .control(
                        PillButton::new("reset-glass", "Reset")
                            .disabled(glass_is_default)
                            .on_click(|_, _, cx| {
                                gradient::update(cx, |glass| *glass = GradientTheme::default());
                            }),
                    )
                    .control(PillButton::new("edit-glass", "Edit…").on_click(
                        |event, window, cx| {
                            picker::open(event.position(), window, cx);
                        },
                    )),
            )
            .rows(glass_rows()),
        chat_backdrop::section(cx),
        fonts::section(cx),
    ]
}

/// The rows for the window's glass: how much of it the card lets through, and how blurred it is.
fn glass_rows() -> [SettingRow; 2] {
    let card = NativeSlider::new(
        "card-glass",
        &GLASS_STOPS,
        |settings| settings.card_glass,
        |settings, stop| settings.card_glass = stop,
    );
    let blur = NativeSlider::new(
        "glass-blur",
        &GLASS_STOPS,
        |settings| settings.glass_blur,
        |settings, stop| settings.glass_blur = stop,
    );
    [
        slider(
            (
                "Card glass",
                "How much of the window's glass shows through the card: the middle \
                 is the theme's own, the right end solid.",
            ),
            card,
            ("Clear", "Solid"),
        ),
        slider(
            (
                "Glass blur",
                "How blurred the desktop behind the window looks through its \
                 glass: the middle is macOS's own, the left end none.",
            ),
            blur,
            ("None", "More"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::sections;
    use crate::settings::{self, Layout};
    use crate::theme;

    /// Appearance has the card's glass and its blur in either layout, and the card is as opaque
    /// as it's set in both.
    #[gpui_kit::test]
    fn the_card_glass_shows_and_tells_in_either_layout(cx: &mut TestAppContext) {
        cx.update(crate::test_support::init);
        let card = |cx: &mut TestAppContext| {
            cx.read(|cx| {
                let shown = sections(cx)
                    .iter()
                    .any(|section| section.matches("card glass") && section.matches("glass blur"));
                (shown, theme::palette(cx).card.a)
            })
        };
        let set = |layout, at, cx: &mut TestAppContext| {
            cx.update(|cx| {
                settings::update(cx, |settings| {
                    (settings.layout, settings.card_glass) = (layout, at);
                });
            });
        };
        for layout in Layout::ALL {
            set(layout, 1., cx);
            assert_eq!(card(cx), (true, 1.), "{layout:?}: solid");
        }
    }
}
