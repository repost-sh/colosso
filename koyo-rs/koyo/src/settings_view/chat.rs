//! The Chat settings page: how hard an agent works in a new chat, and how much of its work shows.

use gpui_kit::App;
use koyo_agent::protocol::model::Effort;
use koyo_agent::words::effort_label;

use super::components::{MenuButton, SettingRow, SettingsSection};
use super::native_slider::NativeSlider;
use super::ticked_slider::nearest;
use crate::settings;

/// The stops of the effort slider, one for each of [`Effort::ALL`].
const EFFORT_STOPS: [f32; 5] = [0., 1., 2., 3., 4.];

pub fn sections(cx: &App) -> Vec<SettingsSection> {
    vec![new_chats(), timeline(cx)]
}

fn effort_stop(effort: Effort) -> f32 {
    let mut stops = Effort::ALL.into_iter().zip(EFFORT_STOPS);
    stops.find(|(of, _)| *of == effort).map_or(0., |(_, stop)| stop)
}

fn effort_at(stop: f32) -> Effort {
    Effort::ALL.get(nearest(&EFFORT_STOPS, stop)).copied().unwrap_or(Effort::Xhigh)
}

fn new_chats() -> SettingsSection {
    SettingsSection::new("New chats").row(
        SettingRow::new("Effort")
            .description(
                "How hard the agent works in a chat, until it's set to something else from the \
                 chat. A model that can't work that hard takes the nearest it can.",
            )
            .control(
                NativeSlider::new(
                    "chat-effort",
                    &EFFORT_STOPS,
                    |settings| effort_stop(settings.chat_effort),
                    |settings, stop| settings.chat_effort = effort_at(stop),
                )
                .ends(effort_label(Effort::Low), effort_label(Effort::Max)),
            ),
    )
}

fn timeline(cx: &App) -> SettingsSection {
    let work = settings::get(cx).chat_work;
    SettingsSection::new("Timeline").row(
        SettingRow::new("Work to show")
            .description(
                "How much of the tools an agent ran and what it thought shows in a chat. What it \
                 said always shows.",
            )
            .control(MenuButton::new("chat-work", work.label(), |cx| {
                settings::get(cx).chat_work.menu()
            })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_effort_has_its_own_stop() {
        for effort in Effort::ALL {
            assert_eq!(effort_at(effort_stop(effort)), effort);
        }
    }
}
