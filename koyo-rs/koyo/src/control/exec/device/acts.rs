//! Acting on a device: touches, typing, keys, buttons and turning. Each takes the device for
//! its caller (refused while the person holds it) and answers what the act did to the screen.

use std::fmt::Write as _;
use std::time::Duration;

use gpui_kit::{App, AsyncApp};
use koyo_app_control::ControlError;
use koyo_app_control::ops::device::{
    DeviceActed, DeviceButtonInput, DeviceKeyInput, DevicePinchInput, DeviceRotateInput,
    DeviceScrollInput, DeviceSwipeInput, DeviceTapInput, DeviceTypeInput, Facing, MAX_TYPED,
    Pressed, Region, spot as checked_spot,
};
use koyo_device_wire::{Act, Answer, Ask, Change, Orientation, Point, Press, Spot};

use super::{ACT, Named, Target, act_on, claim, refused, unexpected, which};
use crate::control::Caller;
use crate::control::exec::{Exec, output};

/// Typing takes this, and [`PER_CHARACTER`] for each character, at most [`TYPING_MOST`].
const TYPING: Duration = Duration::from_secs(10);
const PER_CHARACTER: Duration = Duration::from_millis(40);
const TYPING_MOST: Duration = Duration::from_secs(90);

/// The spot a `ref` or `x` and `y` name, as the host takes it.
fn spot(reference: Option<&str>, x: Option<f64>, y: Option<f64>) -> Result<Spot, ControlError> {
    use koyo_app_control::ops::device::Spot as Given;
    Ok(match checked_spot(reference, x, y)? {
        Given::Ref(reference) => Spot::Ref(reference),
        Given::Point(x, y) => Spot::Point(Point { x, y }),
    })
}

/// `value`, or `default` when left out; refused outside `range`, named `what`.
fn within<T: PartialOrd + Copy + std::fmt::Display>(
    value: Option<T>,
    default: T,
    range: std::ops::RangeInclusive<T>,
    what: &str,
) -> Result<T, ControlError> {
    let value = value.unwrap_or(default);
    if range.contains(&value) {
        Ok(value)
    } else {
        Err(ControlError::invalid_input(format!(
            "`{what}` is {} to {}, not {value}.",
            range.start(),
            range.end()
        )))
    }
}

/// An act's answer, with what it did in a line.
fn acted(name: String, did: String) -> impl FnOnce(Answer) -> Result<serde_json::Value, ControlError> {
    move |answer| match answer {
        Answer::Acted {
            change,
            region,
            still,
            millis,
        } => output(&DeviceActed {
            name,
            change: match change {
                Change::None => "none",
                Change::Small => "small",
                Change::Large => "large",
            }
            .into(),
            region: region.map(|region| Region {
                x: region.x,
                y: region.y,
                width: region.width,
                height: region.height,
            }),
            still,
            millis: u64::from(millis),
            did,
        }),
        other => Err(unexpected(&other)),
    }
}

/// Runs `act` for `caller` on the device `at` names; `did` is its line.
fn act(
    caller: &Caller,
    at: Named<'_>,
    act: Result<(Act, String), ControlError>,
    limit: Duration,
    cx: &mut App,
) -> Result<Exec, ControlError> {
    let (act, did) = act?;
    act_on(caller, at, Ok(Ask::Act(act)), limit, cx, |target| acted(target.name.clone(), did))
}

fn named(spot: &Spot) -> String {
    match spot {
        Spot::Ref(reference) => reference.clone(),
        Spot::Point(Point { x, y }) => format!("{x:.0},{y:.0}"),
    }
}

pub(crate) fn tap(caller: &Caller, input: &DeviceTapInput, cx: &mut App) -> Result<Exec, ControlError> {
    let spot = spot(input.r#ref.as_deref(), input.x, input.y)?;
    let count = within(input.count, 1, 1..=3, "count")?;
    let hold_ms = within(input.hold_ms, 0, 0..=10_000, "holdMs")?;
    let fingers = within(input.fingers, 1, 1..=5, "fingers")?;
    let did = match (count, hold_ms) {
        (_, 500..) => format!("Long-pressed {}", named(&spot)),
        (2, _) => format!("Double-tapped {}", named(&spot)),
        (3, _) => format!("Triple-tapped {}", named(&spot)),
        _ => format!("Tapped {}", named(&spot)),
    };
    // Each touch held as long as asked comes on top of the act's own time.
    let limit = ACT + Duration::from_millis(u64::from(hold_ms) * u64::from(count));
    let act_ = Act::Tap { spot, count, hold_ms, fingers };
    act(caller, (input.device.as_deref(), input.pane), Ok((act_, did)), limit, cx)
}

pub(crate) fn swipe(caller: &Caller, input: &DeviceSwipeInput, cx: &mut App) -> Result<Exec, ControlError> {
    let checked = (|| {
        let from = spot(input.r#ref.as_deref(), input.x, input.y)?;
        let (Some(x), Some(y)) = (input.to_x, input.to_y) else {
            return Err(ControlError::invalid_input("Give `toX` and `toY`: where the swipe ends."));
        };
        let ms = within(input.ms, 300, 50..=5_000, "ms")?;
        let fingers = within(input.fingers, 1, 1..=5, "fingers")?;
        let did = format!("Swiped from {} to {x:.0},{y:.0}", named(&from));
        let to = Spot::Point(Point { x, y });
        Ok((Act::Swipe { from, to, ms, fingers }, did))
    })();
    act(caller, (input.device.as_deref(), input.pane), checked, ACT, cx)
}

pub(crate) fn scroll(caller: &Caller, input: &DeviceScrollInput, cx: &mut App) -> Result<Exec, ControlError> {
    let checked = (|| {
        let spot = spot(input.r#ref.as_deref(), input.x, input.y)?;
        let (dx, dy) = (input.dx.unwrap_or(0.), input.dy.unwrap_or(0.));
        if !(dx.is_finite() && dy.is_finite()) || (dx == 0. && dy == 0.) {
            return Err(ControlError::invalid_input("Give `dx` or `dy`: how far the content moves, in points."));
        }
        let did = format!("Scrolled {dx:.0},{dy:.0} at {}", named(&spot));
        Ok((Act::Scroll { spot, dx, dy }, did))
    })();
    act(caller, (input.device.as_deref(), input.pane), checked, ACT, cx)
}

pub(crate) fn pinch(caller: &Caller, input: &DevicePinchInput, cx: &mut App) -> Result<Exec, ControlError> {
    let checked = (|| {
        let spot = spot(input.r#ref.as_deref(), input.x, input.y)?;
        let (scale, degrees) = (input.scale.unwrap_or(1.), input.degrees.unwrap_or(0.));
        let still = (scale - 1.).abs() < f64::EPSILON && degrees.abs() < f64::EPSILON;
        if !(scale.is_finite() && degrees.is_finite()) || scale <= 0. || still {
            return Err(ControlError::invalid_input(
                "Give `scale` (above 0: 2 spreads to twice the size) or `degrees` (clockwise).",
            ));
        }
        let ms = within(input.ms, 500, 100..=5_000, "ms")?;
        let did = format!("Pinched to {scale} turning {degrees}° at {}", named(&spot));
        Ok((Act::Pinch { spot, scale, degrees, ms }, did))
    })();
    act(caller, (input.device.as_deref(), input.pane), checked, ACT, cx)
}

/// The text to type, if the device can type all of it: ASCII, tabs and new lines, at most
/// [`MAX_TYPED`] characters.
fn typing(text: &str) -> Result<&str, ControlError> {
    if text.is_empty() {
        return Err(ControlError::invalid_input("`text` is empty."));
    }
    if text.chars().count() > MAX_TYPED {
        return Err(ControlError::invalid_input(format!("`text` is at most {MAX_TYPED} characters.")));
    }
    let mut odd: Vec<char> = text
        .chars()
        .filter(|c| !(c.is_ascii_graphic() || matches!(c, ' ' | '\n' | '\t')))
        .collect();
    odd.dedup();
    if odd.is_empty() {
        return Ok(text);
    }
    let named = odd.iter().take(10).fold(String::new(), |mut named, c| {
        let _ = write!(named, "{c:?} ");
        named
    });
    Err(ControlError::invalid_input(format!(
        "The device can type only ASCII, and this has {}: {named}Nothing was typed.",
        if odd.len() == 1 { "a character it can't" } else { "characters it can't" }
    )))
}

/// How long typing `text` may take.
fn typing_limit(text: &str) -> Duration {
    let characters = u32::try_from(text.chars().count()).unwrap_or(u32::MAX);
    (TYPING + PER_CHARACTER.saturating_mul(characters)).min(TYPING_MOST)
}

pub(crate) fn type_text(caller: &Caller, input: &DeviceTypeInput, cx: &mut App) -> Result<Exec, ControlError> {
    let text = typing(&input.text)?.to_owned();
    let did = format!("Typed {} characters", text.chars().count());
    let limit = typing_limit(&text);
    let at = (input.device.as_deref(), input.pane);
    let Some(into) = input.into.clone() else {
        return act(caller, at, Ok((Act::Type { text }, did)), limit, cx);
    };
    // Into a field: tapped first, then typed, both as this caller's.
    let target = which(caller, at.0, at.1, cx)?;
    if let Some(error) = super::not_running(target.link.read(cx)) {
        return Err(error);
    }
    claim(caller, &target, cx)?;
    let tap = Act::Tap {
        spot: Spot::Ref(into),
        count: 1,
        hold_ms: 0,
        fingers: 1,
    };
    let Target { link, name, .. } = target;
    let tapped = link.update(cx, |link, cx| link.ask(Ask::Act(tap), ACT, cx));
    Ok(Exec::Later(cx.spawn(async move |cx: &mut AsyncApp| {
        tapped.await.map_err(refused)?;
        let typed = cx.update(|cx| link.update(cx, |link, cx| link.ask(Ask::Act(Act::Type { text }), limit, cx)));
        typed.await.map_err(refused).and_then(acted(name, did))
    })))
}

pub(crate) fn key(caller: &Caller, input: &DeviceKeyInput, cx: &mut App) -> Result<Exec, ControlError> {
    let combo = input.combo.trim().to_owned();
    let checked = if combo.is_empty() {
        Err(ControlError::invalid_input("`combo` is empty: `cmd+space`, `escape`, `return`."))
    } else {
        Ok((Act::Key { combo: combo.clone() }, format!("Pressed {combo}")))
    };
    act(caller, (input.device.as_deref(), input.pane), checked, ACT, cx)
}

pub(crate) fn button(caller: &Caller, input: &DeviceButtonInput, cx: &mut App) -> Result<Exec, ControlError> {
    let (button, name) = match input.button {
        Pressed::Home => (Press::Home, "Home"),
        Pressed::Lock => (Press::Lock, "the lock button"),
        Pressed::VolumeUp => (Press::VolumeUp, "volume up"),
        Pressed::VolumeDown => (Press::VolumeDown, "volume down"),
        Pressed::AppSwitcher => (Press::AppSwitcher, "the app switcher"),
        Pressed::ControlCenter => (Press::ControlCenter, "Control Center"),
        Pressed::NotificationCenter => (Press::NotificationCenter, "Notification Center"),
    };
    let checked = Ok((Act::Button { button }, format!("Pressed {name}")));
    act(caller, (input.device.as_deref(), input.pane), checked, ACT, cx)
}

pub(crate) fn rotate(caller: &Caller, input: &DeviceRotateInput, cx: &mut App) -> Result<Exec, ControlError> {
    let to = match input.to {
        Facing::Portrait => Orientation::Portrait,
        Facing::PortraitUpsideDown => Orientation::PortraitUpsideDown,
        Facing::LandscapeLeft => Orientation::LandscapeLeft,
        Facing::LandscapeRight => Orientation::LandscapeRight,
    };
    let did = format!("Turned to {}", super::looks::facing(to));
    act(caller, (input.device.as_deref(), input.pane), Ok((Act::Turn { to }, did)), ACT, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_the_device_can_type_is_typed() {
        assert_eq!(typing("hello, world\n").ok(), Some("hello, world\n"));
        let error = typing("café ☕").unwrap_err();
        assert!(error.message.contains("'é'") && error.message.contains("'☕'"), "{}", error.message);
        assert!(typing("").is_err());
        assert!(typing(&"a".repeat(MAX_TYPED + 1)).is_err());
        assert_eq!(typing_limit(""), Duration::from_secs(10));
        assert_eq!(typing_limit(&"a".repeat(500)), Duration::from_secs(30));
        assert_eq!(typing_limit(&"a".repeat(2000)), Duration::from_secs(90));
    }

    #[test]
    fn numbers_are_checked_against_their_range() {
        assert_eq!(within(None, 1_u8, 1..=3, "count").ok(), Some(1));
        assert_eq!(within(Some(3_u8), 1, 1..=3, "count").ok(), Some(3));
        let error = within(Some(4_u8), 1, 1..=3, "count").unwrap_err();
        assert_eq!(error.message, "`count` is 1 to 3, not 4.");
    }
}
