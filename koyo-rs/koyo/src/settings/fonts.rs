//! What text is in: the fonts a person picked, and how big a chat's text is. `theme::fonts` puts
//! them to use.

use gpui_kit::Action;
use gpui_kit::component::native_menu::NativeMenu;
use serde::{Deserialize, Serialize};

use super::checked_menu;

/// The sizes a chat's text can be, in points, from the left of its slider: as dense as code at
/// the left, a book's at the right.
pub const CHAT_SIZES: [f32; 7] = [11., 12., 13., 14., 15., 16., 18.];
/// The biggest of them.
pub const LARGEST_CHAT_SIZE: f32 = CHAT_SIZES[CHAT_SIZES.len() - 1];
/// A chat's text until a person says otherwise: a notch under what a rem is everywhere else, as
/// a chat is read a lot of at a time.
const CHAT_SIZE: f32 = 14.;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fonts {
    /// The family of everything but code; with none, the system's.
    pub interface: Option<String>,
    /// The family of code; with none, the one the app ships.
    pub code: Option<String>,
    /// How big a chat's text is, in points. Named apart from the `chat_size` the sizes were a
    /// notch bigger under, which the migration moves to its place on this slider.
    #[serde(rename = "chat_text_size")]
    pub chat_size: f32,
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts {
            interface: None,
            code: None,
            chat_size: CHAT_SIZE,
        }
    }
}

impl Fonts {
    /// How big a chat's text is, kept within its slider's ends even if the file says otherwise.
    pub fn chat_size(&self) -> f32 {
        self.chat_size.clamp(CHAT_SIZES[0], LARGEST_CHAT_SIZE)
    }
}

/// What a chat's text size is called in a menu: "14 pt", and which is the default.
fn chat_size_label(size: f32) -> String {
    if size.to_bits() == CHAT_SIZE.to_bits() {
        format!("{size} pt (default)")
    } else {
        format!("{size} pt")
    }
}

/// The text sizes, with `current` checked.
pub fn chat_size_menu(current: f32) -> NativeMenu {
    checked_menu(CHAT_SIZES, current, chat_size_label, |size| {
        Box::new(SetChatTextSize(size))
    })
}

/// Sets how big a chat's text is, in points, from a menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetChatTextSize(pub f32);

/// Picks the family of everything but code, from the Appearance settings' menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetInterfaceFont(pub Option<String>);

/// Picks the family of code, from the Appearance settings' menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetCodeFont(pub Option<String>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_outside_the_slider_is_read_as_its_nearest_end() {
        let at = |chat_size| {
            Fonts {
                chat_size,
                ..Fonts::default()
            }
            .chat_size()
            .to_bits()
        };
        assert_eq!(at(1.), CHAT_SIZES[0].to_bits());
        assert_eq!(at(400.), LARGEST_CHAT_SIZE.to_bits());
        assert_eq!(at(16.), 16_f32.to_bits());
        assert_eq!(at(14.), CHAT_SIZE.to_bits());
    }

    #[test]
    fn the_default_is_a_stop_of_the_slider() {
        assert!(
            CHAT_SIZES
                .iter()
                .any(|stop| stop.to_bits() == CHAT_SIZE.to_bits())
        );
    }

    #[test]
    fn a_size_says_which_is_the_default() {
        assert_eq!(chat_size_label(14.), "14 pt (default)");
        assert_eq!(chat_size_label(11.), "11 pt");
    }

    #[test]
    fn a_file_naming_one_of_them_keeps_the_defaults_of_the_rest() {
        let fonts: Fonts = serde_json::from_str(r#"{ "code": "Fira Code" }"#).unwrap();
        assert_eq!(
            fonts,
            Fonts {
                code: Some("Fira Code".to_owned()),
                ..Fonts::default()
            }
        );
    }
}
