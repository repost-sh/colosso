//! The person's input, as CEF takes it: pointer, wheel, keys, composition and edits.
use super::{browser_of, host_of, with_pane};
use cef::sys::cef_event_flags_t as Flag;
use cef::{
    ImplBrowser, ImplBrowserHost, ImplFrame, KeyEvent, KeyEventType, MouseButtonType, MouseEvent,
};
use colosso_browser_ipc::{self as ipc, Ime, KeyKind, Modifiers, MouseButton, MouseKind, PaneId};

fn flags(modifiers: Modifiers, button: Option<MouseButton>) -> u32 {
    let mut flags = 0;
    for (on, flag) in [
        (modifiers.shift, Flag::EVENTFLAG_SHIFT_DOWN),
        (modifiers.control, Flag::EVENTFLAG_CONTROL_DOWN),
        (modifiers.alt, Flag::EVENTFLAG_ALT_DOWN),
        (modifiers.platform, Flag::EVENTFLAG_COMMAND_DOWN),
    ] {
        if on {
            flags |= flag.0;
        }
    }
    flags
        | match button {
            Some(MouseButton::Left) => Flag::EVENTFLAG_LEFT_MOUSE_BUTTON.0,
            Some(MouseButton::Middle) => Flag::EVENTFLAG_MIDDLE_MOUSE_BUTTON.0,
            Some(MouseButton::Right) => Flag::EVENTFLAG_RIGHT_MOUSE_BUTTON.0,
            None => 0,
        }
}

fn mouse_event(x: f32, y: f32, modifiers: Modifiers, button: Option<MouseButton>) -> MouseEvent {
    MouseEvent {
        x: x.round() as i32,
        y: y.round() as i32,
        modifiers: flags(modifiers, button),
    }
}

pub(super) fn mouse(
    pane: PaneId,
    kind: MouseKind,
    x: f32,
    y: f32,
    button: Option<MouseButton>,
    click_count: u32,
    modifiers: Modifiers,
) {
    let Some(host) = host_of(pane) else {
        return;
    };
    let event = mouse_event(x, y, modifiers, button);
    match (kind, button) {
        (MouseKind::Move, _) => host.send_mouse_move_event(Some(&event), 0),
        (MouseKind::Leave, _) => host.send_mouse_move_event(Some(&event), 1),
        (MouseKind::Down | MouseKind::Up, Some(button)) => {
            let kind_up = i32::from(kind == MouseKind::Up);
            let button = match button {
                MouseButton::Left => MouseButtonType::LEFT,
                MouseButton::Middle => MouseButtonType::MIDDLE,
                MouseButton::Right => MouseButtonType::RIGHT,
            };
            let count = i32::try_from(click_count.clamp(1, 3)).unwrap_or(1);
            host.send_mouse_click_event(Some(&event), button, kind_up, count);
        }
        (MouseKind::Down | MouseKind::Up, None) => {}
    }
}

pub(super) fn key(
    pane: PaneId,
    kind: KeyKind,
    key_code: i32,
    native_key_code: i32,
    text: Option<&str>,
    modifiers: Modifiers,
) {
    let Some(host) = host_of(pane) else {
        return;
    };
    let base = KeyEvent {
        modifiers: flags(modifiers, None),
        windows_key_code: key_code,
        native_key_code,
        ..KeyEvent::default()
    };
    // CEF makes a Mac key event of a press or release, and one with no character is a
    // modifier's: Chromium takes it for a press, a release included. So they carry the key's.
    let key = text
        .and_then(|text| text.encode_utf16().next())
        .unwrap_or_default();
    let pressed = KeyEvent {
        character: key,
        unmodified_character: key,
        ..base
    };
    match kind {
        KeyKind::Down => host.send_key_event(Some(&KeyEvent {
            type_: KeyEventType::RAWKEYDOWN,
            ..pressed
        })),
        KeyKind::Up => host.send_key_event(Some(&KeyEvent {
            type_: KeyEventType::KEYUP,
            ..pressed
        })),
        KeyKind::Char => {
            for unit in text.unwrap_or_default().encode_utf16() {
                host.send_key_event(Some(&KeyEvent {
                    type_: KeyEventType::CHAR,
                    character: unit,
                    unmodified_character: unit,
                    ..pressed.clone()
                }));
            }
        }
    }
}

pub(super) fn ime(pane: PaneId, action: Ime) {
    let Some(host) = host_of(pane) else {
        return;
    };
    match action {
        Ime::Compose { text, selection } => {
            let range = cef::Range {
                from: selection.0,
                to: selection.1,
            };
            host.ime_set_composition(Some(&text.as_str().into()), None, None, Some(&range));
        }
        Ime::Commit { text } => host.ime_commit_text(Some(&text.as_str().into()), None, 0),
        Ime::Cancel => host.ime_cancel_composition(),
    }
}

pub(super) fn edit(pane: PaneId, command: ipc::Edit) {
    let Some(frame) = browser_of(pane).and_then(|b| b.focused_frame()) else {
        return;
    };
    match command {
        ipc::Edit::Undo => frame.undo(),
        ipc::Edit::Redo => frame.redo(),
        ipc::Edit::Cut => frame.cut(),
        ipc::Edit::Copy => frame.copy(),
        ipc::Edit::Paste => frame.paste(),
        ipc::Edit::SelectAll => frame.select_all(),
    }
}

/// Whole pixels to scroll for a wheel delta, and what is left below one
/// pixel for the next event, so slow trackpad motion adds up instead of
/// rounding away.
fn wheel_steps(rest: (f32, f32), delta: (f32, f32)) -> ((f32, f32), (f32, f32)) {
    let (x, y) = (rest.0 + delta.0, rest.1 + delta.1);
    let (sx, sy) = (x.trunc(), y.trunc());
    ((sx, sy), (x - sx, y - sy))
}
/// Scrolls by whole pixels, keeping what is left below one for the next
/// event.
pub(super) fn wheel(pane: PaneId, x: f32, y: f32, dx: f32, dy: f32, modifiers: Modifiers) {
    let steps = with_pane(pane, |p| {
        let (steps, rest) = wheel_steps(p.wheel_rest, (dx, dy));
        p.wheel_rest = rest;
        steps
    });
    if let (Some(host), Some((sx, sy))) = (host_of(pane), steps)
        && (sx != 0. || sy != 0.)
    {
        host.send_mouse_wheel_event(
            Some(&mouse_event(x, y, modifiers, None)),
            sx as i32,
            sy as i32,
        );
    }
}

#[cfg(test)]
mod tests {

    #[test]
    #[expect(clippy::float_cmp, reason = "whole-pixel steps are exact in f32")]
    fn slow_wheel_motion_adds_up_to_whole_pixels() {
        let mut rest = (0., 0.);
        let mut moved = 0.;
        for _ in 0..10 {
            let (steps, left) = wheel_steps(rest, (0., -0.4));
            moved += steps.1;
            rest = left;
        }
        assert_eq!(moved, -4.);
        assert!(rest.1.abs() < 1e-4, "{}", rest.1);
        let ((sx, sy), _) = wheel_steps((0.9, 0.), (0.2, 2.5));
        assert_eq!((sx, sy), (1., 2.));
    }

    use super::*;

    #[test]
    fn modifiers_and_buttons_become_cef_flags() {
        let all = Modifiers {
            shift: true,
            control: true,
            alt: true,
            platform: true,
        };
        assert_eq!(flags(all, Some(MouseButton::Left)), 0b1001_1110);
        assert_eq!(
            flags(Modifiers::default(), Some(MouseButton::Right)),
            1 << 6
        );
        assert_eq!(flags(Modifiers::default(), None), 0);
    }
}
