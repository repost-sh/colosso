//! How the terminal check drives the shell: what it reads off the terminals shown, and the
//! keys, mouse, wheel and actions it sends them, as a person at the window would.

use std::time::Duration;

use gpui_kit::{
    Action, Entity, Focusable as _, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, ScrollDelta, ScrollEvent, point,
};
use koyo_term::TerminalView;

use crate::visual::{Drawn, Result};

/// How long the shell gets to answer.
pub const PATIENCE: Duration = Duration::from_secs(20);

/// The terminal tab shown.
pub fn terminal(drawn: &mut Drawn) -> Result<Entity<TerminalView>> {
    let shell = drawn.shell.clone();
    (drawn.cx.read(|cx| shell.read(cx).shown_terminal()))
        .ok_or_else(|| "no terminal is shown".into())
}

/// The text of the terminal tab shown, a line per row from the top.
pub fn text(drawn: &mut Drawn) -> Result<String> {
    let view = terminal(drawn)?;
    Ok(drawn.cx.read(|cx| view.read(cx).text()))
}

pub fn alt_screen(drawn: &mut Drawn) -> Result<bool> {
    let view = terminal(drawn)?;
    Ok(drawn.cx.read(|cx| view.read(cx).alt_screen()))
}

/// Draws until the shown terminal's text is `done`, or says it never got there and what it
/// read.
pub fn wait(drawn: &mut Drawn, what: &str, done: impl Fn(&str) -> bool) -> Result<()> {
    wait_until(drawn, what, |drawn| Ok(done(&text(drawn)?)))
}

/// Draws until `done`, or says it never got there and what the shown terminal read.
pub fn wait_until(
    drawn: &mut Drawn,
    what: &str,
    done: impl FnMut(&mut Drawn) -> Result<bool>,
) -> Result<()> {
    (drawn.until(PATIENCE, done)).map_err(|error| {
        let screen = text(drawn).unwrap_or_default();
        format!("waiting for {what}: {error}; the screen read {screen:?}")
    })
}

/// Gives the terminal tab the keyboard and draws, so the input method's handler is its own
/// before a key goes in: typed text reaches a terminal through it. Whether it had the
/// keyboard already.
pub fn focused(drawn: &mut Drawn) -> Result<bool> {
    let view = terminal(drawn)?;
    let window = drawn.window;
    let (had, has) = (drawn.cx)
        .update_window(window, |_, window, cx| {
            let focus = view.focus_handle(cx);
            let had = focus.is_focused(window);
            window.focus(&focus, cx);
            (had, focus.is_focused(window))
        })
        .map_err(|error| error.to_string())?;
    if !has {
        return Err("the terminal didn't take the keyboard".into());
    }
    drawn.draw()?;
    Ok(had)
}

/// Types `line` a key at a time, as a person would, and presses Enter.
pub fn type_line(drawn: &mut Drawn, line: &str) -> Result<()> {
    type_keys(drawn, line)?;
    press(drawn, "enter")
}

/// Types `line` a key at a time, as a person would.
pub fn type_keys(drawn: &mut Drawn, line: &str) -> Result<()> {
    let window = drawn.window;
    for c in line.chars() {
        let keystroke = Keystroke {
            modifiers: Modifiers {
                shift: c.is_ascii_uppercase(),
                ..Modifiers::default()
            },
            key: c.to_lowercase().to_string(),
            key_char: Some(c.to_string()),
        };
        (drawn.cx)
            .update_window(window, |_, window, cx| {
                window.dispatch_keystroke(keystroke, cx)
            })
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Presses `key`, `ctrl-c` say, and draws.
pub fn press(drawn: &mut Drawn, key: &'static str) -> Result<()> {
    let keystroke = Keystroke::parse(key).map_err(|error| error.to_string())?;
    let window = drawn.window;
    (drawn.cx)
        .update_window(window, |_, window, cx| {
            window.dispatch_keystroke(keystroke, cx)
        })
        .map_err(|error| error.to_string())?;
    drawn.draw()
}

/// Dispatches `action` to what has the keyboard, and draws.
pub fn act(drawn: &mut Drawn, action: Box<dyn Action>) -> Result<()> {
    let window = drawn.window;
    (drawn.cx)
        .update_window(window, |_, window, cx| window.dispatch_action(action, cx))
        .map_err(|error| error.to_string())?;
    drawn.draw()
}

/// The middle of the terminal tab's cell at `col`, `row`, in the window.
pub fn cell(drawn: &mut Drawn, col: u16, row: u16) -> Result<Point<Pixels>> {
    let view = terminal(drawn)?;
    let bounds = drawn.cx.read(|cx| view.read(cx).cell_bounds(col, row));
    bounds
        .map(|bounds| bounds.center())
        .ok_or_else(|| "the terminal isn't laid out".into())
}

/// Clicks the left button `count` times over at `at`, and draws.
pub fn click(drawn: &mut Drawn, at: Point<Pixels>, count: usize) -> Result<()> {
    send(drawn, mouse_down(at, count))?;
    send(drawn, mouse_up(at, count))?;
    drawn.draw()
}

/// Clicks the left button once at `at` with ⌘ held, as a link is opened, and draws.
pub fn command_click(drawn: &mut Drawn, at: Point<Pixels>) -> Result<()> {
    let held = |input| match input {
        PlatformInput::MouseDown(down) => PlatformInput::MouseDown(MouseDownEvent {
            modifiers: Modifiers::command(),
            ..down
        }),
        PlatformInput::MouseUp(up) => PlatformInput::MouseUp(MouseUpEvent {
            modifiers: Modifiers::command(),
            ..up
        }),
        other => other,
    };
    send(drawn, held(mouse_down(at, 1)))?;
    send(drawn, held(mouse_up(at, 1)))?;
    drawn.draw()
}

/// Drags with the left button from `from` to `to`, and draws.
pub fn drag(drawn: &mut Drawn, from: Point<Pixels>, to: Point<Pixels>) -> Result<()> {
    send(drawn, mouse_down(from, 1))?;
    send(
        drawn,
        PlatformInput::MouseMove(MouseMoveEvent {
            position: to,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }),
    )?;
    send(drawn, mouse_up(to, 1))?;
    drawn.draw()
}

/// Turns the wheel `lines` at `at`, positive towards the top, and draws.
pub fn wheel(drawn: &mut Drawn, at: Point<Pixels>, lines: f32) -> Result<()> {
    send(
        drawn,
        PlatformInput::Scroll(ScrollEvent {
            position: at,
            delta: ScrollDelta::Lines(point(0., lines)),
            ..ScrollEvent::default()
        }),
    )?;
    drawn.draw()
}

/// The text the terminal tab has selected, as Copy would take it.
pub fn selected(drawn: &mut Drawn) -> Result<Option<String>> {
    let view = terminal(drawn)?;
    let reply = drawn.cx.read(|cx| view.read(cx).selected_text());
    (reply.recv_timeout(Duration::from_secs(2))).map_err(|error| error.to_string())
}

fn mouse_down(position: Point<Pixels>, click_count: usize) -> PlatformInput {
    PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::default(),
        click_count,
        first_mouse: false,
    })
}

fn mouse_up(position: Point<Pixels>, click_count: usize) -> PlatformInput {
    PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::default(),
        click_count,
    })
}

pub fn send(drawn: &mut Drawn, input: PlatformInput) -> Result<()> {
    let window = drawn.window;
    (drawn.cx)
        .update_window(window, |_, window, cx| {
            window.dispatch_event(input, cx);
        })
        .map_err(|error| error.to_string())
}
