//! The device bar over content in responsive mode, in the browser toolbar's look: the preset
//! menu (`Responsive ▾` for a free size), the width and
//! height (editable for a free size: Enter applies, Escape restores), the zoom menu, rotate
//! for a preset, whatever the pane adds (a page's reload offer), and ✕. A pane gives it its
//! presets' names, the actions their rows and the zoom's rows dispatch, and what its buttons
//! and fields do.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::component::{Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyElement, App, AppContext as _, ClickEvent, Context, Div, ElementId, Entity,
    Focusable as _, InteractiveElement as _, KeyDownEvent, MouseButton, MouseDownEvent,
    ParentElement as _, SharedString, Stateful, Styled as _, Subscription, Window, div, px,
};

use super::Zoom;
use crate::shell::{glass, icon, icon_button};
use crate::theme::{self, Palette};

/// The bar's height.
pub const HEIGHT: f32 = 32.;
/// A size field's width: 16384, the most a side can be, shows whole.
const FIELD_WIDTH: f32 = 68.;

/// What a press of one of the bar's buttons does.
pub type Click = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// What Escape in a field does: the content's size shown again.
pub type Restore = Rc<dyn Fn(&mut Window, &mut App)>;

/// Responsive mode's toggle in a pane's toolbar, the browser's and a terminal's: selected
/// while the mode is `on`, its tooltip showing the key `action` is bound to in the pane's
/// key `context`.
pub fn toggle_button(
    id: impl Into<ElementId>,
    on: bool,
    action: &dyn Action,
    context: &str,
    cx: &App,
) -> Button {
    (glass(id, theme::palette(cx).text, cx).icon(IconName::TabletSmartphone))
        .tooltip_with_action("Responsive", action, Some(context))
        .small()
        .selected(on)
}

/// The width and height fields.
pub struct Fields {
    width: Entity<InputState>,
    height: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl Fields {
    /// Fields in `V`'s window, Enter in either calling `typed`.
    pub fn new<V: 'static>(
        typed: fn(&mut V, &mut Window, &mut Context<V>),
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Self {
        let field =
            |window: &mut Window, cx: &mut Context<V>| cx.new(|cx| InputState::new(window, cx));
        let (width, height) = (field(window, cx), field(window, cx));
        let heard = |field: &Entity<InputState>, window: &mut Window, cx: &mut Context<V>| {
            cx.subscribe_in(
                field,
                window,
                move |this, field, event, window, cx| match event {
                    InputEvent::PressEnter { .. } => typed(this, window, cx),
                    // Left without Enter, a field shows the content's size again, from its start.
                    InputEvent::Blur => {
                        shown_from_start(field, window, cx);
                        cx.notify();
                    }
                    InputEvent::Change | InputEvent::Focus => {}
                },
            )
        };
        let subscriptions = vec![heard(&width, window, cx), heard(&height, window, cx)];
        Fields {
            width,
            height,
            _subscriptions: subscriptions,
        }
    }

    /// Shows `width` × `height` in the fields, those being edited only when `restore`, which
    /// shows them from their start again too: typing a number scrolls a narrow field.
    pub fn show(&self, width: u32, height: u32, restore: bool, window: &mut Window, cx: &mut App) {
        for (field, side) in [(&self.width, width), (&self.height, height)] {
            let editing = field.read(cx).focus_handle(cx).is_focused(window);
            let shown = side.to_string();
            if restore || (!editing && field.read(cx).value().as_ref() != shown) {
                field.update(cx, |field, cx| field.set_value(shown, window, cx));
            }
        }
    }

    /// The size typed, when both fields hold a number.
    pub fn typed(&self, cx: &App) -> Option<(u32, u32)> {
        let typed = |field: &Entity<InputState>| field.read(cx).value().trim().parse::<u32>().ok();
        Some((typed(&self.width)?, typed(&self.height)?))
    }
}

/// `field` showing its text from the start: setting a value scrolls it back.
fn shown_from_start(field: &Entity<InputState>, window: &mut Window, cx: &mut App) {
    field.update(cx, |field, cx| {
        let value = field.value();
        field.set_value(value, window, cx);
    });
}

/// What the bar shows, and what its parts do.
pub struct Bar<'a> {
    /// What its elements' ids start with: `browser`, `terminal`.
    pub ids: &'static str,
    /// The presets' names, the one shown (`None` for a free size), and the action a row
    /// dispatches: a preset by its place, or `None` for a free size.
    pub presets: Vec<SharedString>,
    pub preset: Option<usize>,
    pub pick_preset: Rc<dyn Fn(Option<usize>) -> Box<dyn Action>>,
    pub fields: &'a Fields,
    /// The zoom, the percent the content shows at, and the action a row dispatches.
    pub zoom: Zoom,
    pub shown_percent: u32,
    pub pick_zoom: Rc<dyn Fn(Zoom) -> Box<dyn Action>>,
    pub restore: Restore,
    /// Rotate, offered for a preset.
    pub rotate: Click,
    /// What the pane adds after rotate.
    pub extra: Option<AnyElement>,
    /// ✕, which ends the mode.
    pub close: Click,
}

impl Bar<'_> {
    /// The bar.
    pub fn render(self, cx: &App) -> Div {
        let p = theme::palette(cx);
        let free = self.preset.is_none();
        let id = |part: &str| ElementId::Name(format!("{}-{part}", self.ids).into());
        let shown = self.preset.and_then(|at| self.presets.get(at).cloned());
        let device: SharedString = shown.unwrap_or_else(|| "Responsive".into());
        let (presets, preset, pick) = (self.presets, self.preset, self.pick_preset);
        let device_menu = move || {
            let menu = NativeMenu::new()
                .menu_with_check("Responsive", preset.is_none(), pick(None))
                .separator();
            (presets.iter().enumerate()).fold(menu, |menu, (at, name)| {
                menu.menu_with_check(name.clone(), preset == Some(at), pick(Some(at)))
            })
        };
        let (current, pick_zoom) = (self.zoom, self.pick_zoom);
        let zoom_menu = move || {
            Zoom::ALL.into_iter().fold(NativeMenu::new(), |menu, zoom| {
                menu.menu_with_check(zoom.row(), zoom == current, pick_zoom(zoom))
            })
        };
        let rotate = self.rotate;
        let close = self.close;
        div()
            .h(px(HEIGHT))
            .flex_shrink_0()
            .w_full()
            .px_2()
            .flex()
            .items_center()
            .gap(px(6.))
            .border_b_1()
            .border_color(p.hairline)
            .text_xs()
            .child(menu_chip(id("device-menu"), device, p, device_menu))
            .child(field(&self.fields.width, free, p, &self.restore))
            .child(div().text_color(p.muted).child("×"))
            .child(field(&self.fields.height, free, p, &self.restore))
            .child(menu_chip(
                id("zoom-menu"),
                self.zoom.label(self.shown_percent).into(),
                p,
                zoom_menu,
            ))
            .when(!free, |bar| {
                bar.child(
                    icon_button(id("rotate"), IconName::RotateCw, "Rotate", cx)
                        .xsmall()
                        .on_click(move |event, window, cx| rotate(event, window, cx)),
                )
            })
            .children(self.extra)
            .child(div().flex_1())
            .child(
                icon_button(
                    id("responsive-close"),
                    IconName::X,
                    "Close responsive mode",
                    cx,
                )
                .xsmall()
                .on_click(move |event, window, cx| close(event, window, cx)),
            )
    }
}

/// A chip whose press shows the menu `menu` makes.
fn menu_chip(
    id: ElementId,
    label: SharedString,
    p: Palette,
    menu: impl Fn() -> NativeMenu + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(22.))
        .px_2()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_1()
        .rounded(px(6.))
        .cursor_pointer()
        .text_color(p.text)
        .hover(|chip| chip.bg(p.hover))
        .child(label)
        .child(icon(IconName::ChevronDown, 12.).text_color(p.muted))
        .on_mouse_down(
            MouseButton::Left,
            move |event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                menu().show(event.position, window, cx);
            },
        )
}

/// A number field of the size, wide enough for five digits and never narrower; Escape puts
/// the content's number back.
fn field(state: &Entity<InputState>, enabled: bool, p: Palette, restore: &Restore) -> Div {
    let restore = Rc::clone(restore);
    div()
        .w(px(FIELD_WIDTH))
        .flex_shrink_0()
        .h(px(22.))
        .px_1()
        .flex()
        .items_center()
        .rounded(px(6.))
        .bg(p.raised)
        .border_1()
        .border_color(p.hairline)
        .capture_key_down(move |event: &KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" {
                restore(window, cx);
                cx.stop_propagation();
            }
        })
        .child(
            Input::new(state)
                .appearance(false)
                .small()
                .disabled(!enabled),
        )
}
