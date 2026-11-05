//! What text is in: the fonts a person picked, and how big a chat's text is. `theme::fonts` puts
//! them to use.

use gpui_kit::Action;
use serde::{Deserialize, Serialize};

/// The sizes a chat's text can be, in points, from the left of its slider.
pub const CHAT_SIZES: [f32; 7] = [13., 14., 15., 16., 18., 20., 22.];
/// A chat's text until a person says otherwise, which is what a rem is everywhere else.
const CHAT_SIZE: f32 = 16.;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fonts {
    /// The family of everything but code; with none, the system's.
    pub interface: Option<String>,
    /// The family of code; with none, the one the app ships.
    pub code: Option<String>,
    /// How big a chat's text is, in points.
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
        self.chat_size.clamp(CHAT_SIZES[0], CHAT_SIZES[CHAT_SIZES.len() - 1])
    }
}

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
        let at = |chat_size| Fonts { chat_size, ..Fonts::default() }.chat_size().to_bits();
        assert_eq!(at(1.), CHAT_SIZES[0].to_bits());
        assert_eq!(at(400.), CHAT_SIZES[CHAT_SIZES.len() - 1].to_bits());
        assert_eq!(at(16.), 16_f32.to_bits());
    }

    #[test]
    fn the_default_is_a_stop_of_the_slider() {
        assert!(CHAT_SIZES.iter().any(|stop| stop.to_bits() == CHAT_SIZE.to_bits()));
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
