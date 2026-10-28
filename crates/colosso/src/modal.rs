//! Dialogs on glass. A dialog sits in a blurred window of its own over the window that opened
//! it, like the menus and the theme picker, so the desktop shows through it, tinted with the
//! palette so it follows light and dark. One is open at a time; Escape closes it, and so do a
//! press in the window behind it and focus leaving them both. Also the pieces dialogs are made
//! of.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, Size};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyView, AnyWindowHandle, App, AppContext as _, ClickEvent, Context, FontWeight, Global, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, RenderOnce, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, div, point, px, size,
};

use crate::theme::{self, Palette};

/// How far below the top of the window behind it a dialog sits.
const TOP: f32 = 72.;
/// Between a dialog's edge and what it says.
pub const PADDING: f32 = 20.;
/// The height of a dialog's [`footer`].
pub const FOOTER: f32 = 32.;
/// Every line in a dialog is one of these high, so its height is known before it opens.
pub const LINE: f32 = 20.;
pub const SMALL_LINE: f32 = 18.;

/// The dialog on screen, if any.
#[derive(Default)]
struct OpenModal(Option<AnyWindowHandle>);

impl Global for OpenModal {}

/// Opens the view `build` makes in a `width` by `height` dialog over `parent`, closing any
/// dialog already open. The dialog takes keyboard focus.
pub fn open<V: Render>(
    width: f32,
    height: f32,
    parent: &mut Window,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) {
    close(cx);
    let frame = parent.bounds();
    let origin = frame.origin
        + point(
            (frame.size.width - px(width)) / 2.,
            px(TOP).min(frame.size.height - px(height)).max(px(0.)),
        );
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(gpui_kit::Bounds::new(
            origin,
            size(px(width), px(height)),
        ))),
        titlebar: None,
        focus: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Blurred,
        display_id: parent.display(cx).map(|display| display.id()),
        ..Default::default()
    };
    let parent_handle = parent.window_handle();
    let opened = cx.open_window(options, |window, cx| {
        let content = cx.new(|cx| build(window, cx));
        cx.new(|cx| Surface {
            content: content.into(),
            _subscriptions: [
                cx.observe_window_activation(parent, move |_, _, cx| {
                    close_once_left(parent_handle, cx);
                }),
                cx.observe_window_activation(window, move |_, _, cx| {
                    close_once_left(parent_handle, cx);
                }),
                // The dialog holds focus, so Escape usually arrives in its own window.
                cx.intercept_keystrokes(|event, window, cx| {
                    if event.keystroke.key != "escape" {
                        return;
                    }
                    let open = cx.default_global::<OpenModal>().0;
                    if open == Some(window.window_handle()) {
                        dismiss(window, cx);
                    } else {
                        close(cx);
                    }
                }),
            ],
        })
    });
    if let Ok(handle) = opened {
        cx.set_global(OpenModal(Some(handle.into())));
    }
}

/// Closes the dialog, from outside it. It can't reach its own window from inside it.
pub fn close(cx: &mut App) {
    let Some(window) = cx.default_global::<OpenModal>().0.take() else {
        return;
    };
    window
        .update(cx, |_, window, _| window.remove_window())
        .ok();
}

/// Closes the dialog in `window`, from inside it.
pub fn dismiss(window: &mut Window, cx: &mut App) {
    let open = &mut cx.default_global::<OpenModal>().0;
    // Another dialog may have opened since this one did; that one stays known.
    if *open == Some(window.window_handle()) {
        *open = None;
    }
    window.remove_window();
}

/// Closes the dialog if, once focus has settled, neither it nor `parent` holds it.
fn close_once_left(parent: AnyWindowHandle, cx: &mut App) {
    cx.spawn(async move |cx| {
        // The window gaining focus only hears so after the one losing it.
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
        cx.update(|cx| {
            let open = cx.try_global::<OpenModal>().and_then(|open| open.0);
            let focused = |window: AnyWindowHandle, cx: &mut App| {
                window
                    .update(cx, |_, window, _| window.is_window_active())
                    .unwrap_or(false)
            };
            if open.is_some_and(|open| !focused(open, cx) && !focused(parent, cx)) {
                close(cx);
            }
        });
    })
    .detach();
}

/// The glass a dialog's view sits on.
struct Surface {
    content: AnyView,
    _subscriptions: [Subscription; 3],
}

impl Render for Surface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .size_full()
            .p(px(PADDING))
            .bg(p.menu)
            .text_color(p.text)
            .text_sm()
            .child(self.content.clone())
    }
}

/// Tells the user something went wrong, over `parent`.
pub fn alert(
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    parent: &mut Window,
    cx: &mut App,
) {
    let (title, message) = (title.into(), message.into());
    open(380., 170., parent, cx, |_, _| Alert { title, message });
}

struct Alert {
    title: SharedString,
    message: SharedString,
}

impl Render for Alert {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .child(heading(self.title.clone()))
            .child(
                div()
                    .flex_1()
                    .text_color(p.muted)
                    .child(self.message.clone()),
            )
            .child(
                footer(None, p).child(
                    Button::new("alert-ok", "OK", Tone::Primary)
                        .on_click(|_, window, cx| dismiss(window, cx)),
                ),
            )
    }
}

pub fn heading(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_base()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into())
}

/// The row of buttons at the bottom of a dialog, against its right edge, with what went
/// wrong, if anything, to their left.
pub fn footer(error: Option<SharedString>, p: Palette) -> gpui_kit::Div {
    div().h(px(FOOTER)).flex().items_center().gap_2().child(
        div()
            .flex_1()
            .min_w_0()
            .text_xs()
            .line_clamp(2)
            .text_color(p.danger)
            .children(error),
    )
}

/// The button that closes a dialog without doing anything.
/// A line of small text, cut short to fit.
pub fn small_line() -> gpui_kit::Div {
    div()
        .h(px(SMALL_LINE))
        .flex()
        .items_center()
        .text_xs()
        .truncate()
}

pub fn cancel(id: &'static str) -> Button {
    Button::new(id, "Cancel", Tone::Plain).on_click(|_, window, cx| dismiss(window, cx))
}

/// A text field on the glass, around an `input` that draws none of its own.
pub fn field(input: Input, p: Palette) -> impl IntoElement {
    div()
        .h(px(34.))
        // The input pads its own text, so the field only adds a little.
        .px(px(4.))
        .flex()
        .items_center()
        .rounded(px(8.))
        .border_1()
        .border_color(p.hairline)
        .bg(p.raised)
        .child(input.appearance(false).flex_1())
}

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
