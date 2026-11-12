//! A device's bar (`docs/devices.md` 8): a dot and a word for how it stands, then its buttons,
//! the system's (Home, the app switcher) and the device's own (turning, the lock), each an icon
//! named by its tooltip with its key. It goes in the pane's own bar after the device's name, so
//! a device has one bar and not two. A view of its own: drawn again when the device's standing
//! changes, not with every picture.

use gpui_kit::assets::IconName;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Render, Styled as _,
    Subscription, WeakEntity, Window, div, px,
};
use koyo_device_wire::Button;

use super::link::Link;
use super::view::DeviceView;

pub struct DeviceBar {
    view: WeakEntity<DeviceView>,
    _watching: [Subscription; 2],
}

impl DeviceBar {
    pub(super) fn new(view: WeakEntity<DeviceView>, link: &Entity<Link>, cx: &mut Context<Self>) -> Self {
        let watching = [
            cx.observe(link, |_, _, cx| cx.notify()),
            // Whether it's lent, and whether it's still there.
            cx.observe(&super::devices(cx), |_, _, cx| cx.notify()),
        ];
        DeviceBar { view, _watching: watching }
    }
}

/// A click's doing, on the device's view.
fn does<Act: Fn(&mut DeviceView, &mut Context<DeviceView>) + 'static>(
    view: &WeakEntity<DeviceView>,
    act: Act,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static + use<Act> {
    let view = view.clone();
    move |_, _, cx| {
        let _ = view.update(cx, |view, cx| act(view, cx));
    }
}

impl Render for DeviceBar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = crate::theme::palette(cx);
        let bar = div().flex_1().min_w_0().flex().items_center().gap(px(2.));
        let Some(view) = self.view.upgrade() else {
            return bar;
        };
        let shown = view.read(cx);
        let (Some(chosen), Some(link)) = (shown.device.clone(), shown.link.as_ref()) else {
            return bar;
        };
        let (simulator, kind) = (shown.simulator(), shown.kind());
        let (ready, held, (words, dot)) = {
            let link = link.read(cx);
            let held = matches!(link.standing(), super::Standing::Ready(_) | super::Standing::Locked(_));
            (link.ready(), held, super::screens::standing(link, p))
        };
        // What holding the stream costs, the person's to know while it's held (`docs/devices.md` 5).
        let awake = (chosen.phone && held)
            .then(|| format!("While this tab is open, your {kind} stays awake and its camera and microphone are off."));
        let button = |id: &'static str, glyph: IconName, name: &'static str| {
            crate::shell::icon_button(id, glyph, name, cx).xsmall().disabled(!ready)
        };
        let rule = || div().w(px(1.)).h(px(14.)).mx_1().bg(p.hairline);
        let home = button("device-home", IconName::House, "Home (⇧⌘H)").on_click(does(&self.view, |view, cx| view.press_button(Button::Home, cx)));
        let switcher = button("device-switcher", IconName::SquareStack, "App Switcher").on_click(does(&self.view, DeviceView::app_switcher));
        let left = button("device-rotate-left", IconName::RotateCcwSquare, "Rotate Left (⌘←)").on_click(does(&self.view, |view, cx| view.turn(true, cx)));
        let right = button("device-rotate-right", IconName::RotateCwSquare, "Rotate Right (⌘→)").on_click(does(&self.view, |view, cx| view.turn(false, cx)));
        let lock = button("device-lock", IconName::Lock, "Lock").on_click(does(&self.view, |view, cx| view.press_button(Button::Lock, cx)));
        // A real device not lent yet: the person may lend it ahead of an agent's asking.
        let lendable = chosen.phone && !super::devices(cx).read(cx).lends.is_lent(&chosen.udid);
        let lend = lendable.then(|| {
            let named = format!("Let Agents Use This {}", chosen.kind());
            (crate::shell::icon_button("device-lend", IconName::Bot, named, cx).xsmall()).on_click(does(&self.view, DeviceView::lend_ahead))
        });
        let stands = (div().flex().items_center().gap(px(5.)).flex_shrink_0())
            .child(div().size(px(6.)).rounded_full().bg(dot))
            .child(words);
        bar.child(stands)
            .child(match awake {
                Some(said) => div().flex_1().min_w_0().ml_2().truncate().child(said),
                None => div().flex_1(),
            })
            .child(
                // In a split's bar a press drags the pane: not from its buttons.
                (div().flex().items_center().gap(px(2.)).flex_shrink_0())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(home)
                    .child(switcher)
                    .when(simulator, |buttons| buttons.child(rule()).child(left).child(right).child(lock))
                    .when_some(lend, |buttons, lend| buttons.child(rule()).child(lend)),
            )
    }
}
