//! A dialog's buttons, rounded labels filled by their tone, and the spinner one shows while its
//! action runs.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, Size};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled, Window, div, px,
};

use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    /// The dialog's main action.
    Primary,
    /// A main action that can't be undone.
    Danger,
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A dialog's button: a rounded label filled by its tone.
#[derive(IntoElement)]
pub struct Button {
    id: SharedString,
    label: SharedString,
    tone: Tone,
    disabled: bool,
    busy: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: &'static str, label: impl Into<SharedString>, tone: Tone) -> Self {
        Button {
            id: id.into(),
            label: label.into(),
            tone,
            disabled: false,
            busy: false,
            on_click: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Spins in place of the label while its action runs, taking no more clicks.
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = theme::palette(cx);
        let (fill, text) = match self.tone {
            Tone::Plain => (p.raised, p.text),
            Tone::Primary => (p.text, p.on_text),
            Tone::Danger => (p.danger, gpui_kit::white()),
        };
        div()
            .id(self.id)
            .h(px(30.))
            .px(px(14.))
            .flex()
            .items_center()
            .rounded(px(8.))
            .bg(fill)
            .text_color(text)
            .font_weight(FontWeight::MEDIUM)
            .when(self.tone == Tone::Plain, |button| {
                button.border_1().border_color(p.hairline)
            })
            .when(self.disabled && !self.busy, |button| button.opacity(0.4))
            .when_some(
                self.on_click.filter(|_| !self.disabled && !self.busy),
                |button, handler| {
                    button
                        .cursor_pointer()
                        // The way the settings' pill buttons answer a hover, for the plain one.
                        .hover(move |style| match self.tone {
                            Tone::Plain => style.bg(p.selected),
                            _ => style.opacity(0.85),
                        })
                        .on_click(move |event, window, cx| handler(event, window, cx))
                },
            )
            .relative()
            // The label stays, unseen, so the button keeps its width.
            .child(div().when(self.busy, Styled::invisible).child(self.label))
            .when(self.busy, |button| button.child(spinner(text)))
    }
}

/// A spinner centered over its parent, which must be `relative`.
pub fn spinner(color: Hsla) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            // An open arc turning at an even speed; the default eases each turn, so it stalls.
            Spinner::new()
                .icon(IconName::LoaderCircle)
                .ease(|delta| delta)
                .with_size(Size::Small)
                .color(color),
        )
}
