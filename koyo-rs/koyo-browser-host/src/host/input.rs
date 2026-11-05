//! The person's input, as CEF takes it: pointer, wheel, pinch, keys, composition and edits.
//!
//! All of it reaches CEF through [`handle`], which plays each input as the event the host's
//! app is handling: CEF stamps an input with that event's time, not one of its own.
use super::{browser_of, host_of, with_pane};
use cef::sys::cef_event_flags_t as Flag;
use cef::{
    ImplBrowser, ImplBrowserHost, ImplFrame, KeyEvent, KeyEventType, MouseButtonType, MouseEvent,
    PointerType, TouchEvent, TouchEventType,
};
use koyo_browser_ipc::{
    self as ipc, AppRequest, Ime, KeyKind, Modifiers, MouseButton, MouseKind, PaneId, PinchPhase,
};

/// Half the span between the two fingers a pinch is played with as it starts, in logical
/// pixels. The engine stops pinching once fingers come closer than 125 apart, so they start
/// far enough apart to zoom a page all the way out in one pinch.
const PINCH_REACH: f32 = 400.;
/// The closest the two fingers playing a pinch come: just short of where the engine would
/// stop pinching.
const PINCH_CLOSEST: f32 = 64.;

/// What the trackpad's last events leave for its next.
#[derive(Debug, Default)]
pub(super) struct Trackpad {
    /// Wheel motion below a whole pixel: trackpads send many small fractional steps, and
    /// rounding each loses them.
    wheel_rest: (f32, f32),
    /// Half the span between the fingers playing a pinch, while one is on.
    pinch: Option<f32>,
}

/// Plays the person's input to their pane's page; anything else is left alone.
pub(super) fn handle(request: AppRequest) {
    played(|| send(request));
}

/// Runs `input`, which hands CEF an input, as the app's handling of an event that happened
/// now (`mac_app`).
fn played(input: impl FnOnce()) {
    #[cfg(target_os = "macos")]
    crate::mac_app::playing(input);
    #[cfg(not(target_os = "macos"))]
    input();
}

fn send(request: AppRequest) {
    match request {
        AppRequest::Mouse {
            pane,
            kind,
            x,
            y,
            button,
            click_count,
            modifiers,
        } => mouse(pane, kind, x, y, button, click_count, modifiers),
        AppRequest::Wheel {
            pane,
            x,
            y,
            dx,
            dy,
            modifiers,
        } => wheel(pane, x, y, dx, dy, modifiers),
        AppRequest::Pinch {
            pane,
            x,
            y,
            delta,
            phase,
        } => pinch(pane, x, y, delta, phase),
        AppRequest::Key {
            pane,
            kind,
            key_code,
            native_key_code,
            text,
            modifiers,
        } => key(
            pane,
            kind,
            key_code,
            native_key_code,
            text.as_deref(),
            modifiers,
        ),
        AppRequest::Ime { pane, action } => ime(pane, action),
        AppRequest::Edit { pane, command } => edit(pane, command),
        _ => {}
    }
}

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

fn mouse(
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

fn key(
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

fn ime(pane: PaneId, action: Ime) {
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

fn edit(pane: PaneId, command: ipc::Edit) {
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
fn wheel(pane: PaneId, x: f32, y: f32, dx: f32, dy: f32, modifiers: Modifiers) {
    let steps = with_pane(pane, |p| {
        let (steps, rest) = wheel_steps(p.trackpad.wheel_rest, (dx, dy));
        p.trackpad.wheel_rest = rest;
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

/// A touch of the two fingers playing a pinch, with half their span.
type Touch = (TouchEventType, f32);

/// The touches a pinch event makes of the fingers playing it, given half their span while
/// they're down, and half their span after. Starting puts them down; going on spreads or
/// closes them by the event's delta; ending lifts them. A pinch still on as another starts
/// ends first, and one going on that was never seen to start starts.
fn pinch_touches(held: Option<f32>, delta: f32, phase: PinchPhase) -> (Vec<Touch>, Option<f32>) {
    let up = held.map(|reach| (TouchEventType::RELEASED, reach));
    match phase {
        PinchPhase::Started => {
            let touches = up
                .into_iter()
                .chain([(TouchEventType::PRESSED, PINCH_REACH)]);
            (touches.collect(), Some(PINCH_REACH))
        }
        PinchPhase::Moved => {
            let from = held.unwrap_or(PINCH_REACH);
            let to = (from * (1. + delta)).max(PINCH_CLOSEST);
            let down = held.is_none().then_some((TouchEventType::PRESSED, from));
            let touches = down.into_iter().chain([(TouchEventType::MOVED, to)]);
            (touches.collect(), Some(to))
        }
        PinchPhase::Ended => (up.into_iter().collect(), None),
    }
}

/// A pinch on the trackpad, centred at `(x, y)`. The engine pinch-zooms a page it draws off
/// screen only for touches, so it's played as two fingers either side of the centre,
/// spreading or closing as the pinch does.
fn pinch(pane: PaneId, x: f32, y: f32, delta: f32, phase: PinchPhase) {
    let touches = with_pane(pane, |p| {
        let (touches, held) = pinch_touches(p.trackpad.pinch, delta, phase);
        p.trackpad.pinch = held;
        touches
    });
    let (Some(host), Some(touches)) = (host_of(pane), touches) else {
        return;
    };
    for (type_, reach) in touches {
        for (id, side) in [(0, -1.), (1, 1.)] {
            host.send_touch_event(Some(&TouchEvent {
                id,
                x: x + side * reach,
                y,
                type_,
                pointer_type: PointerType::TOUCH,
                ..TouchEvent::default()
            }));
        }
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

    #[test]
    fn a_pinch_puts_two_fingers_down_spreads_them_by_its_delta_and_lifts_them() {
        let (touches, held) = pinch_touches(None, 0., PinchPhase::Started);
        assert_eq!(touches, [(TouchEventType::PRESSED, PINCH_REACH)]);
        let (touches, held) = pinch_touches(held, 0.5, PinchPhase::Moved);
        assert_eq!(touches, [(TouchEventType::MOVED, 600.)]);
        let (touches, held) = pinch_touches(held, -0.5, PinchPhase::Moved);
        assert_eq!(touches, [(TouchEventType::MOVED, 300.)]);
        let (touches, held) = pinch_touches(held, -1., PinchPhase::Moved);
        assert_eq!(
            touches,
            [(TouchEventType::MOVED, PINCH_CLOSEST)],
            "never close enough to stop"
        );
        let (touches, held) = pinch_touches(held, 0., PinchPhase::Ended);
        assert_eq!(
            (touches, held),
            (vec![(TouchEventType::RELEASED, PINCH_CLOSEST)], None)
        );
    }

    #[test]
    fn a_pinch_missing_its_start_or_end_still_plays_whole() {
        let (touches, held) = pinch_touches(None, 0.25, PinchPhase::Moved);
        assert_eq!(
            touches,
            [
                (TouchEventType::PRESSED, PINCH_REACH),
                (TouchEventType::MOVED, 500.)
            ]
        );
        let (touches, _) = pinch_touches(held, 0., PinchPhase::Started);
        assert_eq!(
            touches,
            [
                (TouchEventType::RELEASED, 500.),
                (TouchEventType::PRESSED, PINCH_REACH)
            ]
        );
        assert_eq!(
            pinch_touches(None, 0., PinchPhase::Ended),
            (Vec::new(), None)
        );
    }
}
