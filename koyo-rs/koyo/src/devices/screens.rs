//! What a device's tab says (`docs/devices.md` 8): the state it stands in with its one way out,
//! the slim toolbar, the picker, the bar that says who has the device, and an agent's touches.
//! Every state has a way out or moves on by itself: a dead end is what angered the owner most.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use koyo_device_wire::{Mark, Missing, State as Said, Step};

use super::link::{Link, State};
use super::view::{Chosen, DeviceView};
use crate::browser::control::Holder;
use crate::held::Held;
use crate::modal::{Button as Push, Tone};
use crate::theme::Palette;

/// What is said over or instead of the picture.
pub struct Notice {
    pub message: String,
    pub buttons: Vec<Push>,
    /// Whether the picture stays under it (a locked device shows its lock screen).
    pub over: bool,
}

impl Notice {
    fn new(message: String, buttons: Vec<Push>) -> Notice {
        Notice {
            message,
            buttons,
            over: false,
        }
    }

    pub fn element(self, p: Palette) -> AnyElement {
        let block = div()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .child(div().max_w(px(460.)).text_center().text_sm().text_color(p.muted).child(self.message))
            .child(div().flex().gap_2().children(self.buttons));
        if self.over {
            div().absolute().bottom(px(64.)).left_0().right_0().flex().justify_center().child(block).into_any_element()
        } else {
            div().absolute().inset_0().flex().items_center().justify_center().child(block).into_any_element()
        }
    }
}

impl DeviceView {
    fn start_button(&self, label: &'static str, cx: &mut Context<Self>) -> Push {
        let start = cx.listener(|this, _: &ClickEvent, _, cx| {
            if let Some(link) = &this.link {
                link.update(cx, Link::start);
            }
        });
        Push::new("device-start", label, Tone::Primary).on_click(start)
    }

    /// A device that is no longer there: the tab says so and offers the picker.
    fn not_there(&self, name: &str, cx: &mut Context<Self>) -> Notice {
        let pick = cx.listener(|this, _: &ClickEvent, _, cx| this.choose_another(cx));
        Notice::new(
            format!("{name} is no longer among Koyo's devices."),
            vec![Push::new("device-choose", "Choose Another…", Tone::Primary).on_click(pick)],
        )
    }

    /// What is in the way of showing the device, and what to do about it.
    pub(super) fn notice(&self, cx: &mut Context<Self>) -> Option<Notice> {
        let (chosen, link) = (self.device.as_ref()?, self.link.as_ref()?);
        let (state, said, since) = {
            let link = link.read(cx);
            (link.state.clone(), link.said.clone(), link.started_at)
        };
        let name = &chosen.name;
        let listed = super::devices(cx).read(cx).simulators.as_ref().and_then(|listed| listed.as_ref().ok());
        if state == State::Off && listed.is_some_and(|listed| listed.iter().all(|simulator| simulator.udid != chosen.udid)) {
            return Some(self.not_there(name, cx));
        }
        match (&state, &said) {
            (State::Off, _) => Some(Notice::new(format!("{name} isn't running."), vec![self.start_button("Start", cx)])),
            (State::Failed(why), _) => Some(Notice::new(why.clone(), vec![self.start_button("Start Again", cx)])),
            (State::Starting | State::Running, None) => Some(Notice::new(format!("Starting {name}…"), Vec::new())),
            (_, Some(Said::Starting { step })) => Some(Notice::new(starting(name, *step, since), Vec::new())),
            (_, Some(Said::Ready { .. })) => None,
            (_, Some(Said::Locked { .. })) => Some(Notice {
                message: format!("Unlock your {} to continue.", self.kind()),
                buttons: Vec::new(),
                over: true,
            }),
            (_, Some(Said::Gone { why })) => Some(Notice::new(
                format!("{name} isn't there: {why}."),
                vec![self.start_button("Start Again", cx)],
            )),
            (_, Some(Said::Unavailable { code, why })) => Some(self.unavailable(*code, why, cx)),
        }
    }

    fn unavailable(&self, code: Missing, why: &str, cx: &mut Context<Self>) -> Notice {
        match code {
            Missing::Simulator => self.not_there(&self.title(), cx),
            Missing::Xcode => Notice::new(
                "Koyo's devices need Xcode 27 or later. Install it from the App Store, open it \
                 once, then start the device again."
                    .into(),
                vec![self.start_button("Start Again", cx)],
            ),
            Missing::System => Notice::new(
                format!(
                    "This {kind} runs an older system. Koyo drives devices on iOS 27 or later: \
                     update it in Settings → General → Software Update.",
                    kind = self.kind()
                ),
                Vec::new(),
            ),
            _ => Notice::new(why.to_owned(), vec![self.start_button("Start Again", cx)]),
        }
    }

    /// The slim bar over the picture: the device and how it stands, and its buttons.
    pub(super) fn toolbar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = crate::theme::palette(cx);
        let chosen = self.device.as_ref()?;
        let ready = self.link.as_ref().is_some_and(|link| link.read(cx).ready());
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px_2()
                .py_1()
                .rounded_md()
                .text_xs()
                .text_color(if ready { p.text } else { p.muted })
                .when(ready, |b| b.hover(|b| b.bg(p.hover)).cursor_pointer())
                .child(label)
        };
        let home = button("device-home", "Home")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.press_button(koyo_device_wire::Button::Home, cx)));
        let switcher = button("device-switcher", "App Switcher")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.app_switcher(cx)));
        let left = button("device-rotate-left", "Rotate Left").on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.turn(true, cx)));
        let right = button("device-rotate-right", "Rotate Right").on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.turn(false, cx)));
        let lock = button("device-lock", "Lock")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.press_button(koyo_device_wire::Button::Lock, cx)));
        let state = self.link.as_ref().map(|link| state_words(link.read(cx))).unwrap_or_default();
        Some(
            div()
                .flex()
                .items_center()
                .gap_1()
                .px_3()
                .py_1()
                .text_xs()
                .child(div().text_color(p.text).child(chosen.name.clone()))
                .child(div().text_color(p.muted).child(state))
                .child(div().flex_1())
                .children([home, switcher, left, right, lock].map(IntoElement::into_any_element))
                .into_any_element(),
        )
    }

    /// The picker: Koyo's simulators, each opened in this tab on a click.
    pub(super) fn picker_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let devices = super::devices(cx);
        let listed = devices.read(cx).simulators.clone();
        let mut list = div().absolute().inset_0().flex().flex_col().items_center().justify_center().gap_2();
        match listed {
            None => list = list.child(div().text_sm().text_color(p.muted).child("Looking for Koyo's simulators…")),
            Some(Err(why)) => list = list.child(div().max_w(px(460.)).text_sm().text_color(p.muted).child(why)),
            Some(Ok(all)) if all.is_empty() => {
                let make = cx.listener(|_, _: &ClickEvent, _, cx| {
                    let view = cx.entity().downgrade();
                    super::devices(cx).update(cx, |devices, cx| {
                        devices.make_default(cx, move |made, cx| match made {
                            Ok(made) => {
                                let chosen = Chosen::from(&made);
                                let _ = view.update(cx, |view, cx| view.choose(chosen, true, cx));
                            }
                            Err(why) => log::warn!("no simulator was made: {why}"),
                        });
                    });
                });
                list = list
                    .child(div().text_sm().text_color(p.muted).child("Koyo has no simulator of its own yet."))
                    .child(Push::new("device-make", "Make an iPhone 17 Pro", Tone::Primary).on_click(make));
            }
            Some(Ok(all)) => {
                list = list.child(div().text_sm().text_color(p.muted).child("Choose a device"));
                for (index, simulator) in all.into_iter().enumerate() {
                    let chosen = Chosen::from(&simulator);
                    let row = div()
                        .id(SharedString::from(format!("device-pick-{index}")))
                        .w(px(320.))
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .cursor_pointer()
                        .hover(|row| row.bg(p.hover))
                        .flex()
                        .justify_between()
                        .child(div().text_sm().text_color(p.text).child(simulator.name))
                        .child(div().text_xs().text_color(p.muted).child(simulator.system))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.choose(chosen.clone(), true, cx)));
                    list = list.child(row);
                }
            }
        }
        list.into_any_element()
    }

    /// The app switcher, as a person brings it up: a slow swipe from the bottom edge, held a
    /// second before the finger lifts.
    pub(super) fn app_switcher(&mut self, cx: &mut Context<Self>) {
        use koyo_device_gestures::{At, Mac};
        const STEPS: u16 = 38;
        const APART: std::time::Duration = std::time::Duration::from_millis(16);
        let at = |step: u16| At::new(0.5, 0.995 - (0.995 - 0.72) * (f32::from(step) / f32::from(STEPS)));
        cx.spawn(async move |view, cx| {
            for step in 0..=STEPS {
                let event = if step == 0 {
                    Mac::Press { at: at(step), option: false }
                } else {
                    Mac::Drag { at: at(step), shift: false }
                };
                if view.update(cx, |view, cx| view.mac(event, cx)).is_err() {
                    return;
                }
                cx.background_executor().timer(APART).await;
            }
            cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
            let _ = view.update(cx, |view, cx| view.mac(Mac::Release { at: at(STEPS) }, cx));
        })
        .detach();
    }

    /// The bar that says who has the device while it's contested, with the person's way to
    /// take it or give it back: the browser's own pill.
    pub(super) fn control_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let link = self.link.as_ref()?;
        let (name, control) = {
            let link = link.read(cx);
            (link.name.clone(), (link.control.holder.clone(), link.control.displaced.clone()))
        };
        let held = match control {
            (Some(Holder::Agent { actor, .. }), _) => {
                let take = link.downgrade();
                Held {
                    text: format!("{} is using {name}", actor.label),
                    id: "device-take-control",
                    label: "Take control",
                    on_click: Box::new(move |_, _, cx| {
                        let _ = take.update(cx, |link, cx| link.person_acts(crate::browser::control::Mode::Explicit, cx));
                    }),
                }
            }
            (Some(Holder::Human { mode, .. }), Some(displaced)) => {
                let back = link.downgrade();
                let text = match mode {
                    crate::browser::control::Mode::Implicit => format!("{} waits while you use {name}", displaced.label),
                    crate::browser::control::Mode::Explicit => format!("You took over {name} from {}", displaced.label),
                };
                Held {
                    text,
                    id: "device-give-back",
                    label: "Give back",
                    on_click: Box::new(move |_, _, cx| {
                        let _ = back.update(cx, Link::give_back);
                    }),
                }
            }
            _ => return None,
        };
        // Its glass is a native view keyed by this id: one per tab, as a page's is.
        let glass = gpui_kit::ElementId::NamedInteger("device-control-glass".into(), cx.entity_id().as_u64());
        Some(crate::held::bar(glass, held, None, window, cx))
    }

    /// An agent's last touch, marked where it is on the picture.
    pub(super) fn marks(&self, link: &Entity<Link>, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = crate::theme::palette(cx);
        let (origin, fit) = self.placed?;
        let touch = link.read(cx).touch.clone()?;
        // A finger on the glass is filled; one just lifted is a ring, until the mark goes.
        let fill = if touch.kind == Mark::Up { 0. } else { 0.25 };
        let marks = touch.at.into_iter().map(move |at| {
            let x = origin.x + px(at.x as f32 * fit) - px(11.);
            let y = origin.y + px(at.y as f32 * fit) - px(11.);
            div()
                .absolute()
                .left(x)
                .top(y)
                .size(px(22.))
                .rounded_full()
                .border_2()
                .border_color(p.accent)
                .bg(p.accent.opacity(fill))
        });
        Some(div().absolute().inset_0().children(marks).into_any_element())
    }
}

/// How the device stands, in a few words for the toolbar.
fn state_words(link: &Link) -> String {
    match (&link.state, &link.said) {
        (State::Off, _) => "not running".into(),
        (State::Failed(_), _) => "stopped".into(),
        (_, Some(Said::Ready { screen })) => format!("{:?}", screen.orientation).to_lowercase(),
        (_, Some(Said::Locked { .. })) => "locked".into(),
        (_, Some(Said::Starting { .. }) | None) => "starting".into(),
        (_, Some(Said::Gone { .. })) => "gone".into(),
        (_, Some(Said::Unavailable { .. })) => "needs setup".into(),
    }
}

/// The words for a start under way.
fn starting(name: &str, step: Step, since: Option<std::time::Instant>) -> String {
    let so_far = since.map_or(0, |at| at.elapsed().as_secs());
    match step {
        Step::Booting { first: true } => format!(
            // The figure and its unit never part at a line's end.
            "Starting {name} for the first time. This takes about a minute ({so_far}\u{a0}s so far)."
        ),
        Step::Booting { first: false } | Step::Picture => format!("Starting {name}…"),
        Step::Input => "Getting input ready…".into(),
    }
}
