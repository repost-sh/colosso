//! A device's tab (`docs/devices.md` 8): one view per pane, showing a device live with an
//! agent's touches marked over it, or what's in the way said with the one thing to do about it,
//! or the picker when no device is chosen yet. Closing the last tab of a device ends its host:
//! the simulator shuts down.

use std::time::Instant;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, App, Context, Entity, EventEmitter, FocusHandle, Global,
    InteractiveElement as _, IntoElement, KeyBinding, KeyDownEvent, KeyUpEvent, ParentElement as _,
    PinchEvent, Pixels, Point, Render, Styled as _, Subscription, Task, WeakEntity, Window, div,
};
use koyo_device_gestures::{At, Fingers, Phase};
use koyo_device_wire::Size;

use super::canvas::Canvas;
use super::link::Link;
use crate::theme;

/// A device chosen for the tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chosen {
    pub udid: String,
    pub name: String,
    pub ipad: bool,
    /// A real device by cable, not a simulator of Koyo's set.
    pub phone: bool,
}

impl Chosen {
    /// "iPhone" or "iPad", for the words that name it.
    pub fn kind(&self) -> &'static str {
        if self.ipad { "iPad" } else { "iPhone" }
    }
}

impl From<&super::connected::Connected> for Chosen {
    fn from(device: &super::connected::Connected) -> Chosen {
        Chosen {
            udid: device.udid.clone(),
            name: device.name.clone(),
            ipad: device.ipad,
            phone: true,
        }
    }
}

impl From<&super::Simulator> for Chosen {
    fn from(simulator: &super::Simulator) -> Chosen {
        Chosen {
            udid: simulator.udid.clone(),
            name: simulator.name.clone(),
            ipad: simulator.ipad,
            phone: false,
        }
    }
}

/// What the shell hears of the view: its title changed.
pub enum DeviceEvent {
    Titled(String),
}

/// A key Koyo binds, sent to the device instead while it has the keyboard.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = devices, no_json)]
pub struct PressKey(pub u16);

pub fn bind_keys(cx: &mut App) {
    let device = Some("Device");
    cx.bind_keys(
        [("tab", 43), ("shift-tab", 43), ("escape", 41), ("enter", 40), ("backspace", 42)]
            .map(|(keys, usage)| KeyBinding::new(keys, PressKey(usage), device)),
    );
    watch_trackpad(cx);
}

pub struct DeviceView {
    pub(super) device: Option<Chosen>,
    pub(super) link: Option<Entity<Link>>,
    pub(super) focus: FocusHandle,
    /// Where the picture shows in the window, and how much of its size in points.
    pub(super) placed: Option<(Point<Pixels>, f32)>,
    pub(super) fingers: Fingers,
    pub(super) pressed: bool,
    /// Where the pointer last was over the picture: the trackpad's gestures happen there.
    pub(super) pointer: Option<At>,
    pub(super) keys_down: Vec<u16>,
    pub(super) born: Instant,
    pub(super) ticking: Option<Task<()>>,
    watching: Vec<Subscription>,
}

impl EventEmitter<DeviceEvent> for DeviceView {}

impl DeviceView {
    /// A tab with no device yet: the picker.
    pub fn picker(cx: &mut Context<Self>) -> Self {
        super::devices(cx).update(cx, super::Devices::refresh);
        let watching = vec![cx.observe(&super::devices(cx), |_, _, cx| cx.notify())];
        Self::with(watching, cx)
    }

    /// A tab of `chosen`, showing that it isn't running until it's started, as a tab brought
    /// back after a restart does: a simulator is gigabytes.
    pub fn of(chosen: Chosen, cx: &mut Context<Self>) -> Self {
        let mut view = Self::with(Vec::new(), cx);
        view.choose(chosen, cx);
        // Whether the device is still there, for a tab brought back.
        super::devices(cx).update(cx, super::Devices::refresh);
        view
    }

    fn with(watching: Vec<Subscription>, cx: &mut Context<Self>) -> Self {
        cx.on_release(|view: &mut Self, cx| {
            if let Some(link) = view.link.take() {
                link.update(cx, Link::tab_gone);
            }
        })
        .detach();
        DeviceView {
            device: None,
            link: None,
            focus: cx.focus_handle(),
            placed: None,
            fingers: Fingers::new(Size {
                width: 402.,
                height: 874.,
            }),
            pressed: false,
            pointer: None,
            keys_down: Vec::new(),
            born: Instant::now(),
            ticking: None,
            watching,
        }
    }

    /// Shows `chosen` in this tab.
    pub fn choose(&mut self, chosen: Chosen, cx: &mut Context<Self>) {
        let link = super::devices(cx).update(cx, |devices, cx| devices.link(&chosen.udid, &chosen.name, chosen.phone, cx));
        let picture = link.read(cx).picture.clone();
        link.update(cx, |link, _| link.shown_in_tab());
        if let Some(before) = self.link.take() {
            before.update(cx, Link::tab_gone);
        }
        self.watching = vec![
            cx.observe(&link, |_, _, cx| cx.notify()),
            cx.observe(&picture, |_, _, cx| cx.notify()),
            // The list says whether the device is still there.
            cx.observe(&super::devices(cx), |_, _, cx| cx.notify()),
        ];
        cx.emit(DeviceEvent::Titled(chosen.name.clone()));
        self.device = Some(chosen);
        self.link = Some(link);
        cx.notify();
    }

    /// Back to the picker, the device shown let go: it was no longer there.
    pub fn choose_another(&mut self, cx: &mut Context<Self>) {
        if let Some(link) = self.link.take() {
            link.update(cx, Link::tab_gone);
        }
        self.device = None;
        super::devices(cx).update(cx, super::Devices::refresh);
        self.watching = vec![cx.observe(&super::devices(cx), |_, _, cx| cx.notify())];
        cx.emit(DeviceEvent::Titled(self.title()));
        cx.notify();
    }

    /// Starts the device shown at the person's click: one more than their limit of simulators
    /// running at once, they're asked first. A real device isn't a simulator: no limit.
    pub fn start_asking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(link), Some(chosen)) = (self.link.clone(), self.device.as_ref()) else {
            return;
        };
        if chosen.phone {
            link.update(cx, Link::start);
            return;
        }
        let running = super::devices(cx).read(cx).running_now(cx);
        let Some(others) = super::past_limit(&running, Some(&chosen.udid), super::at_once(cx)) else {
            link.update(cx, Link::start);
            return;
        };
        let question = crate::modal::Confirm {
            title: format!("Start {} too?", chosen.name),
            text: format!(
                "{}, the most you set in Settings → Devices. Each simulator running takes a share \
                 of this Mac's memory.",
                super::running_already(&others)
            ),
            yes: "Start Anyway",
            tone: crate::modal::Tone::Primary,
        };
        crate::modal::confirm(question, move |cx| link.update(cx, Link::start), window, cx);
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The tab's title: the device's name, or what the picker is.
    pub fn title(&self) -> String {
        self.device.as_ref().map_or_else(|| "Devices".to_owned(), |device| device.name.clone())
    }

    /// The device shown, for a tab kept across a restart.
    pub fn chosen(&self) -> Option<&Chosen> {
        self.device.as_ref()
    }

    #[cfg(any(test, feature = "visual-check"))]
    pub fn link(&self) -> Option<&Entity<Link>> {
        self.link.as_ref()
    }

    /// Whether it shows a simulator, which turns and locks at a button; a real device doesn't.
    pub(super) fn simulator(&self) -> bool {
        self.device.as_ref().is_some_and(|device| !device.phone)
    }

    /// "iPhone" or "iPad", for the words that name the device.
    pub(super) fn kind(&self) -> &'static str {
        self.device.as_ref().map_or("iPhone", Chosen::kind)
    }
}

impl Render for DeviceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        if let Some(screen) = self.link.as_ref().and_then(|link| link.read(cx).screen()) {
            self.fingers.set_size(screen.size);
        }
        let picture = self.link.as_ref().filter(|link| link.read(cx).screen().is_some()).cloned();
        let notice = self.notice(cx);
        // Only with the person at this window: a dialog that opens while Koyo is behind another
        // app takes the keyboard from the app they're typing in.
        if window.is_window_active() && self.may_offer_setup(cx) && super::devices(cx).read(cx).setup_due() {
            window.defer(cx, super::setup::offer);
        }
        let marks = picture.as_ref().and_then(|link| self.marks(link, cx));
        let bar = self.control_bar(window, cx);
        div()
            .id("device")
            .key_context("Device")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            // No background of its own, as a chat has none: the window's card shows round it.
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| this.key_down(event, cx)))
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| this.key_up(event, cx)))
            .on_pinch(cx.listener(|this, event: &PinchEvent, _, cx| this.pinch(event, cx)))
            .on_action(cx.listener(|this, PressKey(usage): &PressKey, _, cx| {
                let usage = *usage;
                this.send(koyo_device_wire::Input::Key { usage, down: true }, cx);
                this.send(koyo_device_wire::Input::Key { usage, down: false }, cx);
            }))
            .children(self.toolbar(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    // The bar floats over the pane's foot: the picture always keeps clear of it,
                    // so the device's screen is never under it and doesn't move when it comes.
                    .when(self.device.is_some(), |area| area.pb(gpui_kit::px(crate::held::ROOM)))
                    .when_some(picture.filter(|_| notice.as_ref().is_none_or(|n| n.over)), |area, link| {
                        area.child(Canvas {
                            view: cx.entity(),
                            link,
                            focused: self.focus.is_focused(window),
                        })
                    })
                    .children(marks)
                    .children(notice.map(|notice| notice.element(p)))
                    .children(self.lend_notice(cx))
                    .when(self.device.is_none(), |area| area.child(self.picker_list(cx))),
            )
            .children(bar)
    }
}

/// The trackpad's gestures GPUI tells no one of: heard by a monitor of the app's own events.
#[derive(Clone, Copy, Debug)]
pub enum Trackpad {
    Rotate { phase: Phase, degrees: f32 },
    SmartZoom,
}

/// The device view the pointer was last over: where a trackpad gesture goes.
struct Hovered(WeakEntity<DeviceView>);

impl Global for Hovered {}

pub(super) fn hovered(view: WeakEntity<DeviceView>, cx: &mut App) {
    // Every move over the picture says so: the global changes only when the view does.
    if cx.try_global::<Hovered>().is_none_or(|hovered| hovered.0 != view) {
        cx.set_global(Hovered(view));
    }
}

/// Installs, once, the monitor that hears the trackpad's rotation and two-finger double tap
/// (`NSEventTypeRotate`, `NSEventTypeSmartMagnify`): GPUI passes them to no one. The proof of
/// concept's own, which a played rotation of 60° reached as 60°.
fn watch_trackpad(cx: &mut App) {
    use objc2::runtime::AnyObject;
    const ROTATE: usize = 18;
    const SMART_MAGNIFY: usize = 32;
    let (tell, mut heard) = futures::channel::mpsc::unbounded::<Trackpad>();
    let handler = block2::RcBlock::new(move |event: *mut AnyObject| -> *mut AnyObject {
        // SAFETY: an `NSEvent` AppKit hands the monitor, alive for the call; `type` and `phase`
        // are unsigned integers and `rotation` a float.
        unsafe {
            let kind: usize = objc2::msg_send![event, type];
            if kind == ROTATE {
                let phase: usize = objc2::msg_send![event, phase];
                let degrees: f32 = objc2::msg_send![event, rotation];
                let phase = match phase {
                    1 | 32 => Phase::Started,
                    8 | 16 => Phase::Ended,
                    _ => Phase::Moved,
                };
                let _ = tell.unbounded_send(Trackpad::Rotate { phase, degrees });
            } else if kind == SMART_MAGNIFY {
                let _ = tell.unbounded_send(Trackpad::SmartZoom);
            }
        }
        event
    });
    let mask: u64 = (1 << ROTATE) | (1 << SMART_MAGNIFY);
    // SAFETY: a class method that copies the block and keeps it; called on the main thread.
    unsafe {
        let _: *mut AnyObject = objc2::msg_send![
            objc2::class!(NSEvent),
            addLocalMonitorForEventsMatchingMask: mask,
            handler: &*handler
        ];
    }
    cx.spawn(async move |cx| {
        use futures::StreamExt as _;
        while let Some(gesture) = heard.next().await {
            cx.update(|cx| {
                let Some(view) = cx.try_global::<Hovered>().and_then(|h| h.0.upgrade()) else {
                    return;
                };
                view.update(cx, |view, cx| view.trackpad(gesture, cx));
            });
        }
    })
    .detach();
}
