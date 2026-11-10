//! The agent desktop's tab (`docs/agent-desktop.md` 9): the desktop drawn live with the
//! agent's pointer over it, or what's in the way of that said with the one thing to do about
//! it. Clicking or typing in it takes over: the person's input goes to the window under it.

use std::time::Instant;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, App, ClickEvent, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    KeyBinding, KeyDownEvent, KeyUpEvent, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render,
    ScrollDelta, ScrollEvent, SharedString, Styled as _, Subscription, Task, TouchPhase, Window,
    div, px,
};
use koyo_desk_wire::keys::{self, flags};
use koyo_desk_wire::link::{Button, DesktopState, Input};

use super::canvas::Canvas;
use super::{Desk, State, marks};
use crate::browser::keys::key_codes;
use crate::modal::{Button as Push, Tone};
use crate::theme;

/// A wheel's click in points, as apps scroll by it.
const LINE: f32 = 40.;
/// The modifier keys: each one's code.
const SHIFT: u16 = 56;
const CONTROL: u16 = 59;
const OPTION: u16 = 58;
const COMMAND: u16 = 55;

/// A key Koyo binds, pressed in the agent's app instead while the desktop has the keyboard:
/// the edit commands, and the keys that would move the focus or close something here.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = agent_desktop, no_json)]
pub struct Press(pub &'static str);

pub fn bind_keys(cx: &mut App) {
    let desktop = Some("AgentDesktop");
    cx.bind_keys(
        [
            ("tab", "tab"),
            ("shift-tab", "shift+tab"),
            ("escape", "escape"),
            ("enter", "return"),
            ("cmd-c", "cmd+c"),
            ("cmd-x", "cmd+x"),
            ("cmd-v", "cmd+v"),
            ("cmd-a", "cmd+a"),
            ("cmd-z", "cmd+z"),
            ("cmd-shift-z", "shift+cmd+z"),
        ]
        .map(|(keys, combo)| KeyBinding::new(keys, Press(combo), desktop)),
    );
}

pub struct DeskView {
    pub(super) desk: Entity<Desk>,
    pub(super) focus: FocusHandle,
    /// The button the person holds down on the desktop.
    pub(super) pressed: Option<Button>,
    /// Where the desktop shows in the window, and how much of its true size it shows at.
    pub(super) placed: Option<(Point<Pixels>, f32)>,
    /// The wake-up for the pointer's fade.
    pub(super) wake: Option<(Instant, Task<()>)>,
    held: Modifiers,
    /// The keys sent down as keys, to be sent up as they're let go.
    down: Vec<u16>,
    _watching: [Subscription; 2],
}

fn button_of(button: MouseButton) -> Option<Button> {
    match button {
        MouseButton::Left => Some(Button::Left),
        MouseButton::Right => Some(Button::Right),
        MouseButton::Middle => Some(Button::Middle),
        MouseButton::Navigate(_) => None,
    }
}

/// The modifiers held, as the flags an event carries.
fn flags_of(modifiers: Modifiers) -> u64 {
    [
        (modifiers.shift, flags::SHIFT),
        (modifiers.control, flags::CONTROL),
        (modifiers.alt, flags::OPTION),
        (modifiers.platform, flags::COMMAND),
    ]
    .into_iter()
    .filter(|(held, _)| *held)
    .fold(0, |all, (_, flag)| all | flag)
}

impl DeskView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let desk = super::desk(cx);
        let picture = desk.read(cx).picture.clone();
        DeskView {
            _watching: [
                cx.observe(&desk, |_, _, cx| cx.notify()),
                cx.observe(&picture, |_, _, cx| cx.notify()),
            ],
            desk,
            focus: cx.focus_handle(),
            pressed: None,
            placed: None,
            wake: None,
            held: Modifiers::default(),
            down: Vec::new(),
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// A point of the window as the desktop counts it, from its top left.
    fn desk_point(&self, at: Point<Pixels>) -> koyo_desk_wire::Point {
        let (origin, fit) = self.placed.unwrap_or_default();
        let fit = if fit > 0. { fit } else { 1. };
        koyo_desk_wire::Point {
            x: f64::from((at.x - origin.x).as_f32() / fit),
            y: f64::from((at.y - origin.y).as_f32() / fit),
        }
    }

    pub(super) fn send(&mut self, input: Input, cx: &mut Context<Self>) {
        self.desk.update(cx, |desk, cx| desk.input(input, cx));
    }

    pub(super) fn pressed_at(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(button) = button_of(event.button) else {
            return;
        };
        window.focus(&self.focus, cx);
        self.pressed = Some(button);
        let input = Input::Down {
            at: self.desk_point(event.position),
            button,
            clicks: u8::try_from(event.click_count).unwrap_or(1),
            flags: flags_of(event.modifiers),
        };
        self.send(input, cx);
    }

    pub(super) fn released_at(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(button) = button_of(event.button) else {
            return;
        };
        if self.pressed.take() == Some(button) {
            let input = Input::Up {
                at: self.desk_point(event.position),
                button,
                clicks: u8::try_from(event.click_count).unwrap_or(1),
                flags: flags_of(event.modifiers),
            };
            self.send(input, cx);
        }
    }

    /// The pointer moved over the desktop, or with a button it pressed there held. A move
    /// alone doesn't take over: it's the person looking, and reaches the window under it
    /// only while they have control.
    pub(super) fn moved_to(&mut self, event: &MouseMoveEvent, over: bool, cx: &mut Context<Self>) {
        let at = self.desk_point(event.position);
        if let Some(button) = self.pressed {
            self.send(Input::Drag { at, button }, cx);
        } else if over && self.desk.read(cx).person_holds() {
            self.send(Input::Move { at }, cx);
        }
    }

    pub(super) fn wheel(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let (dx, dy) = match event.delta {
            ScrollDelta::Pixels(delta) => (delta.x.as_f32(), delta.y.as_f32()),
            ScrollDelta::Lines(delta) => (delta.x * LINE, delta.y * LINE),
        };
        // A trackpad's gesture has phases, which a window that isn't showing needs; a
        // wheel's click has none, and the host makes it a gesture of one step.
        let phase = match (event.delta, event.touch_phase) {
            (ScrollDelta::Lines(_), _) => 0,
            (ScrollDelta::Pixels(_), TouchPhase::Started) => 1,
            (ScrollDelta::Pixels(_), TouchPhase::Moved) => 2,
            (ScrollDelta::Pixels(_), TouchPhase::Ended | TouchPhase::Cancelled) => 4,
        };
        let input = Input::Scroll {
            at: self.desk_point(event.position),
            dx: f64::from(dx),
            dy: f64::from(dy),
            phase,
            momentum: 0,
        };
        self.send(input, cx);
    }

    /// A key that types text goes on to the input method, whose text is sent as text; every
    /// other key (a shortcut, an arrow, a function key) is sent as the key it is.
    fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let commanded = keystroke.modifiers.platform || keystroke.modifiers.control;
        if keystroke.key_char.is_some() && !commanded {
            return;
        }
        let (known, code) = key_codes(&keystroke.key);
        let Ok(code) = u16::try_from(code) else {
            return;
        };
        if known == 0 {
            return;
        }
        self.down.push(code);
        let flags = flags_of(keystroke.modifiers);
        self.send(
            Input::Key {
                down: true,
                code,
                flags,
            },
            cx,
        );
        cx.stop_propagation();
    }

    fn key_up(&mut self, event: &KeyUpEvent, cx: &mut Context<Self>) {
        let (_, code) = key_codes(&event.keystroke.key);
        let Ok(code) = u16::try_from(code) else {
            return;
        };
        if let Some(at) = self.down.iter().position(|down| *down == code) {
            self.down.swap_remove(at);
            let flags = flags_of(event.keystroke.modifiers);
            self.send(
                Input::Key {
                    down: false,
                    code,
                    flags,
                },
                cx,
            );
        }
    }

    /// A modifier pressed or let go reaches the agent's app only while the person has
    /// control: reaching for one of Koyo's shortcuts leaves an agent at work alone.
    fn modifiers_changed(&mut self, now: Modifiers, cx: &mut Context<Self>) {
        let was = std::mem::replace(&mut self.held, now);
        if !self.desk.read(cx).person_holds() {
            return;
        }
        let changed = [
            (SHIFT, was.shift, now.shift),
            (CONTROL, was.control, now.control),
            (OPTION, was.alt, now.alt),
            (COMMAND, was.platform, now.platform),
        ];
        for (code, _, down) in changed.into_iter().filter(|(_, was, now)| was != now) {
            let flags = flags_of(now);
            self.send(Input::Key { down, code, flags }, cx);
        }
    }

    /// A key Koyo binds, pressed in the agent's app: its modifiers down, the key, and up.
    fn press(&mut self, combo: &str, cx: &mut Context<Self>) {
        let Ok(combo) = keys::combo(combo) else {
            return;
        };
        let held: Vec<(u16, u64)> = (keys::MODIFIERS.into_iter())
            .filter(|(_, flag)| combo.flags & flag != 0)
            .collect();
        let mut flags = combo.flags & !keys::MODIFIER_FLAGS;
        for (code, flag) in &held {
            flags |= flag;
            let (down, code) = (true, *code);
            self.send(Input::Key { down, code, flags }, cx);
        }
        for down in [true, false] {
            let (code, flags) = (combo.code, combo.flags);
            self.send(Input::Key { down, code, flags }, cx);
        }
        for (code, flag) in held.iter().rev() {
            flags &= !flag;
            let (down, code) = (false, *code);
            self.send(Input::Key { down, code, flags }, cx);
        }
    }

    /// What is in the way of showing the desktop, and what to do about it.
    fn notice(&self, cx: &mut Context<Self>) -> Option<(SharedString, Vec<Push>)> {
        let desk = self.desk.read(cx);
        let turn_on = || {
            Push::new("agent-desktop-on", "Turn On", Tone::Primary).on_click(|_, _, cx| {
                crate::settings::update(cx, |settings| settings.agent_desktop = true);
            })
        };
        let start = cx.listener(|this, _: &ClickEvent, _, cx| {
            this.desk.update(cx, Desk::start);
        });
        let restart = cx.listener(|this, _: &ClickEvent, _, cx| {
            this.desk.update(cx, Desk::restart);
        });
        let (message, action): (SharedString, Option<Push>) = match (
            &desk.state,
            desk.grants,
            &desk.desktop,
        ) {
            (State::Off, _, _) => (
                "The agent desktop is off. On, agents open and drive apps on a desktop of their \
                 own, shown here, while you keep working on yours."
                    .into(),
                Some(turn_on()),
            ),
            (State::Failed(why), _, _) => (
                why.clone().into(),
                Some(
                    Push::new("agent-desktop-start", "Start Again", Tone::Primary).on_click(start),
                ),
            ),
            (State::Running, Some(grants), _)
                if !(grants.accessibility && grants.screen_recording) =>
            {
                let needs = if grants.accessibility || grants.screen_recording {
                    "one more permission"
                } else {
                    "two permissions"
                };
                (
                    format!(
                        "The agent desktop needs {needs} from macOS before it can work. \
                         Setting it up takes a minute."
                    )
                    .into(),
                    Some(
                        Push::new("agent-desktop-setup", "Set Up…", Tone::Primary)
                            .on_click(|_, window, cx| super::setup::open(window, cx)),
                    ),
                )
            }
            (State::Running, _, Some(DesktopState::Missing { why })) => (
                format!("The agent desktop isn't there: {why}.").into(),
                Some(
                    Push::new("agent-desktop-restart", "Try Again", Tone::Primary)
                        .on_click(restart),
                ),
            ),
            (State::Running, _, Some(DesktopState::Ready { .. })) => return None,
            (State::Starting, _, _) | (State::Running, _, None) => {
                ("Starting the agent desktop…".into(), None)
            }
        };
        Some((message, action.into_iter().collect()))
    }
}

impl Render for DeskView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let notice = self.notice(cx);
        let empty = notice.is_none() && {
            let desk = self.desk.read(cx);
            desk.windows.is_empty() && desk.backdrop.is_none()
        };
        // Only with the person at this window: a dialog that opens while Koyo is behind
        // another app takes the keyboard from the app they're typing in.
        if window.is_window_active() && self.desk.read(cx).setup_due() {
            window.defer(cx, super::setup::offer);
        }
        let marks = if notice.is_none() {
            marks::overlay(self, window, cx)
        } else {
            None
        };
        div()
            .id("agent-desktop")
            .key_context("AgentDesktop")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            // No background of its own, as a chat has none: the window's card shows round
            // the desktop. Painted again here, the card's colour stacked into a flat grey.
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key_down(event, cx)))
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| this.key_up(event, cx)))
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                this.modifiers_changed(event.modifiers, cx);
            }))
            .on_action(cx.listener(|this, Press(combo): &Press, _, cx| this.press(combo, cx)))
            .when(notice.is_none(), |view| {
                view.child(Canvas {
                    view: cx.entity(),
                    desk: self.desk.clone(),
                    focused: self.focus.is_focused(window),
                    palette: p,
                })
            })
            .children(marks)
            .children(self.notices(cx))
            .children(self.control_bar(window, cx))
            .when(empty, |view| {
                view.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(p.muted)
                        .child("No app is open on the agent desktop."),
                )
            })
            .children(notice.map(|(message, action)| {
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(
                        div()
                            .max_w(px(460.))
                            .text_center()
                            .text_sm()
                            .text_color(p.muted)
                            .child(message),
                    )
                    .child(div().flex().gap_2().children(action))
            }))
    }
}
