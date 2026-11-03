//! Dialogs on glass. A dialog sits in a blurred window of its own over the window that opened
//! it, like the menus and the theme picker, so the desktop shows through it, tinted with the
//! palette so it follows light and dark. One is open at a time; Escape closes it, and so do a
//! press in the window behind it and focus leaving them both. Also the pieces dialogs are made
//! of.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, Size};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyView, AnyWindowHandle, App, AppContext as _, ClickEvent, Context, Entity,
    FontWeight, Global, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled, Subscription, TextStyle,
    Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div, point, px,
    size,
};

use crate::shell::icon;
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
/// A [`heading`]'s text is this big, each of its lines this high, and it has at most this
/// many: a long file name wraps it, a silly one is cut short.
const HEADING_SIZE: f32 = 16.;
const HEADING_LINE: f32 = 24.;
const MOST_HEADING_LINES: usize = 3;

/// The height of a dialog's text [`field`].
pub const FIELD: f32 = 34.;
/// Between the pieces of a [`palette`].
const GAP: f32 = 8.;
/// The width of a palette, the most rows it shows at once, and the room round its pieces:
/// less than a dialog's, a palette being a list to scan rather than a question to read.
const PALETTE_WIDTH: f32 = 640.;
const PALETTE_ROWS: usize = 14;
const PALETTE_PADDING: f32 = 12.;

/// Opens the palette `build` makes over `parent`: a dialog a [`palette`] with a field and a
/// status line fills, sized to show the most rows.
pub fn open_palette<V: Render>(
    parent: &mut Window,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) {
    let height = palette_height(true, true, PALETTE_ROWS);
    open_padded(PALETTE_WIDTH, height, PALETTE_PADDING, parent, cx, build);
}

/// Opens the list `build` makes over `parent`: a dialog a [`palette`] of rows alone fills,
/// sized to its `rows`, up to the most a palette shows.
pub fn open_list<V: Render>(
    rows: usize,
    parent: &mut Window,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) {
    let height = palette_height(false, false, rows);
    open_padded(PALETTE_WIDTH, height, PALETTE_PADDING, parent, cx, build);
}

/// How high a palette is with `rows` rows, a field over them if `field` and a line under
/// them if `status`.
fn palette_height(field: bool, status: bool, rows: usize) -> f32 {
    let field = if field { FIELD + GAP } else { 0. };
    let status = if status { GAP + SMALL_LINE } else { 0. };
    let rows = rows.min(PALETTE_ROWS) as f32 * crate::files::ROW;
    2. * PALETTE_PADDING + field + rows + status
}

/// A palette: its field, if it has one, over `list`, what it finds, and under that
/// `status`, if it has one, a small muted line on how much; the pieces [`open_palette`] and
/// [`open_list`] size the dialog for.
pub fn palette(
    input: Option<&Entity<InputState>>,
    list: impl IntoElement,
    status: Option<AnyElement>,
    p: Palette,
) -> gpui_kit::Div {
    let field = input.map(|input| {
        let input = Input::new(input)
            .small()
            .prefix(icon(IconName::Search, 14.).text_color(p.muted));
        field(input, p)
    });
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap(px(GAP))
        .children(field)
        .child(list)
        .children(status.map(|status| small_line().text_color(p.muted).child(status)))
}

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
    open_padded(width, height, PADDING, parent, cx, build);
}

/// [`open`], with `padding` round the view instead of a dialog's.
fn open_padded<V: Render>(
    width: f32,
    height: f32,
    padding: f32,
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
            padding,
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

/// What the dialog on screen shows, if one is open and shows a `T`.
pub fn current<T: 'static>(cx: &App) -> Option<Entity<T>> {
    let surface = window(cx)?.downcast::<Surface>()?.read(cx).ok()?;
    surface.content.clone().downcast::<T>().ok()
}

/// The window of the dialog on screen, if one is open.
pub fn window(cx: &App) -> Option<AnyWindowHandle> {
    cx.try_global::<OpenModal>()?.0
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
    /// The room round the view.
    padding: f32,
    _subscriptions: [Subscription; 3],
}

impl Render for Surface {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        crate::key_repeat::track(div(), window, cx)
            .size_full()
            .p(px(self.padding))
            .bg(p.menu)
            .text_color(p.text)
            .text_sm()
            .child(self.content.clone())
    }
}

/// Tells the user something went wrong, over `parent`.
/// Shows `message` in a dialog's footer as why `what` couldn't be done, and notes it in the log.
pub fn fail(error: &mut Option<SharedString>, what: &str, message: impl Into<SharedString>) {
    let message = message.into();
    log::warn!("couldn't {what}: {message}");
    *error = Some(message);
}

pub fn alert(
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
    parent: &mut Window,
    cx: &mut App,
) {
    let (title, message) = (title.into(), message.into());
    log::warn!("{title}: {message}");
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

/// A dialog's title; a dialog that puts a name in it is sized by [`heading_height`].
pub fn heading(text: impl Into<SharedString>) -> impl IntoElement {
    div()
        .text_size(px(HEADING_SIZE))
        .line_height(px(HEADING_LINE))
        .font_weight(FontWeight::SEMIBOLD)
        .line_clamp(MOST_HEADING_LINES)
        .child(text.into())
}

/// How high the [`heading`] `text` is across a dialog `width` wide, measured before the
/// dialog opens, as its window has to be sized then. The dialog's text is the window's
/// default, the heading's weight apart.
pub fn heading_height(text: &SharedString, width: f32, window: &Window) -> f32 {
    let style = TextStyle {
        font_weight: FontWeight::SEMIBOLD,
        ..TextStyle::default()
    };
    let (run, wrap) = (style.to_run(text.len()), Some(px(width - 2. * PADDING)));
    let shaped = (window.text_system()).shape_text(text.clone(), px(HEADING_SIZE), &[run], wrap, None);
    let lines: usize = shaped.map_or(1, |lines| (lines.iter()).map(|line| 1 + line.wrap_boundaries().len()).sum());
    lines.min(MOST_HEADING_LINES) as f32 * HEADING_LINE
}

/// The row of buttons at the bottom of a dialog, against its right edge, with what went
/// wrong, if anything, to their left.
pub fn footer(error: Option<SharedString>, p: Palette) -> gpui_kit::Div {
    div()
        .debug_selector(|| "dialog-footer".into())
        .h(px(FOOTER))
        .flex()
        .items_center()
        .gap_2()
        .child(
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
pub fn field(input: Input, p: Palette) -> gpui_kit::Div {
    div()
        .h(px(FIELD))
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
