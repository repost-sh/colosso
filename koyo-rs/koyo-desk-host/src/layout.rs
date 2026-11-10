//! The keyboard layout in use: which key types a character. A shortcut is named by its
//! character (`cmd+z`), and an app reads a key event's code through the person's layout, so
//! the US code for "z" would be another letter's key on AZERTY or QWERTZ, another shortcut.

use std::ffi::c_void;

use crate::ns;
use crate::sys::{self, Ref};

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentKeyboardLayoutInputSource() -> Ref;
    fn TISGetInputSourceProperty(source: Ref, key: Ref) -> Ref;
    static kTISPropertyUnicodeKeyLayoutData: Ref;
    fn LMGetKbdType() -> u8;
    fn UCKeyTranslate(
        layout: *const c_void,
        code: u16,
        action: u16,
        modifiers: u32,
        keyboard: u32,
        options: u32,
        dead_keys: *mut u32,
        most: usize,
        length: *mut usize,
        text: *mut u16,
    ) -> i32;
}

/// `kUCKeyActionDisplay`, and `kUCKeyTranslateNoDeadKeysMask`.
const DISPLAY: u16 = 3;
const NO_DEAD_KEYS: u32 = 1;
/// The keys that type characters are among the first of the codes.
const CODES: u16 = 51;

/// The code of the key that types `character` with no modifier held, on the layout in use;
/// `None` when no key does (or the layout can't be read): the caller falls back on the US code.
pub fn code_for(character: char) -> Option<u16> {
    let mut wanted = [0u16; 2];
    let wanted = character
        .to_ascii_lowercase()
        .encode_utf16(&mut wanted)
        .to_vec();
    // SAFETY: the current layout, ours to release; its layout data is lent by it and lives
    // as long as it; each translation writes at most four units into a buffer of four.
    unsafe {
        let source = ns::owned(TISCopyCurrentKeyboardLayoutInputSource())?;
        let data = TISGetInputSourceProperty(ns::raw(&source), kTISPropertyUnicodeKeyLayoutData);
        if data.is_null() {
            return None;
        }
        let layout = sys::CFDataGetBytePtr(data).cast::<c_void>();
        let keyboard = u32::from(LMGetKbdType());
        (0..CODES).find(|code| {
            let (mut dead, mut length, mut typed) = (0u32, 0usize, [0u16; 4]);
            let status = UCKeyTranslate(
                layout,
                *code,
                DISPLAY,
                0,
                keyboard,
                NO_DEAD_KEYS,
                &raw mut dead,
                typed.len(),
                &raw mut length,
                typed.as_mut_ptr(),
            );
            status == 0 && typed.get(..length) == Some(wanted.as_slice())
        })
    }
}

/// The key that types, on the layout in use, what the key `code` types on a US keyboard.
/// The person's keys come from the app by their US codes, which stand for the characters
/// they mean: sent on as they are, ⌘Z pressed in the tab on AZERTY would reach the app as
/// the key that types W there. `code` itself for a key that types no character (an arrow,
/// a modifier), and where the layout has no key for the character.
pub fn placed(code: u16) -> u16 {
    koyo_desk_wire::keys::character(code)
        .and_then(code_for)
        .unwrap_or(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_digits_have_a_key_and_distinct_ones() {
        // Whatever the layout of the Mac the test runs on, it types these somewhere.
        let codes: Vec<Option<u16>> = "az19".chars().map(code_for).collect();
        assert!(codes.iter().all(Option::is_some), "{codes:?}");
        let mut distinct = codes.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), codes.len(), "{codes:?}");
        assert_eq!(code_for('A'), code_for('a'));
        assert_eq!(code_for('\u{1F600}'), None);

        // In this test too, as the layout is read by one thread at a time: the person's
        // key goes to where the layout types its character, which is where an agent's
        // shortcut for that character goes; a key that types none stays where it is.
        let z = koyo_desk_wire::keys::code("z").unwrap();
        assert_eq!(Some(placed(z)), code_for('z'));
        for named in ["left", "return", "f5", "escape", "tab"] {
            let code = koyo_desk_wire::keys::code(named).unwrap();
            assert_eq!(placed(code), code, "{named}");
        }
        for (modifier, _) in koyo_desk_wire::keys::MODIFIERS {
            assert_eq!(placed(modifier), modifier);
        }
    }
}
