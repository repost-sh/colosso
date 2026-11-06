//! Over the file shown, where it is: its folders from the checkout's root with a chevron
//! between each, then its icon and its name. A folder or the name clicked is picked in the
//! tree, so a file far down the tree is placed at a glance.

use std::path::Path;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Context, Div, InteractiveElement as _, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};

use super::file_icon;
use super::file_pane::{FilePane, FilePaneEvent, name};
use super::render::BAR;
use crate::projects::display_path;
use crate::shell::{ellipsized, icon};
use crate::theme::Palette;
use crate::viewer;

/// A piece of the breadcrumb: a folder's or the file's name, and its path as the tree names
/// it, for a file in the checkout.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Crumb {
    pub label: SharedString,
    pub path: Option<Arc<str>>,
}

/// The folders of the file at `path`, `relative` to the checkout as the tree names it, then
/// the file. Outside the checkout, its whole folder, which the tree hasn't got, then the file.
pub(super) fn crumbs(path: &Path, relative: Option<&str>) -> Vec<Crumb> {
    let mut crumbs = Vec::new();
    if let Some(relative) = relative {
        let mut from = 0;
        for (at, _) in relative.match_indices('/') {
            crumbs.push(Crumb {
                label: relative[from..at].to_owned().into(),
                path: Some(relative[..at].into()),
            });
            from = at + 1;
        }
    } else if let Some(folder) = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
    {
        crumbs.push(Crumb {
            label: display_path(folder).into(),
            path: None,
        });
    }
    crumbs.push(Crumb {
        label: name(path).into(),
        path: relative.map(Into::into),
    });
    crumbs
}

impl FilePane {
    /// The bar over the editor. Narrow, the folders give way, cut short; the file's name
    /// never does.
    pub(super) fn breadcrumb(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let crumbs = crumbs(&self.path, self.relative.as_deref());
        let last = crumbs.len() - 1;
        let mut bar = div()
            .h(px(BAR))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .overflow_hidden()
            .text_xs()
            .text_color(p.muted);
        for (index, crumb) in crumbs.into_iter().enumerate() {
            if index > 0 {
                bar = bar.child(icon(IconName::ChevronRight, 12.).flex_shrink_0());
            }
            let piece = div()
                .id(("crumb", index))
                .flex()
                .items_center()
                .gap_1()
                .debug_selector(move || format!("crumb-{index}"));
            let mut piece = if index == last {
                let file = icon(file_icon(&crumb.label), 14.).text_color(p.muted);
                piece
                    .flex_shrink_0()
                    .text_color(p.text)
                    .child(file)
                    .child(crumb.label)
            } else if crumb.path.is_some() {
                piece.min_w_0().child(ellipsized(crumb.label))
            } else {
                // A folder the tree hasn't got is cut from its start: its end places the file.
                let folder = div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_start()
                    .child(crumb.label);
                piece.min_w_0().child(folder)
            };
            if let Some(path) = crumb.path {
                piece = piece
                    .cursor_pointer()
                    .hover(|style| style.text_color(p.text))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(FilePaneEvent::PickInTree(path.clone()));
                    }));
            }
            bar = bar.child(piece);
        }
        if viewer::has_text(&self.path) {
            bar = bar.child(div().flex_1()).child(self.text_or_drawing(p, cx));
        }
        bar
    }

    /// A drawing's two faces, to pick between: the drawing, or its text.
    fn text_or_drawing(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let text = self.shows_text();
        let face = |id: &'static str, label: &'static str, picked: bool| {
            div()
                .id(id)
                .px_1p5()
                .rounded(px(4.))
                .when(picked, |face| face.bg(p.selected).text_color(p.text))
                .when(!picked, |face| {
                    face.cursor_pointer()
                        .hover(|style| style.text_color(p.text))
                })
                .child(label)
        };
        div()
            .flex()
            .flex_shrink_0()
            .gap_0p5()
            .child(
                face("show-drawing", "Preview", !text)
                    .on_click(cx.listener(|this, _, window, cx| this.show_text(false, window, cx))),
            )
            .child(
                face("show-text", "Source", text)
                    .on_click(cx.listener(|this, _, window, cx| this.show_text(true, window, cx))),
            )
    }
}

#[cfg(test)]
mod tests;
