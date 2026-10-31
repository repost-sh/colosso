//! How a tab shows in a list: its pane's icon, a page's own icon for a browser, and its
//! title; a split shows each of its panes as a chip in one capsule, side by side, each
//! with its own button that closes that pane alone.

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Axis, Context, Div, Image, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Styled as _, div, img, px,
};

use super::Shell;
use super::elements::{ellipsized, icon, pane_close_button};
use crate::model::{Pane, Tab};
use crate::theme::Palette;

/// The widest a chip of a split gets in the strip.
const CHIP_MAX: f32 = 180.;

/// What a pane shows as in a tab: its icon, or its page's own, and its title.
#[derive(Clone)]
pub struct Face {
    /// The pane's.
    pub id: u64,
    pub icon: IconName,
    pub favicon: Option<Arc<Image>>,
    pub title: SharedString,
}

impl Face {
    pub fn of(pane: &Pane, cx: &App) -> Self {
        Face {
            id: pane.id,
            icon: pane.kind.icon(),
            favicon: (pane.browser.as_ref()).and_then(|view| view.read(cx).favicon()),
            title: pane.title.clone(),
        }
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
            .child(ellipsized(self.title.clone()).flex_grow(1.))
    }
}

/// A split tab's panes, by their `faces`, as chips in a list along `axis`: down the
/// sidebar they share its width evenly; along the strip each is as wide as its title, to a
/// point. The active tab's chips show their close buttons, as do the sidebar's others
/// while hovered.
pub fn split_chips(
    faces: &[Face],
    (axis, active): (Axis, bool),
    p: Palette,
    cx: &mut Context<Shell>,
) -> Div {
    let chips = faces.iter().map(|face| {
        let close = pane_close_button("pane-chip-close", face.id, p, cx).when(
            axis == Axis::Vertical && !active,
            |close| {
                close
                    .opacity(0.)
                    .group_hover("pane-chip", |style| style.opacity(1.))
            },
        );
        face.chip()
            .group("pane-chip")
            .map(|chip| match axis {
                Axis::Vertical => chip.flex_1(),
                Axis::Horizontal => chip.max_w(px(CHIP_MAX)),
            })
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
