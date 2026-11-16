//! What the log says of the keyboard: one line for each time it moves, naming what had it and
//! what has it by kind and pane, never by anything in them. Nothing typed is ever in it: no
//! character, no key, nothing a key could be told from.

use std::fmt;

use super::state::{Cause, Change, Holder, Part};

/// Where the keyboard's lines go in Koyo's log.
pub const TARGET: &str = "koyo::keyboard";

impl fmt::Display for Part {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Part::Window => f.write_str("the window's keys"),
            Part::Pane(pane) => write!(f, "pane {pane}"),
            Part::Bar(pane) => write!(f, "a bar of page {pane}"),
            Part::Other => f.write_str("another part"),
        }
    }
}

impl fmt::Display for Holder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Holder::Gpui(part) => write!(f, "gpui ({part})"),
            Holder::Page(pane) => write!(f, "page {pane}"),
            Holder::Away => f.write_str("away"),
        }
    }
}

impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Cause::Focused => "focus moved",
            Cause::TabShown => "tab shown",
            Cause::WindowKey => "window key",
            Cause::WindowResigned => "window resigned",
            Cause::DialogOpened => "dialog opened",
            Cause::DialogClosed => "dialog closed",
            Cause::PageClosed => "page closed",
        })
    }
}

/// The line for `change`.
pub fn line(change: &Change) -> String {
    format!("keyboard: {} → {} ({})", change.from, change.to, change.cause)
}

/// How loud `change` is in the log: a dialog or anything taking the keyboard out of the
/// window is a warning, as what most often explains keys going nowhere; the rest is what the
/// person does all day.
pub fn level(change: &Change) -> log::Level {
    let aside = |holder: Holder| holder == Holder::Away;
    let dialog = matches!(change.cause, Cause::DialogOpened | Cause::DialogClosed);
    if aside(change.from) || aside(change.to) || dialog {
        log::Level::Warn
    } else {
        log::Level::Info
    }
}

/// Writes `change`'s line to the log.
pub fn tell(change: &Change) {
    log::log!(target: TARGET, level(change), "{}", line(change));
}
