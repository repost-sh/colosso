//! The card's handles, on its free sides: the left edge, the bottom edge and the corner between
//! them, its others being against the pane's top right under its controls (`docs/chat.md` 5.3).
//! Dragged, they size the card, which follows the pointer off them until it's let go and is kept
//! in settings then; double-clicked, they put their sides back as they come.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, CursorStyle, Div, DispatchPhase, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Pixels, Point, Styled as _, canvas, div, px,
};

use super::super::ChatView;
use super::room::NARROWEST;
use super::{Details, WIDTH};
use crate::settings;
use crate::theme::Palette;

/// What the card's kept within, as the pane has room for.
const WIDEST: f32 = 640.;
const SHORTEST: f32 = 160.;
/// How thick an edge's handle is, and how far the corner's goes up and along.
const THICK: f32 = 6.;
const CORNER: f32 = 12.;
/// How long the mark on an edge's handle is.
const MARK: f32 = 24.;

/// A handle, by the sides it sizes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Handle {
    Left,
    Bottom,
    Corner,
}

impl Handle {
    const ALL: [Handle; 3] = [Handle::Left, Handle::Bottom, Handle::Corner];

    fn widens(self) -> bool {
        self != Handle::Bottom
    }

    fn heightens(self) -> bool {
        self != Handle::Left
    }

    fn cursor(self) -> CursorStyle {
        match self {
            Handle::Left => CursorStyle::ResizeLeftRight,
            Handle::Bottom => CursorStyle::ResizeUpDown,
            Handle::Corner => CursorStyle::ResizeUpRightDownLeft,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Handle::Left => "chat-details-resize-left",
            Handle::Bottom => "chat-details-resize-bottom",
            Handle::Corner => "chat-details-resize-corner",
        }
    }
}

/// A handle in hand: which, where the pointer took it and where it is, and the card's size then.
struct Held {
    handle: Handle,
    from: Point<Pixels>,
    at: Point<Pixels>,
    width: f32,
    height: f32,
}

#[derive(Default)]
pub(super) struct Resize {
    held: Option<Held>,
}

impl Details {
    /// Whether a person has one of the card's handles in hand, which keeps it up.
    pub(super) fn resizing(&self) -> bool {
        self.resize.held.is_some()
    }

    /// How wide the card is: as its left edge is dragged, or as a person left it, within its
    /// limits and what the pane has room for, but never narrower than the narrowest.
    pub(super) fn width(&self, cx: &App) -> f32 {
        let wanted = match &self.resize.held {
            Some(held) if held.handle.widens() => held.width - (held.at.x - held.from.x).as_f32(),
            _ => settings::get(cx).chat_details.width.unwrap_or(WIDTH),
        };
        wanted.min(self.widest().min(WIDEST)).max(NARROWEST)
    }

    /// How tall the card is, where a person gave it a height: as its bottom edge is dragged, or as
    /// they left it, within its limits. Without one it's as tall as what it shows, to the tallest.
    pub(super) fn height(&self, cx: &App) -> Option<f32> {
        let wanted = match &self.resize.held {
            Some(held) if held.handle.heightens() => {
                Some(held.height + (held.at.y - held.from.y).as_f32())
            }
            _ => settings::get(cx).chat_details.height,
        };
        wanted.map(|height| height.min(self.tallest()).max(SHORTEST))
    }

    /// A press on `handle`: a double click puts its sides back, a single one takes it.
    fn take(&mut self, handle: Handle, event: &MouseDownEvent, cx: &mut App) {
        if event.click_count >= 2 {
            self.resize.held = None;
            self.keep(handle, None, None, cx);
            return;
        }
        let (width, height) = (self.width(cx), self.height(cx));
        self.resize.held = Some(Held {
            handle,
            from: event.position,
            at: event.position,
            width,
            height: height.unwrap_or(self.drawn.get()),
        });
    }

    fn drag(&mut self, event: &MouseMoveEvent, cx: &mut App) {
        if event.pressed_button.is_none() {
            self.let_go(cx);
        } else if let Some(held) = &mut self.resize.held {
            held.at = event.position;
        }
    }

    /// Keeps the size the card was dragged to, if it was.
    fn let_go(&mut self, cx: &mut App) {
        let Some(held) = &self.resize.held else {
            return;
        };
        if held.at != held.from {
            let handle = held.handle;
            let (width, height) = (self.width(cx), self.height(cx));
            self.keep(handle, Some(width), height, cx);
        }
        self.resize.held = None;
    }

    /// Keeps `width` and `height` for the sides `handle` sizes, putting back those it has none
    /// for, and the card up where it is, beside the chat or over it.
    fn keep(&mut self, handle: Handle, width: Option<f32>, height: Option<f32>, cx: &mut App) {
        let room = self.has_room();
        settings::update(cx, |settings| {
            let details = &mut settings.chat_details;
            if handle.widens() {
                details.width = width;
            }
            if handle.heightens() {
                details.height = height;
            }
            details.open |= room;
        });
        self.over |= !room;
    }
}

/// The card's handles, and what has a handle in hand follow the pointer anywhere in the window.
pub(super) fn handles(details: &Details, p: Palette, cx: &Context<ChatView>) -> Vec<AnyElement> {
    let held = details.resize.held.as_ref().map(|held| held.handle);
    Handle::ALL
        .into_iter()
        .map(|which| handle(which, held == Some(which), p, cx).into_any_element())
        .chain(held.map(|held| following(held, cx).into_any_element()))
        .collect()
}

/// A handle, invisible but for a faint mark along its edge while the pointer's on it or has it.
fn handle(handle: Handle, held: bool, p: Palette, cx: &Context<ChatView>) -> Div {
    let name = handle.name();
    let placed = div()
        .debug_selector(|| name.into())
        .group(name)
        .absolute()
        .flex()
        .items_center()
        .justify_center()
        .cursor(handle.cursor());
    let placed = match handle {
        Handle::Left => placed.left_0().top_0().bottom(px(CORNER)).w(px(THICK)),
        Handle::Bottom => placed.bottom_0().left(px(CORNER)).right_0().h(px(THICK)),
        Handle::Corner => placed.left_0().bottom_0().size(px(CORNER)),
    };
    let mark = match handle {
        Handle::Left => Some(div().w(px(2.)).h(px(MARK))),
        Handle::Bottom => Some(div().w(px(MARK)).h(px(2.))),
        Handle::Corner => None,
    };
    let mark = mark.map(|mark| {
        mark.rounded_full()
            .when(held, |mark| mark.bg(p.muted))
            .group_hover(name, move |mark| mark.bg(p.muted))
    });
    placed.children(mark).on_mouse_down(
        MouseButton::Left,
        cx.listener(move |view, event: &MouseDownEvent, _, cx| {
            view.details.take(handle, event, cx);
            cx.stop_propagation();
            cx.notify();
        }),
    )
}

/// Has the window's every move drag the card while `held` is in hand, and the button's release
/// let go of it, wherever the pointer is.
fn following(held: Handle, cx: &Context<ChatView>) -> impl IntoElement + use<> {
    let view = cx.weak_entity();
    canvas(
        |_, _, _| {},
        move |_, (), window, _| {
            window.set_window_cursor_style(held.cursor());
            let moved = view.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx: &mut App| {
                if phase == DispatchPhase::Capture {
                    moved
                        .update(cx, |view, cx| {
                            view.details.drag(event, cx);
                            cx.notify();
                        })
                        .ok();
                }
            });
            window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx: &mut App| {
                if phase == DispatchPhase::Capture {
                    view.update(cx, |view, cx| {
                        view.details.let_go(cx);
                        cx.notify();
                    })
                    .ok();
                }
            });
        },
    )
    .absolute()
    .size_full()
}
