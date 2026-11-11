//! Dialogs on glass. A dialog sits in a blurred window of its own over the window that opened
//! it, like the menus and the theme picker, so the desktop shows through it, tinted with the
//! palette so it follows light and dark; it floats as the app's floating windows do
//! (`popup.rs`). One is open at a time; Escape closes it, and so do a press in the window behind
//! it and focus leaving them both. Also the pieces dialogs are made of.

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled,
    Subscription, TextStyle, Window, div, point, px, size,
};

use crate::popup::{self, Kind};
use crate::shell::icon;
use crate::theme::{self, Palette};
use crate::wrapped::wrapped_lines;

mod button;
mod step;

pub use button::{Button, Tone, spinner};
pub use step::{Mark, STEP, step};

/// How far below the top of the window behind it a dialog sits.
const TOP: f32 = 72.;
/// How far down the window behind it a floating bar sits, as a share of its height: high, as
/// Spotlight's, with room under it for what it lists.
const FLOATING_TOP: f32 = 0.2;
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
pub const HEADING_LINE: f32 = 24.;
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
    open_at(TOP, width, height, padding, false, parent, cx, build);
}

/// [`open`], for a dialog the person leaves Koyo for and comes back to, as one that sends them
/// to System Settings does: focus leaving doesn't close it, and while Koyo isn't the app in
/// front it's out of the way, unseen.
pub fn open_kept<V: Render>(
    width: f32,
    height: f32,
    parent: &mut Window,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) {
    open_at(TOP, width, height, PADDING, true, parent, cx, build);
}

/// Opens the view `build` makes as a floating bar over `parent`, high in it and edge to edge
/// with its window: the view pads itself, and fits the window to what it shows with
/// [`fit_height`].
pub fn open_floating<V: Render>(
    width: f32,
    height: f32,
    parent: &mut Window,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) {
    let top = (parent.bounds().size.height.as_f32() * FLOATING_TOP).max(TOP);
    open_at(top, width, height, 0., false, parent, cx, build);
}

/// [`open_padded`], `top` below the top of `parent`; `kept` while the person is away from
/// Koyo ([`open_kept`]).
#[expect(
    clippy::too_many_arguments,
    reason = "the dialog's place, size and kind, and what opens it"
)]
fn open_at<V: Render>(
    top: f32,
    width: f32,
    height: f32,
    padding: f32,
    kept: bool,
    parent: &mut Window,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
) {
    let frame = parent.bounds();
    let origin = frame.origin
        + point(
            (frame.size.width - px(width)) / 2.,
            px(top).min(frame.size.height - px(height)).max(px(0.)),
        );
    let bounds = gpui_kit::Bounds::new(origin, size(px(width), px(height)));
    let opening = popup::Opening {
        kind: Kind::Dialog,
        bounds,
        kept,
        source: None,
    };
    popup::open(opening, parent, cx, |window, cx| {
        let content = cx.new(|cx| build(window, cx));
        // Heard in whichever window has the keys: the dialog usually, as it takes them.
        let escape = cx.intercept_keystrokes(|event, _, cx| {
            if event.keystroke.key == "escape" {
                close(cx);
            }
        });
        Surface {
            content: content.into(),
            padding,
            _escape: escape,
        }
    });
}

/// Closes the dialog, from outside it.
pub fn close(cx: &mut App) {
    popup::close(Kind::Dialog, cx);
}

/// What the dialog on screen shows, if one is open and shows a `T`.
pub fn current<T: 'static>(cx: &App) -> Option<Entity<T>> {
    let surface = popup::root::<Surface>(Kind::Dialog, cx)?;
    surface.content.clone().downcast::<T>().ok()
}

/// The window of the dialog on screen, if one is open.
#[cfg(any(test, feature = "visual-check"))]
pub fn window(cx: &App) -> Option<gpui_kit::AnyWindowHandle> {
    popup::window(Kind::Dialog, cx)
}

/// Closes the dialog in `window`, from inside it.
pub fn dismiss(window: &mut Window, cx: &mut App) {
    popup::close_from_inside(window, cx);
}

/// The glass a dialog's view sits on.
struct Surface {
    content: AnyView,
    /// The room round the view.
    padding: f32,
    _escape: Subscription,
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
    let width = px(width - 2. * PADDING);
    let lines = wrapped_lines(text, &style, px(HEADING_SIZE), width, window);
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
