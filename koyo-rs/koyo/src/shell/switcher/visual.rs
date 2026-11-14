//! The tab switcher in real windows (`docs/tab-switcher.md`): the real shell off screen on this
//! repository's own checkout with three files' tabs, ⌃⇥ as typed, and what macOS says of the
//! panel's window as ⌃ is held and let go: unseen until ⌃ has been held, never with the
//! keyboard, with no edge of its own, the glass behind what GPUI draws. Its cards are saved as a
//! picture in `.superpowers/qa/switcher/`; the glass, which macOS draws, isn't in it.
//!
//! ```sh
//! Q=$PWD/.superpowers/qa/switcher && mkdir -p $Q/tmp &&
//!   KOYO_DATA_DIR=$Q/data TMPDIR=$Q/tmp KOYO_CONFIG_DIR=$Q/config KOYO_LOG_DIR=$Q/logs \
//!   script/cargo run -p koyo --features visual-check -- --visual-check-switcher
//! ```

use std::process::ExitCode;
use std::time::Duration;

use gpui_kit::{AnyWindowHandle, Modifiers, ModifiersChangedEvent};
use objc2::ClassType as _;
use objc2::runtime::NSObjectProtocol as _;
use objc2_app_kit::{NSGlassEffectView, NSWindowStyleMask};

use super::HOLD;
use crate::files::Place;
use crate::popup::{self, Kind};
use crate::visual::{self, Drawn, Result};

/// Where the picture goes, relative to the checkout.
const OUT: &str = ".superpowers/qa/switcher";
/// The files shown in turn, each in a tab of its own: the last is the tab shown.
const FILES: [&str; 3] = [
    "koyo-rs/koyo/src/shell.rs",
    "koyo-rs/koyo/src/popup.rs",
    "docs/tab-switcher.md",
];
/// Just short of how long ⌃ is held before the panel shows.
const NOT_YET: Duration = Duration::from_millis(10);

/// Runs the check and says what it found, or why it failed.
pub fn run() -> ExitCode {
    visual::run(check)
}

fn check() -> Result<Vec<String>> {
    let mut drawn = Drawn::open_on_checkout(OUT, &[])?;
    for file in FILES {
        let shell = drawn.shell.clone();
        (drawn.cx)
            .update_window(drawn.window, |_, window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.view_file(Place::file(file), window, cx);
                });
            })
            .map_err(|error| error.to_string())?;
        drawn.draw()?;
    }

    // ⌃⇥: the switcher is up on the three tabs, the one before picked, its panel unseen.
    drawn.cx.simulate_keystrokes(drawn.window, "ctrl-tab");
    drawn.cx.run_until_parked();
    let panel =
        (drawn.cx.read(|cx| popup::window(Kind::Switcher, cx))).ok_or("⌃⇥ opened no switcher")?;
    // The check's app is never the one in front, and a window that hides while it isn't would
    // stay unseen whatever the switcher did.
    native(&mut drawn, panel, |window| {
        window.setHidesOnDeactivate(false);
    })?;
    expect(&state(&drawn)?, &(titles(), 1, false), "as ⌃⇥ is pressed")?;
    let made = made(&mut drawn, panel)?;
    expect(&made, &(true, true), "its own edge gone, the glass in")?;
    let unseen = (false, false);
    expect(&seen(&mut drawn, panel)?, &unseen, "as ⌃⇥ is pressed")?;
    drawn.cx.advance_clock(HOLD.saturating_sub(NOT_YET));
    drawn.cx.run_until_parked();
    expect(&seen(&mut drawn, panel)?, &unseen, "just before the hold")?;

    // ⌃ held: it shows, and still hasn't the keyboard.
    drawn.cx.advance_clock(NOT_YET);
    drawn.cx.run_until_parked();
    expect(&seen(&mut drawn, panel)?, &(true, false), "after the hold")?;
    expect(&state(&drawn)?, &(titles(), 1, true), "after the hold")?;
    let picture = drawn.save_window(panel, "switcher.png")?;
    let mut lines = vec![format!("saved {}", picture.display())];

    // ⌃⇥ again, heard in the window behind, moves the pick on; ⌃ let go there shows the tab.
    drawn.cx.simulate_keystrokes(drawn.window, "ctrl-tab");
    expect(&state(&drawn)?, &(titles(), 2, true), "after ⌃⇥ again")?;
    let released = ModifiersChangedEvent {
        modifiers: Modifiers::none(),
        capslock: gpui_kit::Capslock::default(),
    };
    drawn.cx.simulate_event(drawn.window, released);
    drawn.draw()?;
    let up = drawn.cx.read(|cx| popup::window(Kind::Switcher, cx));
    expect(&up, &None, "the switcher once ⌃ is let go")?;
    let shown = drawn.cx.read(|cx| {
        let tab = drawn.shell.read(cx).workspace.tab_shown()?;
        Some(tab.first().title.to_string())
    });
    let picked = Some("shell.rs");
    expect(&shown.as_deref(), &picked, "the tab shown once ⌃ is let go")?;
    lines.push(
        "the panel was unseen until ⌃ had been held, never had the keyboard, had no edge of \
         its own and glass behind it; ⌃ let go showed the tab picked"
            .into(),
    );
    Ok(lines)
}

/// The tabs as the switcher lists them: the latest first.
fn titles() -> Vec<String> {
    (FILES.iter().rev())
        .filter_map(|file| file.rsplit('/').next())
        .map(String::from)
        .collect()
}

/// The switcher's cards' titles, its pick and whether its panel shows.
fn state(drawn: &Drawn) -> Result<(Vec<String>, usize, bool)> {
    (drawn.cx.read(super::state)).ok_or_else(|| "the switcher isn't up".into())
}

/// Whether the `panel`'s window shows, and whether it has the keyboard, as macOS says.
fn seen(drawn: &mut Drawn, panel: AnyWindowHandle) -> Result<(bool, bool)> {
    native(drawn, panel, |window| {
        (window.isVisible(), window.isKeyWindow())
    })
}

/// How the `panel`'s window is made: whether it has no edge of its own, and whether the glass is
/// behind what GPUI draws in it.
fn made(drawn: &mut Drawn, panel: AnyWindowHandle) -> Result<(bool, bool)> {
    native(drawn, panel, |window| {
        let glass = window.contentView().is_some_and(|content| {
            (content.subviews().iter()).any(|view| view.isKindOfClass(NSGlassEffectView::class()))
        });
        let borderless = !window.styleMask().contains(NSWindowStyleMask::Titled);
        (borderless, glass)
    })
}

/// What `read` makes of the window macOS draws `panel` in.
fn native<R>(
    drawn: &mut Drawn,
    panel: AnyWindowHandle,
    read: impl FnOnce(&objc2_app_kit::NSWindow) -> R,
) -> Result<R> {
    let read = (drawn.cx)
        .update_window(panel, |_, window, _| {
            crate::native::window::ns_window(window).map(|window| read(&window))
        })
        .map_err(|error| error.to_string())?;
    read.ok_or_else(|| "the panel has no window of macOS's".into())
}

/// Fails, saying `what` was `found`, unless it's as `expected`.
fn expect<T: PartialEq + std::fmt::Debug>(found: &T, expected: &T, what: &str) -> Result<()> {
    if found == expected {
        Ok(())
    } else {
        Err(format!("{what}: {found:?}, not {expected:?}"))
    }
}
