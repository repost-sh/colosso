//! GPUI's keystrokes in the engine's terms: the key, by the W3C code the encoder reads, the
//! modifiers, and the character the key makes unshifted. GPUI names keys by the character
//! they type rather than by their place on the keyboard, so places are inferred for a US
//! layout; they only matter to the kitty protocol and to Ctrl combinations, which a
//! character names well enough.

use colosso_term_engine::{Key, KeyAction, KeyInput, Mods, OptionAsAlt, key_named};
use gpui_kit::{Keystroke, Modifiers};

/// A key event to encode, owned so it can cross to the VT thread.
#[derive(Clone, Debug)]
pub struct OwnedKey {
    pub key: Key,
    pub action: KeyAction,
    pub mods: Mods,
    pub consumed_mods: Mods,
    pub text: Option<String>,
    pub unshifted: Option<char>,
}

impl OwnedKey {
    pub fn as_input(&self) -> KeyInput<'_> {
        KeyInput {
            key: self.key,
            action: self.action,
            mods: self.mods,
            consumed_mods: self.consumed_mods,
            text: self.text.as_deref(),
            unshifted: self.unshifted,
            composing: false,
        }
    }
}

/// GPUI's modifiers as the encoders read them.
pub fn mods(modifiers: Modifiers) -> Mods {
    let mut mods = Mods::empty();
    mods.set(Mods::SHIFT, modifiers.shift);
    mods.set(Mods::CTRL, modifiers.control);
    mods.set(Mods::ALT, modifiers.alt);
    mods.set(Mods::SUPER, modifiers.platform);
    mods
}

/// Whether the keystroke types text the input method should deliver (so composition and
/// dead keys work), rather than a key to encode here.
pub fn types_text(keystroke: &Keystroke, option_as_alt: OptionAsAlt) -> bool {
    let modifiers = &keystroke.modifiers;
    let text = keystroke
        .key_char
        .as_deref()
        .is_some_and(|text| !text.is_empty() && !text.chars().any(char::is_control));
    // Control, Command, and Option when it's Alt make a key to encode, not text.
    let chord = modifiers.control
        || modifiers.platform
        || (modifiers.alt && option_as_alt != OptionAsAlt::No);
    text && !chord
}

/// The key event a keystroke makes, `None` for keys the terminal leaves to the app (anything
/// with Command, and keys it can't name).
pub fn encode(
    keystroke: &Keystroke,
    action: KeyAction,
    option_as_alt: OptionAsAlt,
) -> Option<OwnedKey> {
    let modifiers = &keystroke.modifiers;
    if modifiers.platform {
        return None;
    }
    let mut mods = mods(*modifiers);
    let (key, unshifted, shifted) = key_named(&keystroke.key)?;
    let mut consumed_mods = Mods::empty();
    if shifted {
        // GPUI reports Shift-1 as `!` with Shift taken off; the encoder wants it back on.
        mods.insert(Mods::SHIFT);
        consumed_mods.insert(Mods::SHIFT);
    }
    let alt_is_meta = modifiers.alt && option_as_alt != OptionAsAlt::No;
    let text = if alt_is_meta {
        // Option as Alt: the key's own character, not the one Option composes.
        unshifted.map(|c| {
            if modifiers.shift {
                c.to_uppercase().collect()
            } else {
                c.to_string()
            }
        })
    } else {
        keystroke
            .key_char
            .clone()
            .filter(|text| !text.chars().any(char::is_control))
    };
    if text.is_some() && modifiers.shift {
        consumed_mods.insert(Mods::SHIFT);
    }
    Some(OwnedKey {
        key,
        action,
        mods,
        consumed_mods,
        text,
        unshifted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(key: &str, key_char: Option<&str>, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.into(),
            key_char: key_char.map(Into::into),
        }
    }

    #[test]
    fn text_goes_through_the_input_method_and_the_rest_is_encoded() {
        let none = Modifiers::default();
        assert!(types_text(&stroke("a", Some("a"), none), OptionAsAlt::No));
        assert!(!types_text(
            &stroke("enter", Some("\n"), none),
            OptionAsAlt::No
        ));
        assert!(!types_text(
            &stroke("c", None, Modifiers::control()),
            OptionAsAlt::No
        ));
        let option = Modifiers {
            alt: true,
            ..Modifiers::default()
        };
        assert!(types_text(&stroke("s", Some("ß"), option), OptionAsAlt::No));
        assert!(!types_text(
            &stroke("s", Some("ß"), option),
            OptionAsAlt::Both
        ));
        assert!(
            encode(
                &stroke("c", None, Modifiers::command()),
                KeyAction::Press,
                OptionAsAlt::No
            )
            .is_none()
        );
    }

    #[test]
    fn keys_find_their_places() {
        let ctrl_c = encode(
            &stroke("c", None, Modifiers::control()),
            KeyAction::Press,
            OptionAsAlt::No,
        )
        .unwrap();
        assert_eq!(
            (ctrl_c.key, ctrl_c.unshifted, ctrl_c.mods),
            (Key::C, Some('c'), Mods::CTRL)
        );
        let bang = encode(
            &stroke("!", Some("!"), Modifiers::default()),
            KeyAction::Press,
            OptionAsAlt::No,
        )
        .unwrap();
        assert_eq!(
            (bang.key, bang.unshifted, bang.text.as_deref()),
            (Key::Digit1, Some('1'), Some("!"))
        );
        assert!(bang.mods.contains(Mods::SHIFT) && bang.consumed_mods.contains(Mods::SHIFT));
        let option = Modifiers {
            alt: true,
            ..Modifiers::default()
        };
        let meta = encode(
            &stroke("s", Some("ß"), option),
            KeyAction::Press,
            OptionAsAlt::Both,
        )
        .unwrap();
        assert_eq!(meta.text.as_deref(), Some("s"));
        let up = encode(
            &stroke("up", None, Modifiers::default()),
            KeyAction::Press,
            OptionAsAlt::No,
        )
        .unwrap();
        assert_eq!((up.key, up.text), (Key::ArrowUp, None));
    }
}
