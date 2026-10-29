//! How the Files panel looks: its header with its title, then the tree with the file it
//! shows beside it, or why there's no tree yet.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::path::Path;
use std::sync::Arc;

use colosso_project::{Kind, Row};
use gpui_kit::assets::IconName;
use gpui_kit::base::ElementExt as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyElement, App, Bounds, ClickEvent, Context, Div, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, StyleRefinement, Styled as _, Window, div, point, px,
    uniform_list,
};

use super::icons;
use super::panel::FilesPanel;
use crate::changes::{Mark, Marks};
use crate::modal::spinner;
use crate::projects::{CopyPath, RevealInFinder, with_menu};
use crate::right_panel::{
    Activate, Collapse, Expand, NextFile, PageDown, PageUp, PreviousFile, SelectFirst, SelectLast,
    SelectNext, SelectPrevious, note, panel_header, title, wide_button,
};
use crate::settings::{self, SetPreviewFiles};
use crate::shell::{ellipsized, icon, icon_button, row};
use crate::theme::{self, Palette};

/// A row's height: tighter than the sidebar's, as a tree shows many.
pub const ROW: f32 = 24.;
/// How far each folder deeper is set in.
const INDENT: f32 = 14.;

impl Render for FilesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let checkout = self.checkout.read(cx);
        let opened = checkout.project().is_some();
        let failed = checkout.error().filter(|_| !opened);
        // A file can be shown before the tree is listed, opened from elsewhere.
        let body = if let Some(error) = failed {
            note(p).text_color(p.danger).child(error.to_owned())
        } else if !opened || self.rows.is_empty() && !self.scanned {
            self.beside(div().flex_1().relative().child(spinner(p.muted)), p, cx)
        } else if self.rows.is_empty() {
            self.beside(note(p).child("This checkout has no files."), p, cx)
        } else {
            let tree = self.tree(cx);
            self.beside(tree, p, cx)
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.header(opened, p, cx))
            .child(body)
    }
}

impl FilesPanel {
    /// Its title and the tabs of the files open; while the checkout's `opened` and not
    /// listed yet, so; and the button that widens the panel.
    fn header(&self, opened: bool, p: Palette, cx: &mut Context<Self>) -> Div {
        panel_header(p)
            .child(title("Files", p))
            .child(self.tab_strip(p, cx))
            .when(opened && !self.scanned, |header| {
                header.child(div().text_xs().text_color(p.muted).child("Listing…"))
            })
            .when_some(self.unwatched.as_ref(), |header, reason| {
                let tip = format!("Not following files as they change: {reason}");
                header.child(
                    icon_button("files-unwatched", IconName::TriangleAlert, tip, cx).xsmall(),
                )
            })
            .child(wide_button("files-width", self.is_wide(), cx))
    }

    /// The `tree`, or what's there until there is one, its edge dragged to widen it; and
    /// beside it the file shown.
    fn beside(&self, tree: Div, p: Palette, cx: &mut Context<Self>) -> Div {
        let viewer = match self.viewing() {
            // Drawn again only when it changes, not each time the tree does: a held arrow key
            // draws the panel once a row, and the editor's most of that frame otherwise. It's
            // as big as the room beside the tree, whatever it shows.
            Some(viewed) => (viewed.editor.clone())
                .cached(StyleRefinement::default().flex_1().min_w_0().min_h_0())
                .into_any_element(),
            None => note(p)
                .child("Pick a file to see it here.")
                .into_any_element(),
        };
        let tree = div()
            .size_full()
            .flex()
            .flex_col()
            .child(rail(p, cx))
            .child(tree);
        (self.split).render(tree, viewer, |this: &mut Self| &mut this.split, cx)
    }

    fn tree(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_1()
            .min_h_0()
            .py_1()
            .track_focus(&self.focus)
            .key_context("FilesTree")
            .on_action(moving(cx, |this: &mut Self, _: &SelectPrevious, cx| this.step(-1, cx)))
            .on_action(moving(cx, |this: &mut Self, _: &SelectNext, cx| this.step(1, cx)))
            .on_action(moving(cx, |this: &mut Self, _: &SelectFirst, cx| this.select(0, cx)))
            .on_action(moving(cx, |this: &mut Self, _: &SelectLast, cx| {
                this.select(this.rows.len().saturating_sub(1), cx);
            }))
            .on_action(moving(cx, |this: &mut Self, _: &PageUp, cx| this.page(-1, cx)))
            .on_action(moving(cx, |this: &mut Self, _: &PageDown, cx| this.page(1, cx)))
            .on_action(moving(cx, |this: &mut Self, _: &PreviousFile, cx| this.step_file(-1, cx)))
            .on_action(moving(cx, |this: &mut Self, _: &NextFile, cx| this.step_file(1, cx)))
            .on_action(moving(cx, |this: &mut Self, _: &Collapse, cx| this.close_or_parent(cx)))
            .on_action(moving(cx, |this: &mut Self, _: &Expand, cx| this.open_or_child(cx)))
            .on_action(cx.listener(|this, _: &Activate, window, cx| {
                if let Some(at) = this.cursor() {
                    this.activate(at, true, window, cx);
                }
            }))
            .child(
                uniform_list(
                    "files-tree",
                    self.rows.len(),
                    cx.processor(|this, range: Range<usize>, _, cx| {
                        let p = theme::palette(cx);
                        let checkout = this.checkout.read(cx);
                        let marks = checkout.marks().clone();
                        let root: Arc<Path> = checkout.root().as_path().into();
                        range
                            .map(|index| this.row(index, &marks, &root, p, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .size_full()
                .px_1(),
            )
    }

    fn row(
        &self,
        index: usize,
        marks: &Marks,
        root: &Arc<Path>,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(shown) = self.rows.get(index) else {
            return div().into_any_element();
        };
        let selected = self.selected.as_ref() == Some(&shown.path);
        let active = self.active.as_ref() == Some(&shown.path);
        let (colour, badge) = mark(shown, marks, p);
        let chevron = (shown.entry.kind == Kind::Dir).then(|| {
            let glyph = if shown.expanded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            };
            icon(glyph, 12.).text_color(p.muted)
        });
        let element = row(("files-row", index), selected, p)
            .w_full()
            .h(px(ROW))
            .pl(px(4. + shown.depth as f32 * INDENT))
            .gap_1p5()
            .rounded(px(5.))
            // The file shown beside the tree stands out among the rest.
            .when(active && !selected, |row| row.bg(p.hover))
            .when(shown.entry.ignored, |row| row.opacity(0.55))
            // A click previews a file, a double click keeps it open; a folder opens or closes
            // on the first click only.
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                window.focus(&this.focus, cx);
                this.select(index, cx);
                let twice = event.click_count() > 1;
                let folder = this.rows.get(index).is_some_and(|row| row.entry.is_folder());
                if !(twice && folder) {
                    this.activate(index, twice, window, cx);
                }
            }))
            .child(div().w(px(12.)).flex_shrink_0().children(chevron))
            .child(icon(icons::icon(&shown.entry, shown.expanded), 14.).text_color(p.muted))
            .child(
                ellipsized(SharedString::from(shown.entry.name.clone()))
                    .text_color(colour.unwrap_or(p.text))
                    .when(active, |name| name.font_weight(FontWeight::MEDIUM)),
            )
            .child(div().flex_1())
            .children(badge);
        let (root, path) = (root.clone(), shown.path.clone());
        with_menu(element, move || {
            let absolute = root.join(&*path);
            NativeMenu::new()
                .menu("Copy Path", Box::new(CopyPath(absolute.clone())))
                .menu(
                    "Copy Relative Path",
                    Box::new(CopyPath(path.as_ref().into())),
                )
                .separator()
                .menu("Reveal in Finder", Box::new(RevealInFinder(absolute)))
        })
        .into_any_element()
    }
}

/// What git says of a row, as the colour its name takes and what's shown at its end: a
/// file's status letter, or a dot in the strongest status under a folder.
fn mark(row: &Row, marks: &Marks, p: Palette) -> (Option<Hsla>, Option<Div>) {
    if row.entry.is_folder() {
        let colour = marks.folder(&row.path).map(|mark| mark.colour(p));
        (colour, colour.map(dot))
    } else {
        let file = marks.file(&row.path);
        let colour = file.map(|file| file.mark.colour(p));
        let badge =
            file.map(|file| letter(file.mark, !file.unstaged).text_color(file.mark.colour(p)));
        (colour, badge)
    }
}

/// The letter the changes panel shows for a file's status; dimmer once it's all staged.
fn letter(mark: Mark, staged: bool) -> Div {
    div()
        .w(px(12.))
        .mr_1()
        .flex_shrink_0()
        .text_xs()
        .font_family(theme::MONO)
        .font_weight(FontWeight::BOLD)
        .when(staged, |letter| letter.opacity(0.6))
        .child(mark.letter())
}

/// A folder's mark: a dot in its strongest status's colour, which its name takes too.
fn dot(colour: Hsla) -> Div {
    div()
        .size(px(6.))
        .mr_1p5()
        .flex_shrink_0()
        .rounded_full()
        .bg(colour)
}

/// A key that moves the pick, then previews the file it lands on.
fn moving<A: Action>(
    cx: &mut Context<FilesPanel>,
    by: impl Fn(&mut FilesPanel, &A, &mut Context<FilesPanel>) + 'static,
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    cx.listener(move |this, action: &A, window, cx| {
        by(this, action, cx);
        this.preview_picked(window, cx);
    })
}

/// A quiet strip over the tree, with a menu of how the tree shows files at its end.
fn rail(p: Palette, cx: &mut Context<FilesPanel>) -> Div {
    let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
    let measured = bounds.clone();
    let more = icon_button("files-more", IconName::Ellipsis, "More", cx)
        .xsmall()
        .on_click(move |_, window, cx| {
            let below = bounds.get().bottom_left() + point(px(0.), px(4.));
            let preview = settings::get(cx).preview_files;
            NativeMenu::new()
                .menu_with_check(
                    "Preview Files",
                    preview,
                    Box::new(SetPreviewFiles(!preview)),
                )
                .show(below, window, cx);
        });
    div()
        .h(px(26.))
        .flex_shrink_0()
        .px_1()
        .flex()
        .items_center()
        .justify_end()
        .child(div().on_prepaint(move |bounds, _, _| measured.set(bounds)).child(more))
        .text_color(p.muted)
}
