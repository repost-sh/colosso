//! What runs along the window's bottom: the status bar, across the whole window on the glass
//! under the card, with the selected checkout's language servers at its end, as tall as the
//! rails let it be (`shell::rails`); and the bottom panel, under the workspace.

use std::path::Path;

use gpui_kit::{
    App, AppContext as _, Div, Entity, IntoElement, ParentElement as _, Styled as _, div, px,
};

use crate::language::{self, ServersStatus};
use crate::theme::{self, Palette};

pub struct StatusBar {
    servers: Entity<ServersStatus>,
}

impl StatusBar {
    pub fn new(cx: &mut App) -> Self {
        StatusBar {
            servers: cx.new(ServersStatus::new),
        }
    }

    /// Shows what's running for the selected checkout, by its root and its project's main
    /// checkout.
    pub fn follow(&self, selected: Option<(&Path, &Path)>, cx: &mut App) {
        if let Some((root, project)) = selected {
            language::locate(root, project, cx);
        }
        let root = selected.map(|(root, _)| root);
        self.servers
            .update(cx, |servers, cx| servers.show(root, cx));
    }

    /// The view of the servers, which says when it starts or stops having any.
    pub fn servers(&self) -> &Entity<ServersStatus> {
        &self.servers
    }

    /// The bar's contents, for the rail to lay out at `theme::STATUS_BAR_HEIGHT`.
    pub fn render(&self, p: Palette) -> Div {
        div()
            .flex()
            .items_center()
            .justify_end()
            .px(px(theme::GUTTER))
            .pt(px(2.))
            .pb(px(theme::GUTTER / 2.))
            .text_xs()
            .text_color(p.muted)
            .child(self.servers.clone())
    }
}

/// The panel under the workspace; a stand-in for the terminal it's to hold.
pub fn panel(p: Palette) -> impl IntoElement {
    div()
        .h(px(200.))
        .flex_shrink_0()
        .border_t_1()
        .border_color(p.hairline)
        .p_3()
        .font_family(p.mono)
        .text_sm()
        .text_color(p.muted)
        .bg(p.sunken)
        .child("Terminal")
}
