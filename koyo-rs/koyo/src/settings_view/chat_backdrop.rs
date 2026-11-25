//! Appearance › Wallpaper background (`docs/chat-backdrop.md` 6): the wallpaper behind chats and
//! the rest, what it shows behind, and how it looks. Rows show only while they apply: with the switch off the section is the switch
//! alone, and the fade's own rows show only while it fades. Settings search looks at the rows
//! shown, so a switch that hides rows names what they set.

use gpui_kit::App;

use super::components::{SettingRow, SettingsSection, slider};
use super::native_slider::NativeSlider;
use super::native_switch::NativeSwitch;
use crate::chat_backdrop;
use crate::settings::{self, ChatBackdrop, Quality, Surface};
use crate::toggle::Toggle;

/// What the section's switch says first, on or off.
const WHAT: &str = "Your wallpaper behind chats, files, diffs, terminals and devices, dithered \
                    in its own colours.";

pub fn section(cx: &App) -> SettingsSection {
    let backdrop = settings::get(cx).chat_backdrop;
    let switch = SettingRow::new("Wallpaper background")
        .description(if backdrop.on {
            WHAT.to_owned()
        } else {
            format!(
                "{WHAT} Turn it on to choose what it shows behind (chats, explorers, files, \
                 diffs, terminals, simulators and agent desktop), and set its opacity, the \
                 cell size and density, the fade, where it starts and ends, and the quality."
            )
        })
        .control(NativeSwitch::new(
            "chat-backdrop",
            |settings| settings.chat_backdrop.on,
            |settings, on| settings.chat_backdrop.on = on,
        ));
    let section = SettingsSection::new("Wallpaper background").row(switch);
    if !backdrop.on {
        return section;
    }
    let mut surfaces = backdrop.surfaces;
    let section = section.rows(Surface::ALL.map(|surface| {
        let (id, name, behind) = surface.names();
        let flip = move |on, cx: &mut App| {
            settings::update(cx, |settings| {
                *settings.chat_backdrop.surfaces.switch(surface) = on;
            });
        };
        SettingRow::new(name)
            .description(behind)
            .control(Toggle::new(id, *surfaces.switch(surface), flip))
    }));
    let fade = SettingRow::new("Fade")
        .description(if backdrop.fade {
            "The wallpaper fades out towards the bottom of a chat."
        } else {
            "The wallpaper fades out towards the bottom of a chat. Turn it on to set where it \
             starts and ends."
        })
        .control(NativeSwitch::new(
            "chat-backdrop-fade",
            |settings| settings.chat_backdrop.fade,
            |settings, on| settings.chat_backdrop.fade = on,
        ));
    let quality = format!(
        "How sharp the wallpaper behind chats is kept, always compressed. {}",
        chat_backdrop::memory(backdrop.quality)
    );
    section
        .row(slider(
            ("Opacity", "How strongly the wallpaper shows over the chat."),
            NativeSlider::new(
                "chat-backdrop-opacity",
                &ChatBackdrop::OPACITY_STOPS,
                |settings| settings.chat_backdrop.opacity,
                |settings, stop| settings.chat_backdrop.opacity = stop,
            ),
            ("Less", "More"),
        ))
        .row(slider(
            (
                "Cell size",
                "How many screen pixels each cell of the dither is.",
            ),
            NativeSlider::new(
                "chat-backdrop-cell",
                &ChatBackdrop::CELL_STOPS,
                |settings| settings.chat_backdrop.cell,
                |settings, stop| settings.chat_backdrop.cell = stop,
            ),
            ("Fine", "Coarse"),
        ))
        .row(slider(
            ("Density", "How many of the cells are lit."),
            NativeSlider::new(
                "chat-backdrop-density",
                &ChatBackdrop::DENSITY_STOPS,
                |settings| settings.chat_backdrop.density,
                |settings, stop| settings.chat_backdrop.density = stop,
            ),
            ("Sparse", "Dense"),
        ))
        .row(fade)
        .rows(backdrop.fade.then(fade_rows).into_iter().flatten())
        .row(
            SettingRow::new("Quality").description(quality).control(
                NativeSlider::new(
                    "chat-backdrop-quality",
                    &ChatBackdrop::QUALITY_STOPS,
                    |settings| settings.chat_backdrop.quality.stop(),
                    |settings, stop| settings.chat_backdrop.quality = Quality::at(stop),
                )
                .ends("Smallest", "Sharpest"),
            ),
        )
}

/// The fade's own rows, shown while it fades.
fn fade_rows() -> [SettingRow; 2] {
    [
        slider(
            (
                "Fade starts",
                "How far down a chat the wallpaper starts to fade out.",
            ),
            NativeSlider::new(
                "chat-backdrop-fade-start",
                &ChatBackdrop::FADE_START_STOPS,
                |settings| settings.chat_backdrop.fade_start,
                |settings, stop| settings.chat_backdrop.fade_start = stop,
            ),
            ("High", "Low"),
        ),
        slider(
            ("Fade ends", "How far down a chat the wallpaper is gone."),
            NativeSlider::new(
                "chat-backdrop-fade-end",
                &ChatBackdrop::FADE_END_STOPS,
                |settings| settings.chat_backdrop.fade_end,
                |settings, stop| settings.chat_backdrop.fade_end = stop,
            ),
            ("High", "Low"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::section;
    use crate::settings;

    #[gpui_kit::test]
    fn its_rows_show_only_while_they_apply_and_search_finds_them_either_way(
        cx: &mut TestAppContext,
    ) {
        cx.update(crate::test_support::init);
        let finds = |words: &[&str], cx: &mut TestAppContext| {
            cx.read(|cx| words.iter().all(|word| section(cx).matches(word)))
        };
        let shows = |cx: &mut TestAppContext| cx.read(|cx| section(cx).matches("how strongly"));
        let all = [
            "chats",
            "explorers",
            "files",
            "diffs",
            "terminals",
            "agent desktop",
            "opacity",
            "cell size",
            "density",
            "fade",
            "starts",
            "ends",
            "quality",
        ];
        assert!(finds(&all, cx) && shows(cx), "on, as it starts");
        assert!(finds(&["is gone"], cx), "the fade's rows with it");
        assert!(!finds(&["sidebar"], cx), "it's on the window's glass alone");
        cx.update(|cx| settings::update(cx, |settings| settings.chat_backdrop.fade = false));
        assert!(finds(&all, cx), "the fade off, its switch names its rows");
        assert!(
            !cx.read(|cx| section(cx).matches("is gone")),
            "and they're gone"
        );
        cx.update(|cx| settings::update(cx, |settings| settings.chat_backdrop.on = false));
        assert!(finds(&all, cx), "off, the switch names every row");
        assert!(!shows(cx), "and the switch is all there is");
    }
}
