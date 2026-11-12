//! A device's bar (`docs/devices.md` 8): a dot and a word for how it stands, then its buttons,
//! the system's (Home, the app switcher) and the device's own (turning, the lock), each an icon
//! named by its tooltip with its key. It goes in the pane's own bar after the device's name, so
//! a device has one bar and not two. A view of its own, which reads the device's link and never
//! its view: drawn again when the device's standing changes, not with every picture.

use gpui_kit::assets::IconName;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    ClickEvent, Context, Entity, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render, Styled as _,
    Subscription, WeakEntity, Window, div, px,
};
use koyo_device_wire::Button;

use super::link::Link;
use super::view::{Chosen, DeviceView};

/// What a click on one of its buttons does, on the device's view.
type Does = fn(&mut DeviceView, &mut Context<DeviceView>);

pub struct DeviceBar {
    /// For its buttons' clicks only: read as it draws, the bar would draw with each picture.
    view: WeakEntity<DeviceView>,
    chosen: Chosen,
    link: Entity<Link>,
    _watching: [Subscription; 2],
}

impl DeviceBar {
    pub(super) fn new(view: WeakEntity<DeviceView>, chosen: Chosen, link: Entity<Link>, cx: &mut Context<Self>) -> Self {
        let watching = [
            cx.observe(&link, |_, _, cx| cx.notify()),
            // Whether it's lent, and whether it's still there.
            cx.observe(&super::devices(cx), |_, _, cx| cx.notify()),
        ];
        DeviceBar {
            view,
            chosen,
            link,
            _watching: watching,
        }
    }
}

impl Render for DeviceBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = crate::theme::palette(cx);
        // As wide as the room the pane's bar gives it: a width of its own to be laid out at,
        // whichever pane or split it's drawn in next.
        let bar = div().w_full().flex().items_center().gap(px(2.));
        let Some(view) = self.view.upgrade() else {
            return bar;
        };
        let (chosen, kind) = (&self.chosen, self.chosen.kind());
        let (ready, streaming, (words, dot)) = {
            let link = self.link.read(cx);
            let streaming = matches!(link.standing(), super::Standing::Ready(_) | super::Standing::Locked(_));
            (link.ready(), streaming, super::screens::standing(link, p))
        };
        // What holding the stream costs, the person's to know while it's held (`docs/devices.md` 5).
        let awake = (chosen.phone && streaming)
            .then(|| format!("While this tab is open, your {kind} stays awake and its camera and microphone are off."));
        let button = |id: &'static str, glyph: IconName, name: &'static str, does: Does| {
            let click = window.listener_for(&view, move |view, _: &ClickEvent, _, cx| does(view, cx));
            crate::shell::icon_button(id, glyph, name, cx).xsmall().disabled(!ready).on_click(click)
        };
        let rule = || div().w(px(1.)).h(px(14.)).mx_1().bg(p.hairline);
        // A real device not lent yet: the person may lend it ahead of an agent's asking.
        let lendable = chosen.phone && !super::devices(cx).read(cx).lends.is_lent(&chosen.udid);
        let lend = lendable.then(|| {
            let click = window.listener_for(&view, |view, _: &ClickEvent, _, cx| view.lend_ahead(cx));
            (crate::shell::icon_button("device-lend", IconName::Bot, format!("Let Agents Use This {kind}"), cx).xsmall()).on_click(click)
        });
        let stands = (div().flex().items_center().gap(px(5.)).flex_shrink_0()).child(crate::pane::dot(dot)).child(words);
        bar.child(stands)
            .child(match awake {
                Some(said) => div().flex_1().min_w_0().ml_2().truncate().child(said),
                None => div().flex_1(),
            })
            .child(
                // In a split's bar a press drags the pane: not from its buttons.
                (div().flex().items_center().gap(px(2.)).flex_shrink_0())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(button("device-home", IconName::House, "Home (⇧⌘H)", |view, cx| view.press_button(Button::Home, cx)))
                    .child(button("device-switcher", IconName::SquareStack, "App Switcher", DeviceView::app_switcher))
                    .when(!chosen.phone, |buttons| {
                        (buttons.child(rule()))
                            .child(button("device-rotate-left", IconName::RotateCcwSquare, "Rotate Left (⌘←)", |view, cx| view.turn(true, cx)))
                            .child(button("device-rotate-right", IconName::RotateCwSquare, "Rotate Right (⌘→)", |view, cx| view.turn(false, cx)))
                            .child(button("device-lock", IconName::Lock, "Lock", |view, cx| view.press_button(Button::Lock, cx)))
                    })
                    .when_some(lend, |buttons, lend| buttons.child(rule()).child(lend)),
            )
    }
}
