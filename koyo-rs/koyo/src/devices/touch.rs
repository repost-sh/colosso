//! The person's hand in a device's tab (`docs/devices.md` 8): the Mac's pointer, trackpad and
//! keys turned into touches, keys and buttons by `koyo-device-gestures`, as the proof of concept's
//! window did. A move or a modifier alone doesn't take the device over. Koyo's own shortcuts stay
//! Koyo's; ⇧⌘H is Home, and ⌘← and ⌘→ turn a simulator.

use std::time::Duration;

use gpui_kit::{
    Context, KeyDownEvent, KeyUpEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PinchEvent, Pixels, Point, ScrollDelta, ScrollEvent, TouchPhase, Window,
};
use koyo_device_gestures::{At, Mac, Phase, Step, key_usage};
use koyo_device_wire::{Button, Input, Orientation};

use super::view::DeviceView;

/// How often a resting trackpad's fingers are looked at, to be lifted.
const TICK: Duration = Duration::from_millis(100);

pub fn phase(phase: TouchPhase) -> Phase {
    match phase {
        TouchPhase::Started => Phase::Started,
        TouchPhase::Moved => Phase::Moved,
        // Cancelled too: the system took the gesture, and nothing more comes of it.
        TouchPhase::Ended | TouchPhase::Cancelled => Phase::Ended,
    }
}

impl DeviceView {
    /// A point of the window as a share of the picture, and whether it's on it.
    fn share(&self, at: Point<Pixels>, cx: &Context<Self>) -> Option<(At, bool)> {
        let (origin, fit) = self.placed?;
        let screen = self.link.as_ref()?.read(cx).screen()?;
        let (wide, high) = (screen.size.width as f32 * fit, screen.size.height as f32 * fit);
        if wide <= 0. || high <= 0. {
            return None;
        }
        let x = (at.x - origin.x).as_f32() / wide;
        let y = (at.y - origin.y).as_f32() / high;
        Some((At::new(x, y), (0. ..=1.).contains(&x) && (0. ..=1.).contains(&y)))
    }

    fn millis(&self) -> u64 {
        u64::try_from(self.born.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// What the mapping makes of `event`, sent on; a step that comes later goes after its gap.
    pub(super) fn mac(&mut self, event: Mac, cx: &mut Context<Self>) {
        let now = self.millis();
        let steps = self.fingers.take(event, now);
        self.steps(steps, cx);
        if self.fingers.down() && self.ticking.is_none() {
            self.ticking = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(TICK).await;
                    let more = this.update(cx, |this, cx| {
                        let now = this.millis();
                        let steps = this.fingers.tick(now);
                        this.steps(steps, cx);
                        this.fingers.down()
                    });
                    if !matches!(more, Ok(true)) {
                        break;
                    }
                }
                let _ = this.update(cx, |this, _| this.ticking = None);
            }));
        }
    }

    fn steps(&mut self, steps: Vec<Step>, cx: &mut Context<Self>) {
        for step in steps {
            if step.after_ms == 0 {
                self.send(step.input, cx);
            } else {
                let after = Duration::from_millis(u64::from(step.after_ms));
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(after).await;
                    let _ = this.update(cx, |this, cx| this.send(step.input, cx));
                })
                .detach();
            }
        }
    }

    pub(super) fn send(&mut self, input: Input, cx: &mut Context<Self>) {
        if let Some(link) = &self.link {
            link.update(cx, |link, cx| link.input(input, cx));
        }
    }

    pub(super) fn pressed_at(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        window.focus(&self.focus, cx);
        let Some((at, _)) = self.share(event.position, cx) else {
            return;
        };
        self.pressed = true;
        self.mac(
            Mac::Press {
                at,
                option: event.modifiers.alt,
            },
            cx,
        );
    }

    pub(super) fn released_at(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left || !std::mem::take(&mut self.pressed) {
            return;
        }
        let Some((at, _)) = self.share(event.position, cx) else {
            return;
        };
        self.mac(Mac::Release { at }, cx);
    }

    /// The pointer moved: a drag while the button is held; otherwise only where it is, for the
    /// trackpad's gestures, which GPUI tells without a place of their own.
    pub(super) fn moved_to(&mut self, event: &MouseMoveEvent, over: bool, cx: &mut Context<Self>) {
        let Some((at, _)) = self.share(event.position, cx) else {
            return;
        };
        if over {
            self.pointer = Some(at);
            super::view::hovered(cx.entity().downgrade(), cx);
        }
        if self.pressed {
            self.mac(
                Mac::Drag {
                    at,
                    shift: event.modifiers.shift,
                },
                cx,
            );
        }
    }

    /// Two fingers scrolling on the trackpad drag a finger on the device. A mouse's wheel does
    /// nothing: its notches are lines, no distance, and steps as fine as a trackpad's that never
    /// say they began are left by the mapping.
    pub(super) fn scrolled(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let ScrollDelta::Pixels(by) = event.delta else {
            return;
        };
        // Where the pointer is, and where the scroll would take it: how far it goes, in the
        // picture's shares.
        let (Some((at, on)), Some((to, _))) = (self.share(event.position, cx), self.share(event.position + by, cx)) else {
            return;
        };
        if !on && !self.fingers.down() {
            return;
        }
        let (phase, dx, dy) = (phase(event.touch_phase), to.x - at.x, to.y - at.y);
        self.mac(Mac::Scroll { at, phase, dx, dy }, cx);
    }

    pub(super) fn pinch(&mut self, event: &PinchEvent, cx: &mut Context<Self>) {
        let Some((at, on)) = self.share(event.position, cx) else {
            return;
        };
        if !on && !self.fingers.down() {
            return;
        }
        self.pointer = Some(at);
        let phase = phase(event.phase);
        self.mac(Mac::Pinch { at, phase, delta: event.delta }, cx);
    }

    /// A rotation or a two-finger double tap on the trackpad, or its fingers lifting before
    /// they scrolled, heard by the window's monitor.
    pub(super) fn trackpad(&mut self, gesture: super::view::Trackpad, cx: &mut Context<Self>) {
        let Some(at) = self.pointer else {
            return;
        };
        match gesture {
            super::view::Trackpad::Rotate { phase, degrees } => self.mac(Mac::Rotate { at, phase, degrees }, cx),
            super::view::Trackpad::SmartZoom => self.mac(Mac::SmartZoom { at }, cx),
            super::view::Trackpad::Lifted => {
                let (phase, dx, dy) = (Phase::Ended, 0., 0.);
                self.mac(Mac::Scroll { at, phase, dx, dy }, cx);
            }
        }
    }

    /// A key that types text is left to the input method, whose text goes as text; any other
    /// key (an arrow, Return, a shortcut of the device's own) goes as its usage.
    pub(super) fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let held = keystroke.modifiers;
        if held.platform {
            let simulator = self.simulator();
            match (keystroke.key.as_str(), held.shift) {
                ("h", true) => self.press_button(Button::Home, cx),
                ("left", false) if simulator => self.turn(true, cx),
                ("right", false) if simulator => self.turn(false, cx),
                _ => return,
            }
            cx.stop_propagation();
            return;
        }
        if keystroke.key_char.is_some() && !held.control {
            return;
        }
        let Some(usage) = key_usage(&keystroke.key) else {
            return;
        };
        self.keys_down.push(usage);
        self.send(Input::Key { usage, down: true }, cx);
        cx.stop_propagation();
    }

    pub(super) fn key_up(&mut self, event: &KeyUpEvent, cx: &mut Context<Self>) {
        let Some(usage) = key_usage(&event.keystroke.key) else {
            return;
        };
        if let Some(at) = self.keys_down.iter().position(|down| *down == usage) {
            self.keys_down.swap_remove(at);
            self.send(Input::Key { usage, down: false }, cx);
        }
    }

    pub(super) fn press_button(&mut self, button: Button, cx: &mut Context<Self>) {
        self.steps(koyo_device_gestures::pressed(button, 1), cx);
    }

    /// The app switcher, as the Simulator app brings it up: Home pressed twice, close together.
    pub(super) fn app_switcher(&mut self, cx: &mut Context<Self>) {
        self.steps(koyo_device_gestures::pressed(Button::Home, 2), cx);
    }

    /// Turns a simulator a quarter, to the left or the right of how its picture faces now.
    pub(super) fn turn(&mut self, left: bool, cx: &mut Context<Self>) {
        let Some(screen) = self.link.as_ref().and_then(|link| link.read(cx).screen()) else {
            return;
        };
        let quarters = screen.orientation.quarters();
        let to = Orientation::from_quarters(if left { quarters + 1 } else { quarters + 3 });
        self.send(Input::Turn { to }, cx);
    }
}
