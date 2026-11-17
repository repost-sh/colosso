//! What the log says of the keyboard: one line for each time it moves, naming what had it and
//! what has it by kind and pane, never by anything in them. Nothing typed is ever in it: no
//! character, no key, nothing a key could be told from.

use std::fmt;

use super::reconcile::{Moved, Responder};
use super::state::{Cause, Change, Holder, Intent, PaneId, Part};

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
            Holder::Menu => f.write_str("a menu"),
            Holder::Away => f.write_str("away"),
        }
    }
}

impl fmt::Display for Intent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Intent::Address => "⌘L",
            Intent::DevTools => "⌥⌘I",
            Intent::NewTab => "new tab",
            Intent::Reopen => "⌘⇧T",
            Intent::LinkBeside => "link opened beside",
            Intent::Switcher => "⌃⇥",
            Intent::Launcher => "launcher",
            Intent::PaneFocus => "pane_focus",
        })
    }
}

impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Cause::Intent(intent) => return intent.fmt(f),
            Cause::Click => "click",
            Cause::Focused => "focus moved",
            Cause::TabShown => "tab shown",
            Cause::WindowKey => "window key",
            Cause::WindowResigned => "window resigned",
            Cause::DialogOpened => "dialog opened",
            Cause::DialogClosed => "dialog closed",
            Cause::PageClosed => "page closed",
            Cause::MenuOpened => "menu opened",
            Cause::MenuClosed => "menu closed",
            Cause::AppActive => "app active",
            Cause::AppInactive => "app inactive",
            Cause::WindowClosed => "window closed",
            Cause::Healed => "healed: a key or a press came to the window",
        })
    }
}

impl fmt::Display for Responder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Responder::Gpui => f.write_str("gpui"),
            Responder::Page(pane) => write!(f, "page {pane}"),
            Responder::Window => f.write_str("the window"),
            Responder::Other => f.write_str("another view"),
        }
    }
}

/// What the first responder did as the keyboard moved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Followed {
    /// Nothing's known of it: no `AppKit` is there to move it, as in tests.
    Untold,
    /// It was what the holder wants already, or the holder wants none.
    Stayed,
    Moved(Moved),
}

impl fmt::Display for Moved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "first responder {} → {}", self.before, self.after)
    }
}

/// The line for `change`, and for what the first responder did if it's known.
pub fn line(change: &Change, followed: Followed) -> String {
    let (from, to, cause) = (change.from, change.to, change.cause);
    let line = format!("keyboard: {from} → {to} ({cause})");
    match followed {
        Followed::Untold => line,
        Followed::Stayed => format!("{line}; first responder as it was"),
        Followed::Moved(moved) => format!("{line}; {moved}"),
    }
}

/// How loud `change` is in the log: a menu, a dialog or anything taking the keyboard out of
/// the window is a warning, as what most often explains keys going nowhere; the rest is
/// what the person does all day.
pub fn level(change: &Change) -> log::Level {
    let aside = |holder: Holder| matches!(holder, Holder::Menu | Holder::Away);
    let dialog = matches!(change.cause, Cause::DialogOpened | Cause::DialogClosed);
    if aside(change.from) || aside(change.to) || dialog {
        log::Level::Warn
    } else {
        log::Level::Info
    }
}

/// Writes `change`'s line to the log, with what the first responder did if it's known.
pub fn tell(change: &Change, followed: Followed) {
    write(level(change), &line(change, followed));
}

#[cfg(test)]
thread_local! {
    /// Every line written in this test's thread, as a test reads them.
    pub static WRITTEN: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Writes `line` to the log at `level`.
pub fn write(level: log::Level, line: &str) {
    log::log!(target: TARGET, level, "{line}");
    #[cfg(test)]
    WRITTEN.with(|written| written.borrow_mut().push(line.to_owned()));
}

/// The line for the refused and healed moves of a window left out of the log in the last
/// second, `count` of them, past what a second may have.
pub fn left_out(count: u32) -> String {
    format!("keyboard: {count} more refused or healed in the last second, not logged one by one")
}

/// The line for the page in `pane` asking for the keyboard while `holder` has it: refused.
pub fn refused(pane: PaneId, holder: Holder, moved: Moved) -> String {
    let Moved { before, after } = moved;
    format!(
        "keyboard: page {pane} asked for the keyboard ({before} → {after}) while {holder} has it: \
         refused"
    )
}

/// The line for the first responder found off what `holder` wants, `why`, and put back.
pub fn healed(holder: Holder, why: &str, moved: Moved) -> String {
    format!("keyboard: {holder} has it, {why}; {moved}: healed")
}
