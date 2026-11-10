//! The bar at the bottom of a pane the person and an agent both want (a browser page, the
//! agent desktop): who has it, and the person's one thing to do about it, in a floating
//! pill. Liquid Glass where the system has it, of the other tone than what's under it where
//! that is known (`theme::glass`).

use gpui_kit::{
    AnyElement, App, ClickEvent, Div, ElementId, FontWeight, InteractiveElement as _,
    IntoElement as _, MouseButton, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, Window, deferred, div, px,
};

use crate::native::liquid;
use crate::theme::{self, Palette};

/// What the bar's button does.
pub type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// What the bar says, and the button beside it: its id, its label and what it does.
pub struct Held {
    pub text: String,
    pub id: &'static str,
    pub label: &'static str,
    pub on_click: OnClick,
}

/// How wide the bar is at most, how far up from the pane's foot it sits, and about how high
/// it is: where what's under it is read for its glass's tone.
pub const WIDTH: f32 = 560.;
pub const BOTTOM: f32 = 14.;
pub const HEIGHT: f32 = 34.;

/// The bar, over the bottom of the pane it's a child of. `glass` keys its Liquid Glass, which
/// is `dark` or not where what's under it says so (a page), and of the theme's tone with none.
pub fn bar(
    glass: impl Into<ElementId>,
    held: Held,
    dark: Option<bool>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let (p, tint) = theme::glass::look(dark, cx);
    let glass = liquid::backdrop_toned(glass, px(999.), dark.zip(tint), window, cx);
    let button = button(held.id, held.label, glass.is_some(), p).on_click(held.on_click);
    let pill = div()
        .relative()
        .flex()
        .items_center()
        .gap_2()
        .max_w(px(WIDTH))
        .pl_3()
        .pr(px(5.))
        .py(px(5.))
        .rounded_full()
        .occlude()
        .text_xs()
        .text_color(p.text);
    let pill = liquid::floating(pill, glass, p);
    // Among the window's overlays, over its Liquid Glass, a native view of the window's.
    let bar = div()
        .absolute()
        .bottom(px(BOTTOM))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(pill.child(div().truncate().child(held.text)).child(button));
    deferred(bar).into_any_element()
}

/// The bar's button, and a notice's: a tint of the text's colour on glass, a raised fill
/// on a surface of the app's own.
pub fn button(
    id: &'static str,
    label: &'static str,
    on_glass: bool,
    p: Palette,
) -> gpui_kit::Stateful<Div> {
    let (fill, hovered) = if on_glass {
        (p.text.opacity(0.08), p.text.opacity(0.14))
    } else {
        (p.raised, p.hover)
    };
    div()
        .id(id)
        .px_2()
        .py(px(3.))
        .rounded_full()
        .cursor_pointer()
        .font_weight(FontWeight::MEDIUM)
        .bg(fill)
        .hover(move |button| button.bg(hovered))
        .child(label)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}
