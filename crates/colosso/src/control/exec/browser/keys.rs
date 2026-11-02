//! Keys as an agent names them, `Enter`, `ArrowDown`, `Meta+A`, made into the key events
//! the Chrome `DevTools` protocol dispatches, with the codes the person's own keys carry.

use colosso_app_control::ControlError;
use serde_json::{Value, json};

use crate::browser::keys::key_codes;

/// The protocol's modifier bits.
const ALT: u32 = 1;
const CTRL: u32 = 2;
const META: u32 = 4;
const SHIFT: u32 = 8;

/// One key with its modifiers, ready to dispatch.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Key {
    /// `KeyboardEvent.key`.
    name: String,
    /// `KeyboardEvent.code`.
    code: String,
    /// The Windows virtual key code Chromium keys events by, and the Mac key code.
    codes: (i32, i32),
    /// The text it types, for a key that types any.
    text: Option<String>,
    modifiers: u32,
}

impl Key {
    /// Parses `name`: modifiers and a key joined by `+`.
    pub(super) fn parse(name: &str) -> Result<Key, ControlError> {
        let parts: Vec<&str> = name.split('+').map(str::trim).collect();
        let (last, held) = parts
            .split_last()
            .filter(|(last, _)| !last.is_empty())
            .ok_or_else(|| ControlError::invalid_input("`key` is empty."))?;
        let mut modifiers = 0;
        for modifier in held {
            modifiers |= match modifier.to_ascii_lowercase().as_str() {
                "alt" | "option" => ALT,
                "ctrl" | "control" => CTRL,
                "meta" | "cmd" | "command" => META,
                "shift" => SHIFT,
                other => {
                    return Err(ControlError::invalid_input(format!(
                        "{other} isn't a modifier: use Alt, Ctrl, Meta or Shift."
                    )));
                }
            };
        }
        let (name, code, known, text) = named(last).or_else(|| typed(last)).ok_or_else(|| {
            ControlError::invalid_input(format!(
                "{last} isn't a key: name one as Enter, Tab, Escape, ArrowDown or a single character."
            ))
        })?;
        Ok(Key {
            name,
            code,
            codes: key_codes(&known),
            // A shortcut types nothing.
            text: text.filter(|_| modifiers & (CTRL | META) == 0),
            modifiers,
        })
    }

    /// The protocol's `Input.dispatchKeyEvent` parameters, pressed and let go.
    pub(super) fn events(&self) -> [Value; 2] {
        let mut up = json!({
            "type": "keyUp",
            "key": self.name,
            "code": self.code,
            "windowsVirtualKeyCode": self.codes.0,
            "nativeVirtualKeyCode": self.codes.1,
            "modifiers": self.modifiers,
        });
        let mut down = up.clone();
        down["type"] = json!(if self.text.is_some() {
            "keyDown"
        } else {
            "rawKeyDown"
        });
        if let Some(text) = &self.text {
            down["text"] = json!(text);
            down["unmodifiedText"] = json!(text);
        }
        if let Some(command) = self.command() {
            down["commands"] = json!([command]);
        }
        up["type"] = json!("keyUp");
        [down, up]
    }

    /// The editing command a shortcut stands for. Chromium on macOS carries out ⌘A, ⌘C and the
    /// like through the menu, which a dispatched key never reaches, so the command goes with it.
    fn command(&self) -> Option<&'static str> {
        if self.modifiers & META == 0 {
            return None;
        }
        let shift = self.modifiers & SHIFT != 0;
        Some(match (self.name.to_ascii_lowercase().as_str(), shift) {
            ("a", false) => "selectAll",
            ("c", false) => "copy",
            ("x", false) => "cut",
            ("v", false) => "paste",
            ("z", false) => "undo",
            ("z", true) => "redo",
            _ => return None,
        })
    }
}

/// A key that types nothing, or Enter and Space, by name: its `key`, its `code`, its name
/// for [`key_codes`] and its text.
fn named(name: &str) -> Option<(String, String, String, Option<String>)> {
    let lower = name.to_ascii_lowercase();
    let (key, code, known, text) = match lower.as_str() {
        "enter" | "return" => ("Enter", "Enter", "enter", Some("\r")),
        "tab" => ("Tab", "Tab", "tab", None),
        "escape" | "esc" => ("Escape", "Escape", "escape", None),
        "backspace" => ("Backspace", "Backspace", "backspace", None),
        "delete" => ("Delete", "Delete", "delete", None),
        "space" => (" ", "Space", "space", Some(" ")),
        "arrowup" | "up" => ("ArrowUp", "ArrowUp", "up", None),
        "arrowdown" | "down" => ("ArrowDown", "ArrowDown", "down", None),
        "arrowleft" | "left" => ("ArrowLeft", "ArrowLeft", "left", None),
        "arrowright" | "right" => ("ArrowRight", "ArrowRight", "right", None),
        "home" => ("Home", "Home", "home", None),
        "end" => ("End", "End", "end", None),
        "pageup" => ("PageUp", "PageUp", "pageup", None),
        "pagedown" => ("PageDown", "PageDown", "pagedown", None),
        _ => {
            let number = lower.strip_prefix('f')?.parse::<u32>().ok()?;
            let name = format!("F{number}");
            return (1..=12)
                .contains(&number)
                .then(|| (name.clone(), name, lower, None));
        }
    };
    Some((
        key.to_owned(),
        code.to_owned(),
        known.to_owned(),
        text.map(str::to_owned),
    ))
}

/// A single character's key.
fn typed(name: &str) -> Option<(String, String, String, Option<String>)> {
    let mut chars = name.chars();
    let character = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let upper = character.to_ascii_uppercase();
    let code = if upper.is_ascii_alphabetic() {
        format!("Key{upper}")
    } else if character.is_ascii_digit() {
        format!("Digit{character}")
    } else {
        String::new()
    };
    let text = character.to_string();
    Some((text.clone(), code, text.to_lowercase(), Some(text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_read_with_their_modifiers() {
        let enter = Key::parse("Enter").unwrap();
        assert_eq!(
            (enter.codes, enter.text.as_deref()),
            ((13, 0x24), Some("\r"))
        );
        let select_all = Key::parse("Meta+a").unwrap();
        assert_eq!((select_all.modifiers, select_all.text), (META, None));
        assert_eq!(
            (select_all.code.as_str(), select_all.codes),
            ("KeyA", (0x41, 0x00))
        );
        assert_eq!(Key::parse("shift+Tab").unwrap().modifiers, SHIFT);
        assert_eq!(Key::parse("F5").unwrap().codes.0, 116);
        assert_eq!(Key::parse(";").unwrap().codes, (0xBA, 0x29));
        assert!(Key::parse("Hyper+x").is_err());
        assert!(Key::parse("Banana").is_err());
        assert_eq!(Key::parse("Enter").unwrap().events()[0]["type"], "keyDown");
        let [down, up] = Key::parse("Escape").unwrap().events();
        assert_eq!(
            (down["type"].as_str(), up["type"].as_str()),
            (Some("rawKeyDown"), Some("keyUp"))
        );
        assert_eq!(
            Key::parse("Meta+a").unwrap().events()[0]["commands"],
            json!(["selectAll"])
        );
        assert_eq!(
            Key::parse("Meta+Shift+z").unwrap().events()[0]["commands"],
            json!(["redo"])
        );
        assert!(
            Key::parse("a").unwrap().events()[0]
                .get("commands")
                .is_none()
        );
    }
}
