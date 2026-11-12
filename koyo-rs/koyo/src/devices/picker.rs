//! The picker (`docs/devices.md` 9.7): Koyo's simulators, or what they miss with the setup;
//! then the iPhones and iPads plugged in, each with the step it misses. A click opens one in
//! this tab and starts it.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, ClickEvent, Context, InteractiveElement as _, IntoElement as _, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::screens::{Notice, missing, set_up};
use super::view::{Chosen, DeviceView};
use crate::theme::Palette;

const WIDTH: f32 = 320.;

impl DeviceView {
    pub(super) fn picker_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let (simulators, connected) = {
            let devices = super::devices(cx);
            let devices = devices.read(cx);
            (devices.simulators.clone(), devices.plugged().to_vec())
        };
        let missing = missing(cx);
        let choosing = (missing.is_none() && matches!(simulators, Some(Ok(_)))) || !connected.is_empty();
        let mut list = (div().absolute().inset_0().flex().flex_col().items_center().justify_center().gap_2())
            .when(choosing, |list| list.child(div().text_sm().text_color(p.muted).child("Choose a device")));
        list = match (missing, simulators) {
            (Some(missing), _) => list.child(Notice::new(missing, vec![set_up()]).block(p)),
            (None, None) => list.child(Notice::new("Loading simulators…".into(), Vec::new()).block(p)),
            (None, Some(Err(why))) => list.child(Notice::new(why, Vec::new()).block(p)),
            (None, Some(Ok(all))) => all.iter().fold(list, |list, simulator| {
                list.child(self.row(Chosen::from(simulator), &simulator.system, None, p, cx))
            }),
        };
        if !connected.is_empty() {
            let plugged = div().w(px(WIDTH)).px_3().pt_2().text_xs().text_color(p.muted).child("Connected");
            list = connected.iter().fold(list.child(plugged), |list, device| {
                let missing = device.missing.as_ref().map(|(_, said)| said.as_str());
                list.child(self.row(Chosen::from(device), &device.system, missing, p, cx))
            });
        }
        list.into_any_element()
    }

    /// A device's row: its name and system, and under them the step it misses.
    fn row(&self, chosen: Chosen, system: &str, missing: Option<&str>, p: Palette, cx: &mut Context<Self>) -> impl gpui_kit::IntoElement {
        let named = (div().flex().justify_between())
            .child(div().text_sm().text_color(p.text).child(chosen.name.clone()))
            .child(div().text_xs().text_color(p.muted).child(system.to_owned()));
        div()
            .id(SharedString::from(format!("device-pick-{}", chosen.udid)))
            .w(px(WIDTH))
            .px_3()
            .py_2()
            .rounded_md()
            .cursor_pointer()
            .hover(|row| row.bg(p.hover))
            .child(named)
            .children(missing.map(|said| div().pt_1().text_xs().text_color(p.muted).child(said.to_owned())))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.choose(chosen.clone(), cx);
                this.start_asking(window, cx);
            }))
    }
}
