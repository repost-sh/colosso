//! How a tab shows in a list: its pane's icon, a page's own icon for a browser or the
//! file's for a file, and its title, in italics for a preview; a split shows each of its
//! panes as a chip in one capsule, side by side, each with its own button that closes that
//! pane alone, a dot in its place while the pane's file has unsaved edits.

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Axis, Context, Div, Image, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Styled, div, img, px,
};

use super::Shell;
use super::elements::{ellipsized, icon, pane_close_button};
use crate::files::{FilePane, file_icon};
use crate::model::{Pane, Tab};
use crate::theme::Palette;

/// The widest a chip of a split gets in the strip.
const CHIP_MAX: f32 = 180.;
/// A pane's close button, which a half of a split down the sidebar makes room for as it shows.
const CLOSE: f32 = 18.;

/// What a pane shows as in a tab: its icon, or its page's own, and its title.
#[derive(Clone)]
pub struct Face {
    /// The pane's.
    pub id: u64,
    pub icon: IconName,
    pub favicon: Option<Arc<Image>>,
    pub title: SharedString,
    /// Whether it's a file the next file previewed takes the place of.
    pub preview: bool,
    /// Whether it's a file with unsaved edits.
    pub dirty: bool,
}

impl Face {
    pub fn of(pane: &Pane, cx: &App) -> Self {
        let file = pane.file.as_ref().map(|file| file.read(cx));
        Face {
            id: pane.id,
            // A file's pane is titled with its name; one brought back shows as what it opens.
            icon: if file.is_some() || pane.unopened_file().is_some() {
                file_icon(&pane.title)
            } else if pane.shows_changes() {
                IconName::FileDiff
            } else {
                pane.kind.icon()
            },
            favicon: pane.favicon(cx),
            title: pane.title.clone(),
            preview: file.is_some_and(FilePane::is_preview),
            dirty: file.is_some_and(FilePane::is_dirty),
        }
    }

    /// Its title, cut short to fit, in italics for a preview.
    pub fn label(&self) -> Div {
        ellipsized(self.title.clone()).when(self.preview, Styled::italic)
    }

    /// Each pane of `tab`, left to right.
    pub fn all(tab: &Tab, cx: &App) -> Vec<Self> {
        tab.panes()
            .into_iter()
            .map(|pane| Face::of(pane, cx))
            .collect()
    }

    /// Its icon, `size` points square: the page's own when it has one.
    pub fn icon(&self, size: f32) -> AnyElement {
        match &self.favicon {
            Some(favicon) => img(favicon.clone())
                .size(px(size))
                .flex_shrink_0()
                .into_any_element(),
            None => icon(self.icon, size).into_any_element(),
        }
    }

    /// Its icon and title as a chip, as each pane of a split shows.
    pub fn chip(&self) -> Div {
        div()
            .h(px(22.))
            .min_w_0()
            .flex()
            .items_center()
            .gap_1p5()
            .rounded(px(5.))
            .child(self.icon(13.))
            .child(self.label().flex_grow(1.))
    }
}

/// A split tab's panes, by their `faces`, in a list along `axis`. Down the sidebar they're as
/// Arc has them (`split_halves`); along the strip they're chips, each as wide as its title, to a
/// point, each with its close button.
pub fn split_chips(faces: &[Face], axis: Axis, p: Palette, cx: &mut Context<Shell>) -> Div {
    if axis == Axis::Vertical {
        return split_halves(faces, p, cx);
    }
    let chips = faces.iter().map(|face| {
        let close = pane_close_button("pane-chip-close", face.id, face.dirty, p, cx);
        face.chip()
            .max_w(px(CHIP_MAX))
            .pl_1p5()
            .pr_0p5()
            .bg(p.hover)
            .child(close)
    });
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap_1()
        .children(chips)
}

/// A split tab's panes down the sidebar, as Arc has them (`docs/arc-sidebar.md` 5).
fn split_halves(faces: &[Face], p: Palette, cx: &mut Context<Shell>) -> Div {
    let mut parts: Vec<AnyElement> = Vec::new();
    for (at, face) in faces.iter().enumerate() {
        if at > 0 {
            parts.push(
                div()
                    .w(px(1.))
                    .h(px(16.))
                    .flex_shrink_0()
                    .bg(p.text.opacity(0.2))
                    .into_any_element(),
            );
        }
        // It takes room only while it shows, so the title has all the half till then.
        let close = div()
            .flex_shrink_0()
            .overflow_hidden()
            .when(!face.dirty, |close| {
                close
                    .w(px(0.))
                    .group_hover("pane-half", |style| style.w(px(CLOSE)))
            })
            .child(pane_close_button(
                "pane-chip-close",
                face.id,
                face.dirty,
                p,
                cx,
            ));
        parts.push(
            div()
                .group("pane-half")
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(p.text.opacity(0.75))
                        .child(face.icon(16.)),
                )
                .child(face.label().flex_1())
                .child(close)
                .into_any_element(),
        );
    }
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .items_center()
        .gap(px(10.))
        .children(parts)
}
