//! Where a key goes when GPUI's view is offered it: one decision for each offer, by who
//! offers it and who has the keyboard, with nothing remembered (`docs/native-layer.md` 4.1).
//!
//! GPUI's view is offered a key it may not have the keyboard for in two ways. `AppKit` offers
//! every ⌘ and ⌃ key, and every function key (the arrows, F1 to F12, Page Up, Home), to the
//! views of the key window as a key equivalent before it delivers the key to the first
//! responder. And a page hands back a key it had no use for. A key GPUI's view takes then runs
//! Koyo's bindings and never the text system: the text system types into whatever has the
//! keyboard, the page, so a left arrow offered by `AppKit` put a control character in a page's
//! field and never moved its caret.

/// Who offers the key to GPUI's view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    /// `AppKit`'s own pass of key equivalents, before the key is delivered.
    Equivalent,
    /// The view that had the keyboard, handing back a key it had no use for.
    HandBack,
}

/// What has the keyboard in the window as the key is offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum First {
    /// GPUI's view, or no view at all: the window itself.
    Gpui,
    /// Another view: a page's.
    Native,
}

/// The key, as far as its route needs it: nothing a key could be told from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub command: bool,
    pub control: bool,
}

/// What GPUI's view does with a key offered to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Everything GPUI does with a key: its bindings, then the text system.
    Whole,
    /// Koyo's bindings and nothing else. Unused, the key goes on as `AppKit` would send it.
    Bindings,
    /// Not GPUI's: `AppKit` delivers it to the view that has the keyboard.
    Deliver,
    /// Nobody's: the keyboard has moved since the key was pressed.
    Drop,
}

/// What GPUI's view does with `key`, offered in `pass` while `first` has the keyboard.
///
/// Koyo keeps every ⌘ and ⌃ key it binds, whoever has the keyboard (the owner's choice,
/// 2026-11-16): those are offered to its bindings first, and go on to the page when Koyo binds
/// nothing to them. Every other key is the page's first, and Koyo's bindings' once the page
/// hands it back.
#[must_use]
pub fn route(key: Key, pass: Pass, first: First) -> Route {
    match (first, pass) {
        (First::Gpui, Pass::Equivalent) => Route::Whole,
        (First::Gpui, Pass::HandBack) => Route::Drop,
        (First::Native, Pass::Equivalent) if key.command || key.control => Route::Bindings,
        (First::Native, Pass::Equivalent) => Route::Deliver,
        (First::Native, Pass::HandBack) => Route::Bindings,
    }
}

#[cfg(test)]
mod tests {
    use super::{First, Key, Pass, Route, route};

    const PLAIN: Key = Key { command: false, control: false };
    const COMMAND: Key = Key { command: true, control: false };
    const CONTROL: Key = Key { command: false, control: true };

    #[test]
    fn with_gpui_holding_a_key_equivalent_is_gpuis_whole() {
        for key in [PLAIN, COMMAND, CONTROL] {
            assert_eq!(route(key, Pass::Equivalent, First::Gpui), Route::Whole);
        }
    }

    #[test]
    fn an_arrow_appkit_offers_while_a_page_holds_is_the_pages() {
        assert_eq!(route(PLAIN, Pass::Equivalent, First::Native), Route::Deliver);
    }

    #[test]
    fn a_command_or_control_key_while_a_page_holds_is_koyos_bindings_first() {
        assert_eq!(route(COMMAND, Pass::Equivalent, First::Native), Route::Bindings);
        assert_eq!(route(CONTROL, Pass::Equivalent, First::Native), Route::Bindings);
    }

    #[test]
    fn a_key_a_page_hands_back_runs_koyos_bindings_and_never_the_text_system() {
        for key in [PLAIN, COMMAND, CONTROL] {
            assert_eq!(route(key, Pass::HandBack, First::Native), Route::Bindings);
        }
    }

    #[test]
    fn a_key_handed_back_after_the_keyboard_moved_is_nobodys() {
        for key in [PLAIN, COMMAND, CONTROL] {
            assert_eq!(route(key, Pass::HandBack, First::Gpui), Route::Drop);
        }
    }

    #[test]
    fn no_key_reaches_the_text_system_while_a_page_holds() {
        for key in [PLAIN, COMMAND, CONTROL] {
            for pass in [Pass::Equivalent, Pass::HandBack] {
                assert_ne!(route(key, pass, First::Native), Route::Whole);
            }
        }
    }
}
