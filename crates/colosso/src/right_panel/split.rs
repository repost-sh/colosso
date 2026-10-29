//! What both of the right panel's views are made of: a list at the left, the changed files or
//! the file tree, as wide as set, and what it shows beside it. Dragging the list's edge
//! widens it as the pointer goes, and the width is saved once the edge is let go. The list
//! is as wide as set unless that leaves what's beside it too little room, which the layout
//! sees to, so a view needn't know how wide the right panel is.

use gpui_kit::{
    Context, Div, DragMoveEvent, Empty, InteractiveElement as _, IntoElement, MouseButton,
    MouseUpEvent, ParentElement as _, Render, Styled as _, Window, div, px,
};

use crate::settings::{self, Settings};
use crate::sidebar::resize_handle;
use crate::theme;

/// One of the splits: the setting the list's width is, and how narrow or wide either side
/// can be.
pub struct Spec {
    /// The list's edge's, and its drag's.
    id: &'static str,
    width: fn(&Settings) -> f32,
    set_width: fn(&mut Settings, f32),
    min: f32,
    max: f32,
    /// The narrowest what's beside the list can be.
    beside_min: f32,
}

/// The changes panel's: the changed files, and their diffs beside them.
pub const CHANGES: Spec = Spec {
    id: "changes-files-resize",
    width: |settings| settings.changes_files_width,
    set_width: |settings, width| settings.changes_files_width = width,
    min: theme::CHANGES_FILES_MIN_WIDTH,
    max: theme::CHANGES_FILES_MAX_WIDTH,
    beside_min: theme::DIFFS_MIN_WIDTH,
};

/// The Files panel's: the tree, and the file shown beside it.
pub const FILES: Spec = Spec {
    id: "files-tree-resize",
    width: |settings| settings.files_tree_width,
    set_width: |settings, width| settings.files_tree_width = width,
    min: theme::FILES_TREE_MIN_WIDTH,
    max: theme::FILES_TREE_MAX_WIDTH,
    beside_min: theme::VIEWER_MIN_WIDTH,
};

/// The drag payload while a list's edge is dragged, naming the split it widens.
#[derive(Clone)]
pub struct SplitResize(&'static str);

impl Render for SplitResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// A view's split: whether the panel it's in has the card to itself, and the list's width
/// while its edge is dragged.
pub struct Split {
    spec: &'static Spec,
    /// As the right panel says when it changes.
    wide: bool,
    /// Before it's saved.
    dragged: Option<f32>,
}

impl Split {
    pub fn new(spec: &'static Spec) -> Self {
        Split {
            spec,
            wide: false,
            dragged: None,
        }
    }

    /// Whether the panel has the card to itself.
    pub fn is_wide(&self) -> bool {
        self.wide
    }

    /// Says whether the panel has the card to itself; whether that's news.
    pub fn set_wide(&mut self, wide: bool) -> bool {
        let news = self.wide != wide;
        self.wide = wide;
        news
    }

    /// The list's width as set, or as it's being dragged, within its limits.
    fn width(&self, settings: &Settings) -> f32 {
        let spec = self.spec;
        (self.dragged)
            .unwrap_or_else(|| (spec.width)(settings))
            .clamp(spec.min, spec.max)
    }

    /// `list` at the left, its edge dragged to widen it, and `beside` it the rest. `split`
    /// finds the view's split.
    pub fn render<V: 'static>(
        &self,
        list: impl IntoElement,
        beside: impl IntoElement,
        split: fn(&mut V) -> &mut Split,
        cx: &mut Context<V>,
    ) -> Div {
        let spec = self.spec;
        let p = theme::palette(cx);
        let let_go = move |this: &mut V, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<V>| {
            let split = split(this);
            if split.dragged.is_some() {
                let width = split.width(settings::get(cx));
                split.dragged = None;
                settings::update(cx, |settings| (spec.set_width)(settings, width));
                cx.notify();
            }
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<SplitResize>, _, cx| {
                    if event.drag(cx).0 != spec.id {
                        return;
                    }
                    // No wider than leaves the room beside it, as the layout would have it.
                    let widest = event.bounds.size.width.as_f32() - spec.beside_min;
                    let width = (event.event.position.x - event.bounds.left()).as_f32();
                    let split = split(this);
                    if split.dragged != Some(width.min(widest)) {
                        split.dragged = Some(width.min(widest));
                        cx.notify();
                    }
                }),
            )
            // Let go wherever the pointer is.
            .capture_any_mouse_up(cx.listener(let_go))
            .on_mouse_up_out(MouseButton::Left, cx.listener(let_go))
            .child(
                div()
                    .relative()
                    .h_full()
                    .w(px(self.width(settings::get(cx))))
                    .min_w_0()
                    .flex_shrink(1.)
                    .flex()
                    .flex_col()
                    .border_r_1()
                    .border_color(p.hairline)
                    .child(list)
                    .child(resize_handle(spec.id, SplitResize(spec.id)).right_0()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(spec.beside_min))
                    .min_h_0()
                    .flex()
                    .child(beside),
            )
    }
}
