//! Appearance › Chat background (`docs/chat-backdrop.md` 6): the wallpaper behind chats, and how
//! it looks. Rows show only while they apply: with the switch off the section is the switch
//! alone, and the blur's own rows show only while the blur is on. Settings search looks at the
//! rows shown, so a switch that hides rows names what they set.

use gpui_kit::App;

use super::components::{SettingRow, SettingsSection};
use super::native_slider::NativeSlider;
use super::native_switch::NativeSwitch;
use crate::chat_backdrop;
use crate::settings::{self, ChatBackdrop, Quality};

pub fn section(cx: &App) -> SettingsSection {
    let backdrop = settings::get(cx).chat_backdrop;
    let switch = SettingRow::new("Chat background")
        .description(if backdrop.on {
            "Your wallpaper, dimmed under a dot screen behind chats."
        } else {
            "Your wallpaper, dimmed under a dot screen behind chats. Turn it on to set the dim, \
             the dot size and density, the blur, its strength and where it starts, and the \
             quality."
        })
        .control(NativeSwitch::new(
            "chat-backdrop",
            |settings| settings.chat_backdrop.on,
            |settings, on| settings.chat_backdrop.on = on,
        ));
    let section = SettingsSection::new("Chat background").row(switch);
    if !backdrop.on {
        return section;
    }
    let blur = SettingRow::new("Blur")
        .description(if backdrop.blur {
            "The bottom of a chat fades into a blur of the wallpaper."
        } else {
            "The bottom of a chat fades into a blur of the wallpaper. Turn it on to set its \
             strength and where it starts."
        })
        .control(NativeSwitch::new(
            "chat-backdrop-blur",
            |settings| settings.chat_backdrop.blur,
            |settings, on| settings.chat_backdrop.blur = on,
        ));
    let quality = format!(
        "How sharp the wallpaper behind chats is kept, always compressed. {}",
        chat_backdrop::memory(backdrop.quality, cx)
    );
    section
        .row(slider(
            ("Dim", "How far the wallpaper fades into the chat's own colour."),
            NativeSlider::new(
                "chat-backdrop-dim",
                &ChatBackdrop::DIM_STOPS,
                |settings| settings.chat_backdrop.dim,
                |settings, stop| settings.chat_backdrop.dim = stop,
            ),
            ("Less", "More"),
        ))
        .row(slider(
            ("Dot size", "How wide the dots over the wallpaper are."),
            NativeSlider::new(
                "chat-backdrop-dot-size",
                &ChatBackdrop::DOT_SIZE_STOPS,
                |settings| settings.chat_backdrop.dot_size,
                |settings, stop| settings.chat_backdrop.dot_size = stop,
            ),
            ("Fine", "Coarse"),
        ))
        .row(slider(
            ("Density", "How close together the dots are, and how dark."),
            NativeSlider::new(
                "chat-backdrop-density",
                &ChatBackdrop::DENSITY_STOPS,
                |settings| settings.chat_backdrop.density,
                |settings, stop| settings.chat_backdrop.density = stop,
            ),
            ("Sparse", "Dense"),
        ))
        .row(blur)
        .rows(backdrop.blur.then(blur_rows).into_iter().flatten())
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

/// The blur's own rows, shown while it's on.
fn blur_rows() -> [SettingRow; 2] {
    [
        slider(
            ("Blur strength", "How far the blur at the bottom of a chat spreads."),
            NativeSlider::new(
                "chat-backdrop-blur-strength",
                &ChatBackdrop::BLUR_STRENGTH_STOPS,
                |settings| settings.chat_backdrop.blur_strength,
                |settings, stop| settings.chat_backdrop.blur_strength = stop,
            ),
            ("Soft", "Strong"),
        ),
        slider(
            ("Blur starts", "How far down a chat the blur starts to fade in."),
            NativeSlider::new(
                "chat-backdrop-blur-start",
                &ChatBackdrop::BLUR_START_STOPS,
                |settings| settings.chat_backdrop.blur_start,
                |settings, stop| settings.chat_backdrop.blur_start = stop,
            ),
            ("High", "Low"),
        ),
    ]
}

/// A row named and described by `said`, set by `slider` between the words `ends` say.
fn slider(
    said: (&'static str, &'static str),
    slider: NativeSlider,
    ends: (&'static str, &'static str),
) -> SettingRow {
    SettingRow::new(said.0)
        .description(said.1)
        .control(slider.ends(ends.0, ends.1))
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
        cx.update(crate::init);
        let finds = |words: &[&str], cx: &mut TestAppContext| {
            cx.read(|cx| words.iter().all(|word| section(cx).matches(word)))
        };
        let shows = |cx: &mut TestAppContext| cx.read(|cx| section(cx).matches("how wide"));
        let all = ["dim", "dot size", "density", "blur", "strength", "quality"];
        assert!(finds(&all, cx) && shows(cx), "on, as it starts");
        assert!(finds(&["spreads"], cx), "the blur's rows with it");
        cx.update(|cx| settings::update(cx, |settings| settings.chat_backdrop.blur = false));
        assert!(finds(&all, cx), "the blur off, its switch names its rows");
        assert!(!cx.read(|cx| section(cx).matches("spreads")), "and they're gone");
        cx.update(|cx| settings::update(cx, |settings| settings.chat_backdrop.on = false));
        assert!(finds(&all, cx), "off, the switch names every row");
        assert!(!shows(cx), "and the switch is all there is");
    }
}
