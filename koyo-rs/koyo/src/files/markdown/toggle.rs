//! The toggle floating at the top right of a markdown file's tab: two segments, Code and
//! Preview, on Liquid Glass where the system has it, the one shown under a thumb that slides
//! across when the other's picked.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, ClickEvent, Context, ElementId, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, Window, deferred, div, px,
};

use super::Mode;
use crate::files::{FilePane, TogglePreview};
use crate::native::liquid;
use crate::shell::icon;
use crate::theme::{Palette, gray};

/// How tall the toggle is, the room between it and its thumb, and what it keeps from the
/// tab's top and right edges.
const HEIGHT: f32 = 32.;
const INSET: f32 = 3.;
const TOP: f32 = 10.;
const RIGHT: f32 = 16.;
/// Each segment's width: enough for its mark and its name.
const CODE: f32 = 66.;
const PREVIEW: f32 = 84.;
/// How far down the tab the toggle reaches, for what's under it to start below.
pub const BELOW: f32 = TOP + HEIGHT;
/// How long the thumb takes to slide across.
const SLIDE: Duration = Duration::from_millis(320);

/// The toggle for a tab showing its markdown file in `mode`, picked `flips` times: its thumb
/// slides across each time, not when it's first drawn.
pub fn toggle(mode: Mode, flips: usize, window: &mut Window, cx: &mut Context<FilePane>) -> AnyElement {
    let p = crate::theme::palette(cx);
    let id = cx.entity_id().as_u64();
    let glass_id = ElementId::NamedInteger("markdown-toggle-glass".into(), id);
    let glass = liquid::backdrop(glass_id, px(HEIGHT / 2.), window, cx);
    let on_glass = glass.is_some();
    let dark = cx.theme().mode.is_dark();
    let thumb_fill = match (on_glass, dark) {
        (true, true) => gray(1., 0.16),
        (true, false) => gray(1., 0.92),
        (false, _) => p.selected,
    };
    let (from, to) = (place(mode.flipped()), place(mode));
    let thumb = div()
        .absolute()
        .top(px(INSET))
        .bottom(px(INSET))
        .left(px(to.0))
        .w(px(to.1))
        .rounded_full()
        .bg(thumb_fill)
        .when(on_glass && !dark, gpui_kit::Styled::shadow_sm);
    let thumb = if flips == 0 || cx.reduce_motion() {
        thumb.into_any_element()
    } else {
        let slide = Animation::new(SLIDE).with_easing(settle);
        let key = ElementId::NamedInteger("markdown-toggle-thumb".into(), flips as u64);
        (thumb.with_animation(key, slide, move |thumb, t| {
            thumb.left(px(from.0 + (to.0 - from.0) * t)).w(px(from.1 + (to.1 - from.1) * t))
        }))
        .into_any_element()
    };
    let pill = div()
        .id(("markdown-toggle", id))
        .relative()
        .flex()
        .items_center()
        .h(px(HEIGHT))
        .px(px(INSET))
        .rounded_full()
        .occlude()
        .text_xs()
        .font_weight(FontWeight::MEDIUM);
    let pill = liquid::floating(pill, glass, p)
        .child(thumb)
        .child(segment(Mode::Code, mode, p, cx))
        .child(segment(Mode::Preview, mode, p, cx));
    deferred(div().absolute().top(px(TOP)).right(px(RIGHT)).child(pill)).into_any_element()
}

/// The segment that shows the file in `mode`, the one shown being `shown`.
fn segment(mode: Mode, shown: Mode, p: Palette, cx: &mut Context<FilePane>) -> impl IntoElement + use<> {
    let (mark, id) = match mode {
        Mode::Code => (IconName::CodeXml, "markdown-toggle-code"),
        Mode::Preview => (IconName::Eye, "markdown-toggle-preview"),
    };
    let name = mode.label();
    let width = place(mode).1;
    let on = mode == shown;
    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(5.))
        .w(px(width))
        .h_full()
        .rounded_full()
        .cursor_pointer()
        .text_color(if on { p.text } else { p.muted })
        .when(!on, |segment| segment.hover(move |segment| segment.text_color(p.text)))
        .child(icon(mark, 13.))
        .child(name)
        .tooltip(move |window, cx| Tooltip::new(name).action(&TogglePreview, None).build(window, cx))
        .on_click(cx.listener(move |pane, _: &ClickEvent, window, cx| {
            pane.show_as(mode, window, cx);
        }))
}

/// Where the thumb sits over the segment for `mode`: its left edge and its width.
fn place(mode: Mode) -> (f32, f32) {
    match mode {
        Mode::Code => (INSET, CODE),
        Mode::Preview => (INSET + CODE, PREVIEW),
    }
}

/// Out fast, past where it's going by a touch, and back: a thumb with some give to it.
fn settle(t: f32) -> f32 {
    const GIVE: f32 = 1.2;
    let t = t - 1.;
    1. + (GIVE + 1.) * t * t * t + GIVE * t * t
}
