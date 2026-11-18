//! The cursor's blink, which only a view with the keyboard in the window in front draws: in a
//! window behind another app's its cursor holds still, drawn, until the window's back in front.
//! And the title the daemon says, which retitles the terminal however the view is drawn.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, TestAppContext, VisualTestContext, font, hsla, px,
};
use koyo_term_control::{DaemonCommand, EventKind, Handle, TermDir};
use koyo_term_engine::{Engine, Frame, Rgb, Size};

use super::{BLINK, Source, TerminalEvent, TerminalView};
use crate::client::TermClient;
use crate::glyphs::Look;
use crate::vt::Update;

/// The time between two frames of a 60 Hz display.
const FRAME: Duration = Duration::from_micros(16_667);

fn look() -> Look {
    Look {
        font: font("Menlo"),
        font_size: px(13.),
        line_height: 1.2,
        padding: px(4.),
        foreground: Rgb {
            r: 220,
            g: 220,
            b: 220,
        },
        background: Rgb {
            r: 20,
            g: 20,
            b: 20,
        },
        cursor: hsla(0., 0., 1., 1.),
        selection: hsla(0.6, 0.5, 0.5, 0.4),
    }
}

/// A screen with nothing on it but a blinking block cursor, as a program that asks for one
/// (`DECSCUSR` 1) has it.
fn screen() -> Frame {
    let size = Size {
        cols: 80,
        rows: 24,
        cell_width_px: 0,
        cell_height_px: 0,
    };
    let mut engine = Engine::new(size, 0).unwrap();
    engine.write(b"\x1b[1 q");
    let frame = engine.frame().unwrap();
    assert!(
        frame
            .cursor
            .is_some_and(|cursor| cursor.visible() && cursor.blinking())
    );
    frame
}

/// A view showing that screen, with the keyboard, in a window of its own. Its daemon never
/// starts, so it's never attached and the screen is all it shows.
fn view_with_the_keyboard(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<TerminalView>) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/test-term-view");
    let client = TermClient::new(TermDir::at(dir), DaemonCommand::binary("koyo"));
    client.shutdown().unwrap();
    let source = Source::Existing {
        handle: Handle::generate(),
        name: "zsh".into(),
    };
    let shown = cx.add_window(|window, cx| TerminalView::new(client, source, look(), window, cx));
    let view = shown.root(cx).unwrap();
    let window = shown.into();
    view.update(cx, |view, cx| {
        view.frame = Some(Arc::new(screen()));
        cx.notify();
    });
    cx.run_until_parked();
    (cx.update_window(window, |_, window, cx| {
        view.read(cx).focus.clone().focus(window, cx);
    }))
    .unwrap();
    cx.run_until_parked();
    (window, view)
}

fn in_front(window: AnyWindowHandle, front: bool, cx: &mut TestAppContext) {
    if front {
        (cx.update_window(window, |_, window, _| window.activate_window())).unwrap();
    } else {
        VisualTestContext::from_window(window, cx).deactivate_window();
    }
    cx.run_until_parked();
}

/// How many frames `window` draws while the clock passes by `time`, a display frame at a time.
fn frames_over(window: AnyWindowHandle, time: Duration, cx: &mut TestAppContext) -> u64 {
    (cx.update_window(window, |_, window, _| window.reset_layout_stats())).unwrap();
    let mut passed = Duration::ZERO;
    while passed < time {
        cx.executor().advance_clock(FRAME);
        cx.run_until_parked();
        passed += FRAME;
    }
    (cx.update_window(window, |_, window, _| window.layout_stats().frames)).unwrap()
}

fn cursor_drawn(view: &Entity<TerminalView>, cx: &TestAppContext) -> bool {
    view.read_with(cx, |view, _| view.cursor_on)
}

#[gpui_kit::test]
fn the_cursor_blinks_only_in_the_window_in_front_and_holds_still_drawn_behind(
    cx: &mut TestAppContext,
) {
    let (window, view) = view_with_the_keyboard(cx);
    let two_seconds = Duration::from_secs(2);
    let blinks = u64::try_from(two_seconds.as_millis() / BLINK.as_millis()).unwrap();

    in_front(window, true, cx);
    assert_eq!(
        frames_over(window, two_seconds, cx),
        blinks,
        "it blinks in front"
    );

    in_front(window, false, cx);
    assert!(cursor_drawn(&view, cx), "behind, its cursor is drawn");
    assert_eq!(frames_over(window, two_seconds, cx), 0, "and holds still");

    in_front(window, true, cx);
    assert!(
        cursor_drawn(&view, cx),
        "back in front, its cursor shows at once"
    );
    assert_eq!(
        frames_over(window, two_seconds, cx),
        blinks,
        "and blinks again"
    );
}

/// A view out of sight makes no frames to read a new title from, so the daemon's word of one
/// retitles the terminal; its word of the same title again, as a program that sets its title
/// over and over has it, says nothing.
#[gpui_kit::test]
fn the_daemons_word_of_a_new_title_retitles_the_terminal(cx: &mut TestAppContext) {
    let (_, view) = view_with_the_keyboard(cx);
    let titles = Rc::new(RefCell::new(Vec::new()));
    let heard = Rc::clone(&titles);
    let _listening = cx.update(|cx| {
        cx.subscribe(&view, move |_, event, _| {
            if let TerminalEvent::Title(title) = event {
                heard.borrow_mut().push(title.clone());
            }
        })
    });

    let say = |title: &str, cx: &mut TestAppContext| {
        let said = EventKind::Title {
            title: title.into(),
        };
        view.update(cx, |view, cx| view.on_update(Update::Event(Box::new(said)), cx));
    };
    say("vim notes.md", cx);
    assert_eq!(*titles.borrow(), ["vim notes.md"]);
    say("vim notes.md", cx);
    say("vim notes.md", cx);
    assert_eq!(*titles.borrow(), ["vim notes.md"], "the same title said again");
    say("vim todo.md", cx);
    assert_eq!(*titles.borrow(), ["vim notes.md", "vim todo.md"]);
}
