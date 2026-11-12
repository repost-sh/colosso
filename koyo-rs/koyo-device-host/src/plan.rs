//! An agent's act as timed touches, keys and buttons (`docs/devices.md` 6), in points of the
//! upright picture: what the host's loop sends, a step at a time, so the person can interrupt
//! it anywhere. The timings are the proof of concept's, which were measured.

use koyo_device_gestures::{At, key_usage, pressed};
use koyo_device_wire::{Act, Button, Edge, Input, Point, Press, Refusal, RefusalCode, Size};

/// HID pages and usages for buttons and modifiers.
pub const CONSUMER: u32 = 0x0C;
const SHIFT: u16 = 0xE1;
const CONTROL: u16 = 0xE0;
const OPTION: u16 = 0xE2;
const COMMAND: u16 = 0xE3;
/// A tap's touch, and the gap between a double tap's two.
const TAP_HELD_MS: u32 = 60;
const TAP_GAP_MS: u32 = 90;
/// Sixty steps a second for anything that moves.
const STEP_MS: u32 = 16;
/// From a contact's last move to its lift: longer than a frame of the simulator's 60, or the
/// last move is folded into the lift and lost (measured: a drag came 17 steps of 18 short).
const LIFT_AFTER_MS: u32 = 25;
/// A key's press, and the time to the next.
const KEY_HELD_MS: u32 = 15;
const KEY_EVERY_MS: u32 = 30;

#[derive(Clone, Debug, PartialEq)]
pub enum Touch {
    /// Fingers at these places, touching, or lifting; a contact keeps the edge it began at.
    Fingers {
        at: Vec<Point>,
        touching: bool,
        edge: Option<Edge>,
    },
    Key {
        usage: u16,
        down: bool,
    },
    Button {
        page: u32,
        usage: u32,
        down: bool,
    },
}

/// A step and when it goes, in milliseconds from the act's start.
#[derive(Clone, Debug, PartialEq)]
pub struct Timed {
    pub at_ms: u32,
    pub touch: Touch,
}

fn fingers(at_ms: u32, at: Vec<Point>, touching: bool, edge: Option<Edge>) -> Timed {
    Timed {
        at_ms,
        touch: Touch::Fingers { at, touching, edge },
    }
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::Invalid, message)
}

/// Where the edge gestures begin: as the person's own presses do (`koyo-device-gestures`).
fn edge_of(at: Point, screen: Size) -> Option<Edge> {
    koyo_device_gestures::edge(At::new(
        (at.x / screen.width) as f32,
        (at.y / screen.height) as f32,
    ))
}

fn share(screen: Size, x: f64, y: f64) -> Point {
    Point {
        x: x * screen.width,
        y: y * screen.height,
    }
}

/// `count` fingers side by side about `at`, 30 points apart, kept on the screen.
fn side_by_side(at: Point, count: u8, screen: Size) -> Vec<Point> {
    let count = count.clamp(1, 5);
    (0..count)
        .map(|index| {
            let offset = (f64::from(index) - f64::from(count - 1) / 2.) * 30.;
            on_screen(at.x + offset, at.y, screen)
        })
        .collect()
}

/// The steps of a move from `from` to `to` over `ms`, held `hold_ms` where it ends, then lifted.
fn path(from: &[Point], to: &[Point], ms: u32, hold_ms: u32, edge: Option<Edge>) -> Vec<Timed> {
    let steps = (ms / STEP_MS).max(2);
    let mut timed = vec![fingers(0, from.to_vec(), true, edge)];
    for step in 1..=steps {
        let t = f64::from(step) / f64::from(steps);
        let at = from
            .iter()
            .zip(to)
            .map(|(a, b)| Point {
                x: a.x + (b.x - a.x) * t,
                y: a.y + (b.y - a.y) * t,
            })
            .collect();
        timed.push(fingers(step * STEP_MS, at, true, edge));
    }
    let mut end = steps * STEP_MS;
    // Held where it ended, as a finger that stops before it lifts: the app switcher's gesture,
    // and a scroll that shouldn't glide on.
    let mut held = 0;
    while held < hold_ms {
        held += 50;
        timed.push(fingers(end + held, to.to_vec(), true, edge));
    }
    end += held;
    lift(&mut timed, end, to, edge);
    timed
}

/// The last place sent once more, then the lift: the simulator keeps a contact's newest place
/// until the next event of it, and a lift that comes next drops it (measured: two fingers came
/// a step of thirty-one short with the lift alone).
fn lift(timed: &mut Vec<Timed>, last_ms: u32, at: &[Point], edge: Option<Edge>) {
    timed.push(fingers(last_ms + STEP_MS, at.to_vec(), true, edge));
    timed.push(fingers(
        last_ms + STEP_MS + LIFT_AFTER_MS,
        at.to_vec(),
        false,
        edge,
    ));
}

fn swipe_shares(
    screen: Size,
    from: (f64, f64),
    to: (f64, f64),
    ms: u32,
    hold_ms: u32,
    edge: Edge,
) -> Vec<Timed> {
    let (from, to) = (share(screen, from.0, from.1), share(screen, to.0, to.1));
    path(&[from], &[to], ms, hold_ms, Some(edge))
}

/// Two fingers `spread` points apart about `middle`, on a line `degrees` clockwise from across,
/// each kept on the screen: a pinch near an edge spreads no finger off it.
fn pair(middle: Point, spread: f64, degrees: f64, screen: Size) -> Vec<Point> {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (dx, dy) = (cos * spread / 2., sin * spread / 2.);
    vec![
        on_screen(middle.x - dx, middle.y - dy, screen),
        on_screen(middle.x + dx, middle.y + dy, screen),
    ]
}

/// The place (`x`, `y`), kept a point inside the screen's edges.
fn on_screen(x: f64, y: f64, screen: Size) -> Point {
    Point {
        x: x.clamp(1., screen.width - 1.),
        y: y.clamp(1., screen.height - 1.),
    }
}

/// A button's usage on the consumer page.
pub fn button_usage(button: Button) -> u32 {
    match button {
        Button::Home => 0x40,
        Button::Lock => 0x30,
        Button::VolumeUp => 0xE9,
        Button::VolumeDown => 0xEA,
    }
}

/// `button` pressed `times`, as the tab's bar presses it.
fn button(button: Button, times: u32) -> Vec<Timed> {
    pressed(button, times)
        .into_iter()
        .filter_map(|step| match step.input {
            Input::Button { button, down } => Some(Timed {
                at_ms: step.after_ms,
                touch: Touch::Button {
                    page: CONSUMER,
                    usage: button_usage(button),
                    down,
                },
            }),
            _ => None,
        })
        .collect()
}

/// The steps of `act`, whose places (`spot`s) are already points: `at` and `to`.
pub fn plan(act: &Act, at: Point, to: Point, screen: Size) -> Result<Vec<Timed>, Refusal> {
    let inside = |p: Point| p.x >= 0. && p.y >= 0. && p.x <= screen.width && p.y <= screen.height;
    if !inside(at) {
        return Err(invalid(format!(
            "{:.0},{:.0} is off the screen, which is {:.0} by {:.0} points",
            at.x, at.y, screen.width, screen.height
        )));
    }
    Ok(match act {
        Act::Tap {
            count,
            hold_ms,
            fingers: many,
            ..
        } => {
            let places = side_by_side(at, *many, screen);
            let held = if *hold_ms == 0 { TAP_HELD_MS } else { *hold_ms };
            let mut timed = Vec::new();
            for index in 0..u32::from((*count).clamp(1, 3)) {
                let start = index * (held + TAP_GAP_MS);
                timed.push(fingers(start, places.clone(), true, None));
                timed.push(fingers(start + held, places.clone(), false, None));
            }
            timed
        }
        Act::Swipe {
            ms, fingers: many, ..
        } => {
            if !inside(to) {
                return Err(invalid("the swipe ends off the screen"));
            }
            let (from, to_) = (
                side_by_side(at, *many, screen),
                side_by_side(to, *many, screen),
            );
            let edge = if *many <= 2 {
                edge_of(at, screen)
            } else {
                None
            };
            path(&from, &to_, (*ms).clamp(50, 10_000), 0, edge)
        }
        Act::Scroll { dx, dy, .. } => {
            let margin = screen.width.min(screen.height) * 0.03;
            let to = Point {
                x: (at.x + dx).clamp(margin, screen.width - margin),
                y: (at.y + dy).clamp(margin, screen.height - margin),
            };
            path(&[at], &[to], 300, 150, None)
        }
        Act::Pinch {
            scale, degrees, ms, ..
        } => {
            if !(0.1..=10.).contains(scale) {
                return Err(invalid("scale is from 0.1 to 10"));
            }
            let room = screen.width.min(screen.height) * 0.9;
            let from = (screen.width * 0.35).min(room / scale.max(1.));
            let to = (from * scale).min(room);
            let ms = (*ms).clamp(100, 5_000);
            let mut timed = vec![fingers(0, pair(at, from, 0., screen), true, None)];
            let steps = (ms / STEP_MS).max(2);
            for step in 1..=steps {
                let t = f64::from(step) / f64::from(steps);
                let spread = from + (to - from) * t;
                timed.push(fingers(
                    step * STEP_MS,
                    pair(at, spread, degrees * t, screen),
                    true,
                    None,
                ));
            }
            lift(&mut timed, steps * STEP_MS, &pair(at, to, *degrees, screen), None);
            timed
        }
        Act::Type { text } => typed(text)?,
        Act::Key { combo } => combo_keys(combo)?,
        Act::Button { button: press } => match press {
            Press::Home => button(Button::Home, 1),
            Press::Lock => button(Button::Lock, 1),
            Press::VolumeUp => button(Button::VolumeUp, 1),
            Press::VolumeDown => button(Button::VolumeDown, 1),
            // Home twice, as the Simulator app's App Switcher.
            Press::AppSwitcher => button(Button::Home, 2),
            Press::NotificationCenter => {
                swipe_shares(screen, (0.3, 0.003), (0.3, 0.6), 300, 0, Edge::Top)
            }
            Press::ControlCenter => {
                swipe_shares(screen, (0.9, 0.003), (0.9, 0.5), 300, 0, Edge::Top)
            }
        },
        Act::Turn { .. } => Vec::new(),
    })
}

/// The usage that types `c`, and whether Shift is held for it: ASCII only.
pub fn character(c: char) -> Option<(u16, bool)> {
    const SHIFTED: &str = ")!@#$%^&*(";
    if c.is_ascii_uppercase() {
        return key_usage(&c.to_ascii_lowercase().to_string()).map(|usage| (usage, true));
    }
    if let Some(index) = SHIFTED.find(c) {
        return key_usage(&index.to_string()).map(|usage| (usage, true));
    }
    let plain = match c {
        '\n' => return Some((40, false)),
        '\t' => return Some((43, false)),
        '_' => '-',
        '+' => '=',
        '{' => '[',
        '}' => ']',
        '|' => '\\',
        ':' => ';',
        '"' => '\'',
        '<' => ',',
        '>' => '.',
        '?' => '/',
        '~' => '`',
        other => {
            return key_usage(&other.to_string()).map(|usage| (usage, false));
        }
    };
    key_usage(&plain.to_string()).map(|usage| (usage, true))
}

/// Text typed as characters, Shift round those that need it and no other modifier held.
pub fn typed(text: &str) -> Result<Vec<Timed>, Refusal> {
    let unknown: String = text.chars().filter(|c| character(*c).is_none()).collect();
    if !unknown.is_empty() {
        return Err(invalid(format!(
            "only ASCII is typed here; these characters can't be: {unknown:?}. Type the rest, \
             or leave them out"
        )));
    }
    let key = |at_ms, usage, down| Timed {
        at_ms,
        touch: Touch::Key { usage, down },
    };
    let mut timed = Vec::new();
    let mut t = 0;
    for (usage, shift) in text.chars().filter_map(character) {
        if shift {
            timed.push(key(t, SHIFT, true));
        }
        timed.push(key(t, usage, true));
        timed.push(key(t + KEY_HELD_MS, usage, false));
        if shift {
            timed.push(key(t + KEY_HELD_MS, SHIFT, false));
        }
        t += KEY_EVERY_MS;
    }
    Ok(timed)
}

/// `cmd+space`, `shift+tab`, `escape`: the modifiers pressed round the key.
pub fn combo_keys(combo: &str) -> Result<Vec<Timed>, Refusal> {
    let mut modifiers = Vec::new();
    let mut key = None;
    for part in combo
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "cmd" | "command" | "meta" => modifiers.push(COMMAND),
            "shift" => modifiers.push(SHIFT),
            "ctrl" | "control" => modifiers.push(CONTROL),
            "alt" | "option" | "opt" => modifiers.push(OPTION),
            "return" => key = Some(40),
            "esc" => key = Some(41),
            name => {
                key =
                    Some(key_usage(name).ok_or_else(|| invalid(format!("no key named {part:?}")))?);
            }
        }
    }
    let usage = key.ok_or_else(|| invalid(format!("{combo:?} names no key")))?;
    let press = |at_ms, usage, down| Timed {
        at_ms,
        touch: Touch::Key { usage, down },
    };
    let mut timed: Vec<Timed> = modifiers.iter().map(|&m| press(0, m, true)).collect();
    timed.push(press(10, usage, true));
    timed.push(press(10 + KEY_HELD_MS * 2, usage, false));
    timed.extend(
        modifiers
            .iter()
            .rev()
            .map(|&m| press(20 + KEY_HELD_MS * 2, m, false)),
    );
    Ok(timed)
}

#[cfg(test)]
mod tests;
