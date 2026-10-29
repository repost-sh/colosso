use std::{
    cell::Cell,
    ops::Range,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AbsoluteLength, AnyElement, App, AppContext as _, AvailableSpace, Bounds, Context,
    EdgesRefinement, Element, ElementId, Entity, Focusable as _, Global, HitboxBehavior,
    InteractiveElement, IntoElement, MouseDownEvent, MouseMoveEvent, ParentElement as _, Pixels,
    Render, SharedString, Size, StatefulInteractiveElement as _, Styled, Window, deferred, div,
    point, px, size,
};

use crate::{
    ActiveTheme as _, ThemeStyled as _,
    animation::ease_out_cubic,
    highlighter::DiagnosticEntry,
    input::{
        EditorState,
        popovers::{render_diagnostic, render_markdown},
    },
    styled::popover_ring,
};

/// What an app puts behind the editor's hover and diagnostic cards, a platform's blur say,
/// the cards then drawn clear over it. Set with [`set_popover_backdrop`].
pub trait PopoverBackdrop: 'static {
    /// Whether it shows behind cards in `window`. Asked as a card is laid out, under the
    /// card's element id, which [`Self::place`] is called under too.
    fn shows(&self, window: &mut Window, cx: &mut App) -> bool;
    /// Puts it behind the card at `bounds` for this frame, rounded by `radius`. Called as the
    /// card's editor is prepainted, so a native view goes under the card rather than over.
    fn place(&self, bounds: Bounds<Pixels>, radius: Pixels, window: &mut Window, cx: &mut App);
}

struct GlobalPopoverBackdrop(Rc<dyn PopoverBackdrop>);

impl Global for GlobalPopoverBackdrop {}

/// Puts `backdrop` behind the editor's hover and diagnostic cards from now on.
pub fn set_popover_backdrop(backdrop: impl PopoverBackdrop, cx: &mut App) {
    cx.set_global(GlobalPopoverBackdrop(Rc::new(backdrop)));
}

fn popover_backdrop(cx: &App) -> Option<Rc<dyn PopoverBackdrop>> {
    cx.try_global::<GlobalPopoverBackdrop>()
        .map(|backdrop| backdrop.0.clone())
}

/// A card's corners.
const RADIUS: Pixels = px(10.);
/// Between the two cards, and between a card and what it's about.
const GAP: Pixels = px(4.);
/// How long a card takes to open, change size or close.
const MOTION: Duration = Duration::from_millis(180);
/// A card's largest width, and its largest height before it scrolls.
const MAX_WIDTH: Pixels = px(500.);
const MAX_HEIGHT: Pixels = px(320.);
/// The least space kept between a card and the window's edge.
const SNAP_TO_EDGE: Pixels = px(8.);

/// How big a card is drawn: it eases from the size it's drawn at toward the size it's to be,
/// its content's or nothing.
#[derive(Default)]
struct Motion(Cell<Option<Tween>>);

#[derive(Clone, Copy)]
struct Tween {
    from: Size<Pixels>,
    to: Size<Pixels>,
    started: Instant,
}

impl Tween {
    /// The size at `now`, and whether it has arrived.
    fn at(&self, now: Instant) -> (Size<Pixels>, bool) {
        let t = now.saturating_duration_since(self.started).as_secs_f32() / MOTION.as_secs_f32();
        if t >= 1. {
            return (self.to, true);
        }
        let eased = ease_out_cubic(t);
        let lerp = |from: Pixels, to: Pixels| from + (to - from) * eased;
        let at = size(
            lerp(self.from.width, self.to.width),
            lerp(self.from.height, self.to.height),
        );
        (at, false)
    }
}

impl Motion {
    /// The size to draw at now, heading for `to`, and whether it's still on its way. With
    /// `still`, for those who'd rather nothing moved, it's there at once.
    fn toward(&self, to: Size<Pixels>, still: bool) -> (Size<Pixels>, bool) {
        let now = Instant::now();
        let started = if still { now - MOTION } else { now };
        let tween = match self.0.get() {
            Some(tween) if tween.to == to => tween,
            Some(tween) => Tween {
                from: tween.at(now).0,
                to,
                started,
            },
            None => Tween {
                from: Size::default(),
                to,
                started,
            },
        };
        self.0.set(Some(tween));
        let (at, arrived) = tween.at(now);
        (at, !arrived)
    }

    /// Whether it has opened all the way.
    fn is_open(&self) -> bool {
        (self.0.get())
            .is_some_and(|tween| tween.to != Size::default() && tween.at(Instant::now()).1)
    }

    /// Whether it's at nothing, having shrunk away or not opened yet.
    fn is_closed(&self) -> bool {
        (self.0.get())
            .is_none_or(|tween| tween.to == Size::default() && tween.at(Instant::now()).1)
    }
}

/// One of the cards: what it shows, kept while it shrinks away, and whether it's wanted.
struct Card<T> {
    shown: Option<T>,
    wanted: bool,
    motion: Rc<Motion>,
}

impl<T> Default for Card<T> {
    fn default() -> Self {
        Card {
            shown: None,
            wanted: false,
            motion: Rc::default(),
        }
    }
}

impl<T> Card<T> {
    /// Shows `value`, or has the card go.
    fn set(&mut self, value: Option<T>) {
        self.wanted = value.is_some();
        if value.is_some() {
            self.shown = value;
        }
    }

    /// Whether it's still to be drawn, shown or shrinking away.
    fn is_up(&self) -> bool {
        self.shown.is_some()
    }

    /// Lets go of what a card that has shrunk away showed.
    fn forget_if_closed(&mut self) {
        if !self.wanted && self.motion.is_closed() {
            self.shown = None;
        }
    }

    /// Whether it's going and has shrunk away, so it's to be forgotten next frame.
    fn is_gone(&self) -> bool {
        !self.wanted && self.motion.is_closed()
    }
}

/// The editor's diagnostic and hover cards. The diagnostic under the mouse sits next to what
/// it's about, the hover beyond it, as a second card once the first has opened. Each grows
/// from the corner where it meets its symbol, changes size smoothly, and shrinks back to
/// that corner as it goes: the hover first when both go together.
pub struct EditorCards {
    editor: Entity<EditorState>,
    diagnostic: Card<Rc<DiagnosticEntry>>,
    /// The symbol hovered and the server's markdown for it.
    hover: Card<(Range<usize>, SharedString)>,
}

impl EditorCards {
    pub(crate) fn new(editor: Entity<EditorState>, cx: &mut App) -> Entity<Self> {
        cx.new(|_| EditorCards {
            editor,
            diagnostic: Card::default(),
            hover: Card::default(),
        })
    }

    /// Shows the diagnostic under the mouse, or has its card go.
    pub(crate) fn show_diagnostic(
        &mut self,
        diagnostic: Option<Rc<DiagnosticEntry>>,
        cx: &mut Context<Self>,
    ) {
        self.diagnostic.set(diagnostic);
        cx.notify();
    }

    /// Shows the hover for the symbol at `range`, or has its card go.
    pub(crate) fn show_hover(
        &mut self,
        hover: Option<(Range<usize>, &lsp_types::Hover)>,
        cx: &mut Context<Self>,
    ) {
        let hover = hover.map(|(range, hover)| (range, hover_markdown(hover).into()));
        self.hover.set(hover);
        cx.notify();
    }

    /// Whether a card is up, or still shrinking away.
    pub(crate) fn is_up(&self) -> bool {
        self.diagnostic.is_up() || self.hover.is_up()
    }
}

/// The hover's contents as one markdown text.
fn hover_markdown(hover: &lsp_types::Hover) -> String {
    let marked = |marked: &lsp_types::MarkedString| match marked {
        lsp_types::MarkedString::String(text) => text.clone(),
        lsp_types::MarkedString::LanguageString(code) => code.value.clone(),
    };
    match &hover.contents {
        lsp_types::HoverContents::Scalar(scalar) => marked(scalar),
        lsp_types::HoverContents::Array(items) => {
            items.iter().map(marked).collect::<Vec<_>>().join("\n\n")
        }
        lsp_types::HoverContents::Markup(markup) => markup.value.clone(),
    }
}

impl Render for EditorCards {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.diagnostic.forget_if_closed();
        self.hover.forget_if_closed();
        let (diagnostic, hover) = (&self.diagnostic, &self.hover);
        let mut cards = Vec::with_capacity(2);
        if let Some(entry) = diagnostic.shown.clone() {
            cards.push(CardLayout {
                key: "diagnostic",
                range: entry.range.clone(),
                // It stays while the hover beyond it shrinks away.
                open: diagnostic.wanted || (hover.is_up() && !hover.wanted && !hover.is_gone()),
                wanted: diagnostic.wanted,
                gone: diagnostic.is_gone(),
                motion: diagnostic.motion.clone(),
                content: CardContent::Diagnostic(entry),
            });
        }
        if let Some((range, markdown)) = hover.shown.clone() {
            cards.push(CardLayout {
                key: "hover",
                range,
                // It opens once the diagnostic before it has.
                open: hover.wanted
                    && (!diagnostic.wanted || diagnostic.motion.is_open()),
                wanted: hover.wanted,
                gone: hover.is_gone(),
                motion: hover.motion.clone(),
                content: CardContent::Hover(markdown),
            });
        }
        Cards {
            editor: self.editor.clone(),
            cards,
        }
    }
}

enum CardContent {
    Diagnostic(Rc<DiagnosticEntry>),
    Hover(SharedString),
}

/// One card as the element lays it out.
struct CardLayout {
    key: &'static str,
    /// What it's about.
    range: Range<usize>,
    /// Whether it's to be open now, else shrinking or staying shut.
    open: bool,
    /// Whether it's wanted at all, which it is unless it's going.
    wanted: bool,
    /// Whether it has gone, to be forgotten on the next frame.
    gone: bool,
    motion: Rc<Motion>,
    content: CardContent,
}

/// The cards, stacked away from what the first is about.
struct Cards {
    editor: Entity<EditorState>,
    cards: Vec<CardLayout>,
}

impl IntoElement for Cards {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

pub(crate) struct CardsLayout {
    /// Each card, where it's drawn.
    drawn: Vec<(Bounds<Pixels>, AnyElement)>,
    /// What the cards are about.
    trigger: Option<Bounds<Pixels>>,
    /// Whether the cards are stacked above it.
    above: bool,
    backdrop: Option<Rc<dyn PopoverBackdrop>>,
    /// Whether another frame is wanted: a card is on its way, waiting on the other, or gone
    /// and to be forgotten.
    moving: bool,
}

impl Cards {
    /// Whether a card is wanted, so the mouse leaving them has them go.
    fn wanted(&self) -> bool {
        self.cards.iter().any(|card| card.wanted)
    }

    /// What a card shows, padded and held to its largest size, scrolling past it.
    fn content(
        card: &CardLayout,
        max_width: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Stateful<gpui::Div> {
        let max_height = (window.bounds().size.height - SNAP_TO_EDGE * 2).min(MAX_HEIGHT);
        let content = match &card.content {
            CardContent::Diagnostic(entry) => render_diagnostic(entry, window, cx),
            CardContent::Hover(markdown) => {
                render_markdown("message", markdown.clone(), window, cx).into_any_element()
            }
        };
        div()
            .id("content")
            .px_3()
            .py_2()
            .text_xs()
            .font_family(cx.theme().font_family.clone())
            .max_w(max_width)
            .max_h(max_height)
            .overflow_y_scroll()
            .child(content)
    }

    /// A card at `drawn`, its content laid out at `laid_out` and clipped to it.
    fn card(
        card: &CardLayout,
        laid_out: Size<Pixels>,
        drawn: Size<Pixels>,
        max_width: Pixels,
        backdrop: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let content = Self::content(card, max_width, window, cx)
            .flex_none()
            .w(laid_out.width)
            .h(laid_out.height);
        deferred(
            div()
                .id("card")
                .flex_none()
                .when(card.wanted, |this| this.occlude())
                .w(drawn.width)
                .h(drawn.height)
                .overflow_hidden()
                .popover_style(cx)
                .rounded(RADIUS)
                // Over a backdrop the card is clear, and the shadow's ring that edges it
                // would tint it all through; a hairline border in its ink edges it instead.
                .when(backdrop, |mut this| {
                    let hairline = Some(AbsoluteLength::Pixels(px(0.5)));
                    this.style().border_widths = EdgesRefinement {
                        top: hairline,
                        right: hairline,
                        bottom: hairline,
                        left: hairline,
                    };
                    this.bg(cx.theme().transparent)
                        .shadow_none()
                        .border_color(popover_ring(cx))
                })
                .child(content),
        )
        .into_any_element()
    }
}

impl Element for Cards {
    type RequestLayoutState = CardsLayout;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some("editor-cards".into())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui::LayoutId, Self::RequestLayoutState) {
        let empty = div().into_any_element().request_layout(window, cx);
        // The cards hang off what the first is about.
        let trigger = (self.cards.first())
            .and_then(|card| self.editor.read(cx).range_to_bounds(&card.range));
        let mut layout = CardsLayout {
            drawn: Vec::new(),
            trigger,
            above: false,
            backdrop: popover_backdrop(cx),
            moving: false,
        };
        let Some(trigger) = trigger else {
            return (empty, layout);
        };
        let max_width = MAX_WIDTH
            .min(window.bounds().size.width - SNAP_TO_EDGE * 2)
            .max(px(200.));
        let still = cx.reduce_motion();

        // Each card's content's own size, measured under the id the drawn card's content
        // has, so they share its state; then the card's size as it eases there or away.
        let mut sizes = Vec::with_capacity(self.cards.len());
        for card in &self.cards {
            let laid_out = window.with_id(card.key, |window| {
                window.with_id("card", |window| {
                    Self::content(card, max_width, window, cx)
                        .into_any_element()
                        .layout_as_root(AvailableSpace::min_size(), window, cx)
                })
            });
            let to = if card.open {
                laid_out
            } else {
                Size::default()
            };
            let (drawn, moving) = card.motion.toward(to, still);
            layout.moving |= moving || card.open != card.wanted || card.gone;
            sizes.push((laid_out, drawn));
        }

        // Above what they're about while the cards fit in the editor there, so they stay off
        // the window's title bar and panels, else below; to its right unless the window ends
        // first. Settled by the sizes they're growing to, so they don't jump as they grow.
        let full_height = (sizes.iter()).fold(px(0.), |sum, (laid_out, _)| {
            sum + laid_out.height + GAP
        });
        let full_width = (sizes.iter()).fold(px(0.), |widest, (laid_out, _)| {
            widest.max(laid_out.width)
        });
        let editor_top = self.editor.read(cx).input_bounds().top().max(px(0.));
        layout.above = full_height <= trigger.top() - editor_top - SNAP_TO_EDGE;
        let leftward = full_width > window.bounds().size.width - trigger.left() - SNAP_TO_EDGE;

        // Stacked outward from what they're about, each card's gap growing with it.
        let mut reach = px(0.);
        for (card, (laid_out, drawn)) in self.cards.iter().zip(sizes) {
            let gap = if laid_out.height > px(0.) {
                GAP * (drawn.height / laid_out.height).min(1.)
            } else {
                px(0.)
            };
            let x = if leftward {
                trigger.right() - drawn.width
            } else {
                trigger.left()
            };
            let y = if layout.above {
                trigger.top() - reach - gap - drawn.height
            } else {
                trigger.bottom() + reach + gap
            };
            reach += gap + drawn.height;
            let element = window.with_id(card.key, |window| {
                let shows = (layout.backdrop.as_ref())
                    .is_some_and(|backdrop| backdrop.shows(window, cx));
                let mut element = Self::card(card, laid_out, drawn, max_width, shows, window, cx);
                element.layout_as_root(AvailableSpace::min_size(), window, cx);
                element
            });
            layout.drawn.push((Bounds::new(point(x, y), drawn), element));
        }
        (empty, layout)
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if layout.moving {
            window.request_animation_frame();
        }
        // From what they're about to the cards, and between them, the mouse is the cards':
        // the editor under it would take it for a hover somewhere else, and have them go.
        if let (true, Some(trigger), Some(cards)) =
            (self.wanted(), layout.trigger, span(&layout.drawn))
        {
            let reach = if layout.above {
                Bounds::from_corners(cards.origin, point(cards.right(), trigger.top()))
            } else {
                Bounds::from_corners(point(cards.left(), trigger.bottom()), cards.bottom_right())
            };
            window.insert_hitbox(reach, HitboxBehavior::BlockMouse);
        }
        for (card, (bounds, element)) in self.cards.iter().zip(&mut layout.drawn) {
            window.with_id(card.key, |window| {
                if let Some(backdrop) = &layout.backdrop {
                    backdrop.place(*bounds, RADIUS, window, cx);
                }
                window.with_absolute_element_offset(bounds.origin, |window| {
                    element.prepaint(window, cx);
                });
            });
        }
    }

    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (card, (_, element)) in self.cards.iter().zip(&mut layout.drawn) {
            window.with_id(card.key, |window| element.paint(window, cx));
        }
        let (Some(trigger), Some(cards), true) =
            (layout.trigger, span(&layout.drawn), self.wanted())
        else {
            return;
        };
        // Focus taken into a card, to select its text, goes back to the editor with it.
        let close = |editor: &Entity<EditorState>, window: &mut Window, cx: &mut App| {
            let focus = editor.read(cx).focus_handle(cx);
            if !focus.is_focused(window) && focus.contains_focused(window, cx) {
                focus.focus(window, cx);
            }
            editor.update(cx, |editor, cx| {
                editor.clear_hover_state(cx);
                editor.clear_diagnostic_popover(cx);
            });
        };

        // A press outside the cards has them go.
        let editor = self.editor.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, _, window, cx| {
            if !cards.contains(&event.position) {
                close(&editor, window, cx);
            }
        });

        // So does the mouse leaving them and what they're about, unless it's selecting text
        // in them.
        let editor = self.editor.clone();
        let keep_open = trigger.union(&cards);
        window.on_mouse_event(move |event: &MouseMoveEvent, _, window, cx| {
            if event.pressed_button.is_none() && !keep_open.contains(&event.position) {
                close(&editor, window, cx);
            }
        })
    }
}

/// What the cards cover together, if there are any.
fn span(drawn: &[(Bounds<Pixels>, AnyElement)]) -> Option<Bounds<Pixels>> {
    (drawn.iter())
        .map(|(bounds, _)| *bounds)
        .reduce(|all, bounds| all.union(&bounds))
}
