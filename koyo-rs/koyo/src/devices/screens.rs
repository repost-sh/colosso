//! What a device's tab says (`docs/devices.md` 8): the state it stands in with its one way out,
//! the slim toolbar, the picker, the bar that says who has the device, and an agent's touches.
//! Every state has a way out or moves on by itself: a dead end is what angered the owner most.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Entity, IntoElement, ParentElement as _, Styled as _, Window, div, px,
};
use koyo_device_wire::{Mark, Missing, Step};

use super::link::Link;
use super::view::DeviceView;
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
    /// Under way: a loader over what it says.
    busy: bool,
    /// A quieter line under what it says.
    hint: Option<String>,
}

impl Notice {
    pub(super) fn new(message: String, buttons: Vec<Push>) -> Notice {
        Notice {
            message,
            buttons,
            over: false,
            busy: false,
            hint: None,
        }
    }

    /// Something under way: a loader, what it is, and a quieter line under it.
    fn busy(message: String, hint: Option<String>) -> Notice {
        Notice {
            busy: true,
            hint,
            ..Notice::new(message, Vec::new())
        }
    }

    pub fn element(self, p: Palette) -> AnyElement {
        let over = self.over;
        let block = self.block(p);
        if over {
            div().absolute().bottom(px(64.)).left_0().right_0().flex().justify_center().child(block).into_any_element()
        } else {
            div().absolute().inset_0().flex().items_center().justify_center().child(block).into_any_element()
        }
    }

    /// What it says and its buttons, in a column, wherever it's put; under way, a loader first.
    pub(super) fn block(self, p: Palette) -> gpui_kit::Div {
        use gpui_kit::component::spinner::Spinner;
        use gpui_kit::component::{Sizable as _, Size};
        let loader = self.busy.then(|| {
            // An open arc turning at an even speed, as the app's other waits.
            let wheel = Spinner::new().icon(gpui_kit::assets::IconName::LoaderCircle).ease(|delta| delta);
            wheel.with_size(Size::Size(px(24.))).color(p.muted)
        });
        let hint = self.hint.map(|hint| div().text_xs().text_color(p.muted).child(hint));
        (div().flex().flex_col().items_center().gap_3())
            .children(loader)
            .child(
                (div().flex().flex_col().items_center().gap_1())
                    .child(div().max_w(px(460.)).text_center().text_sm().text_color(p.muted).child(self.message))
                    .children(hint),
            )
            .child(div().flex().gap_2().children(self.buttons))
    }
}

impl DeviceView {
    fn start_button(&self, label: &'static str, cx: &mut Context<Self>) -> Push {
        let start = cx.listener(|this, _: &ClickEvent, window, cx| this.start_asking(window, cx));
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
        let standing = Owned::from(link.read(cx).standing());
        let name = &chosen.name;
        let listed = super::devices(cx).read(cx).simulators.as_ref().and_then(|listed| listed.as_ref().ok());
        let gone = listed.is_some_and(|listed| listed.iter().all(|simulator| simulator.udid != chosen.udid));
        if !chosen.phone && standing == Owned::Off && gone {
            return Some(self.not_there(name, cx));
        }
        match standing {
            // Unplugged, in its host's words: back on its cable it starts again by itself; Start
            // Again is there should the plug go unheard.
            Owned::Ended(None, why) if chosen.phone => {
                let again = cx.listener(|this, _: &ClickEvent, window, cx| this.start_asking(window, cx));
                Some(Notice::new(why, vec![Push::new("device-start", "Start Again", Tone::Plain).on_click(again)]))
            }
            Owned::Ended(None, why) => Some(Notice::new(
                format!("{name} isn't there: {why}."),
                vec![self.start_button("Start Again", cx)],
            )),
            Owned::Ended(Some(code), why) => Some(self.unavailable(code, why, cx)),
            Owned::Off => Some(Notice::new(format!("{name} isn't running."), vec![self.start_button("Start", cx)])),
            Owned::Failed(why) => Some(Notice::new(why, vec![self.start_button("Start Again", cx)])),
            Owned::Starting(None) => Some(Notice::busy(format!("Starting {name}"), None)),
            Owned::Starting(Some(step)) => Some(starting_notice(name, step)),
            Owned::Ready => None,
            Owned::Locked => Some(Notice {
                over: true,
                ..Notice::new(format!("Unlock your {} to continue.", self.kind()), Vec::new())
            }),
        }
    }

    /// What's missing, in its host's words, with the way on: the setup for Xcode, or for a step
    /// the person takes on a real device (it starts again by itself once that's done); nothing
    /// for an update, which is theirs on the device; Start Again for the rest.
    fn unavailable(&self, code: Missing, why: String, cx: &mut Context<Self>) -> Notice {
        let udid = self.device.as_ref().map(|device| device.udid.clone());
        match code {
            Missing::Simulator => self.not_there(&self.title(), cx),
            // The one thing missing, with the setup; once it's there, Start Again.
            Missing::Xcode => match missing(cx) {
                Some(said) => Notice::new(said, vec![set_up()]),
                None => Notice::new(format!("{why}."), vec![self.start_button("Start Again", cx)]),
            },
            Missing::System => Notice::new(why, Vec::new()),
            Missing::Cable | Missing::Trust | Missing::DeveloperMode => Notice::new(why, vec![set_up_device(udid)]),
            _ => Notice::new(why, vec![self.start_button("Start Again", cx)]),
        }
    }

    /// The bar over the picture, as the browser's over a page: the device, a dot and a word for
    /// how it stands; then its buttons, the system's (Home, the app switcher) and the device's
    /// own (turning, the lock), each an icon named by its tooltip with its key.
    pub(super) fn toolbar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use gpui_kit::assets::IconName;
        use gpui_kit::component::{Disableable as _, Sizable as _};
        use koyo_device_wire::Button;

        let p = crate::theme::palette(cx);
        let chosen = self.device.as_ref()?;
        let (ready, held, (words, dot)) = {
            let link = self.link.as_ref()?.read(cx);
            let held = matches!(link.standing(), super::Standing::Ready(_) | super::Standing::Locked(_));
            (link.ready(), held, standing(link, p))
        };
        let simulator = self.simulator();
        // What holding the stream costs, the person's to know while it's held (`docs/devices.md` 5).
        let awake = (chosen.phone && held).then(|| {
            format!(
                "While this tab is open, your {} stays awake and its camera and microphone are off.",
                self.kind()
            )
        });
        let glyph = if chosen.ipad { IconName::Tablet } else { IconName::Smartphone };
        let button = |id: &'static str, glyph: IconName, name: &'static str, cx: &mut Context<Self>| {
            crate::shell::icon_button(id, glyph, name, cx).small().disabled(!ready)
        };
        let rule = || div().w(px(1.)).h(px(16.)).mx_1().bg(p.hairline);
        let home = button("device-home", IconName::House, "Home (⇧⌘H)", cx)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.press_button(Button::Home, cx)));
        let switcher = button("device-switcher", IconName::SquareStack, "App Switcher", cx)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.app_switcher(cx)));
        let left = button("device-rotate-left", IconName::RotateCcwSquare, "Rotate Left (⌘←)", cx)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.turn(true, cx)));
        let right = button("device-rotate-right", IconName::RotateCwSquare, "Rotate Right (⌘→)", cx)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.turn(false, cx)));
        let lock = button("device-lock", IconName::Lock, "Lock", cx)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.press_button(Button::Lock, cx)));
        // A real device not lent yet: the person may lend it ahead of an agent's asking.
        let lendable = chosen.phone && !super::devices(cx).read(cx).lends.is_lent(&chosen.udid);
        let lend = lendable.then(|| {
            let named = format!("Let Agents Use This {}", chosen.kind());
            crate::shell::icon_button("device-lend", IconName::Bot, named, cx)
                .small()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.lend_ahead(cx)))
        });
        let named = div()
            .flex()
            .items_center()
            .gap_2()
            .pl_1()
            .min_w_0()
            .child(crate::shell::icon(glyph, 14.).text_color(p.muted))
            .child(div().text_sm().text_color(p.text).truncate().child(chosen.name.clone()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_xs()
                    .text_color(p.muted)
                    .child(div().size(px(6.)).rounded_full().bg(dot))
                    .child(words),
            );
        Some(
            div()
                .h(px(40.))
                .px_2()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(2.))
                .border_b_1()
                .border_color(p.hairline)
                .child(named)
                .child(match awake {
                    Some(said) => div().flex_1().min_w_0().ml_3().text_xs().text_color(p.muted).truncate().child(said),
                    None => div().flex_1(),
                })
                .child(home)
                .child(switcher)
                .when(simulator, |bar| bar.child(rule()).child(left).child(right).child(lock))
                .when_some(lend, |bar, lend| bar.child(rule()).child(lend))
                .into_any_element(),
        )
    }

    /// Whether this tab brings the setup up by itself when something is missing: the picker,
    /// and a tab the person opened that can't start for want of Xcode. Never a tab an agent
    /// opened.
    pub(super) fn may_offer_setup(&self, cx: &Context<Self>) -> bool {
        let Some(link) = &self.link else {
            return self.device.is_none();
        };
        let link = link.read(cx);
        matches!(link.standing(), super::Standing::Ended { missing: Some(Missing::Xcode), .. })
            && !matches!(link.control.holder, Some(Holder::Agent { .. }))
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

/// What Koyo's simulators miss, in a sentence, once it's known.
pub(super) fn missing(cx: &gpui_kit::App) -> Option<String> {
    let devices = super::devices(cx);
    let devices = devices.read(cx);
    Some(devices.found.as_ref()?.sentence(devices.need()?))
}

/// The button that opens the setup.
pub(super) fn set_up() -> Push {
    Push::new("device-set-up", "Set Up…", Tone::Primary).on_click(|_, window, cx| super::setup::open(window, cx))
}

/// The button that opens a real device's setup, at `udid`'s steps.
fn set_up_device(udid: Option<String>) -> Push {
    Push::new("device-set-up", "Set Up…", Tone::Primary)
        .on_click(move |_, window, cx| super::setup::device::open(udid.clone(), window, cx))
}

/// A device's standing, owned, for a notice built while the view is borrowed.
#[derive(Clone, Debug, PartialEq)]
enum Owned {
    Ended(Option<Missing>, String),
    Off,
    Failed(String),
    Starting(Option<Step>),
    Ready,
    Locked,
}

impl From<super::Standing<'_>> for Owned {
    fn from(standing: super::Standing<'_>) -> Owned {
        use super::Standing;
        match standing {
            Standing::Ended { missing, why } => Owned::Ended(missing, why.to_owned()),
            Standing::Off => Owned::Off,
            Standing::Failed(why) => Owned::Failed(why.to_owned()),
            Standing::Starting(step) => Owned::Starting(step),
            Standing::Ready(_) => Owned::Ready,
            Standing::Locked(_) => Owned::Locked,
        }
    }
}

/// How the device stands, in a word for the toolbar, and its dot's colour: green ready, amber
/// on its way, red what needs doing, grey not running.
fn standing(link: &Link, p: Palette) -> (&'static str, gpui_kit::Hsla) {
    match link.standing() {
        super::Standing::Ended { missing: None, .. } if link.phone => ("Unplugged", p.danger),
        super::Standing::Ended { missing: None, .. } => ("Gone", p.danger),
        super::Standing::Ended { .. } => ("Needs setup", p.danger),
        super::Standing::Off => ("Not running", p.muted),
        super::Standing::Failed(_) => ("Stopped", p.danger),
        super::Standing::Ready(_) => ("Ready", p.added),
        super::Standing::Locked(_) => ("Locked", p.modified),
        super::Standing::Starting(_) => ("Starting", p.modified),
    }
}

/// A start under way, as the tab shows it: a loader, what it is, and on a first boot how long
/// that takes.
fn starting_notice(name: &str, step: Step) -> Notice {
    match step {
        Step::Booting { first: true } => Notice::busy(format!("Starting {name}"), Some("The first start takes about a minute.".into())),
        Step::Booting { first: false } | Step::Picture => Notice::busy(format!("Starting {name}"), None),
        Step::Input => Notice::busy("Getting input ready".into(), None),
    }
}

/// The words for a start under way, as the setup says it.
pub(super) fn starting(name: &str, step: Step, since: Option<std::time::Instant>) -> String {
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
