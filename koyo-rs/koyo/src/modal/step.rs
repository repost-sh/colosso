//! A setup dialog's step row (the agent desktop's, Koyo's simulators'): its number in a ring,
//! a wheel while it's under way, a tick once it's there; its name, what it's for, and how it
//! stands.

use gpui_kit::assets::IconName;
use gpui_kit::{Div, FontWeight, ParentElement as _, Styled as _, div, px};

use super::spinner;
use crate::shell::icon;
use crate::theme::Palette;

/// A step row's height.
pub const STEP: f32 = 44.;

/// Where a step is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// Still to come; `dim`, its name greyed as well.
    Next { dim: bool },
    Busy,
    Done,
}

pub fn step(number: usize, mark: Mark, name: &str, why: &str, stands: &str, p: Palette) -> Div {
    let ring = div().relative().flex_none().size(px(22.)).flex().items_center().justify_center();
    let ring = match mark {
        Mark::Done => ring.text_color(p.accent).child(icon(IconName::CircleCheck, 18.)),
        Mark::Busy => ring.child(spinner(p.muted)),
        Mark::Next { .. } => (ring.rounded_full().border_1().border_color(p.hairline))
            .text_xs()
            .text_color(p.muted)
            .child(number.to_string()),
    };
    let named = if mark == (Mark::Next { dim: true }) { p.muted } else { p.text };
    div()
        .h(px(STEP))
        .flex()
        .items_center()
        .gap_3()
        .child(ring)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(div().font_weight(FontWeight::MEDIUM).text_color(named).child(name.to_owned()))
                .child(div().text_xs().text_color(p.muted).truncate().child(why.to_owned())),
        )
        .child(div().flex_none().text_xs().text_color(p.muted).child(stands.to_owned()))
}
