//! The places a symbol led to, listed under the editor: a header per file, then a row per
//! place with its line number and its line, the place itself in the accent colour. A click
//! previews the place, a double click or Enter keeps it open, Escape closes the list. The
//! arrows move over the places, skipping the headers.

use std::ops::Range;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, ClickEvent, Context, FocusHandle, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled as _, Task, UniformListScrollHandle, WeakEntity,
    Window, div, px, uniform_list,
};

use super::panel::FilesPanel;
use super::render::ROW;
use super::tabs::name;
use super::{CloseLocations, Place, file_icon};
use crate::language::Found;
use crate::list::{page, stepped, styled};
use crate::right_panel::{
    Activate, PageDown, PageUp, SelectFirst, SelectLast, SelectNext, SelectPrevious, note,
    panel_header,
};
use crate::shell::{ellipsized, icon, icon_button, row};
use crate::theme::{self, Palette};

/// How tall the list is, under the editor.
const HEIGHT: f32 = 220.;
/// The strip over the rows, with the title and the close button.
const HEADER: f32 = 28.;
/// Room for a line number.
const NUMBER: f32 = 36.;
/// How long the pick rests on a place before the place is shown: a held arrow moves over
/// the places without opening each one passed.
const PEEK_DELAY: Duration = Duration::from_millis(80);

/// A row of the list: a file's header, or a place in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    File(usize),
    Place { file: usize, row: usize },
}

pub struct Locations {
    found: Found,
    /// The rows in order, each file's header before its places.
    items: Vec<Item>,
    /// The places among `items`, by index, in the list's order.
    places: Vec<usize>,
    /// The place picked, as an index into `places`.
    selected: usize,
    files: WeakEntity<FilesPanel>,
    pub(super) focus: FocusHandle,
    scroll: UniformListScrollHandle,
    /// Showing the place picked, once the pick has rested on it.
    peeking: Task<()>,
}

impl Locations {
    pub(super) fn new(found: Found, files: WeakEntity<FilesPanel>, cx: &mut Context<Self>) -> Self {
        let (mut items, mut places) = (Vec::new(), Vec::new());
        for (file, locations) in found.files.iter().enumerate() {
            items.push(Item::File(file));
            for row in 0..locations.rows.len() {
                places.push(items.len());
                items.push(Item::Place { file, row });
            }
        }
        Locations {
            found,
            items,
            places,
            selected: 0,
            files,
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            peeking: Task::ready(()),
        }
    }

    /// What was found, as listed.
    #[cfg(test)]
    pub fn found(&self) -> &Found {
        &self.found
    }

    /// The place picked: which file's, and which of its rows.
    pub fn selected(&self) -> Option<(usize, usize)> {
        match self.items[*self.places.get(self.selected)?] {
            Item::Place { file, row } => Some((file, row)),
            Item::File(_) => None,
        }
    }

    /// Picks the place `at` among the places, and scrolls to it.
    fn select(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some(&index) = self.places.get(at) {
            self.selected = at;
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    /// Moves the pick `by` places, stopping at either end, and shows the one landed on once
    /// the pick rests there.
    fn step(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(landed) = stepped(Some(self.selected), by, self.places.len()) else {
            return;
        };
        self.select(landed, cx);
        self.peeking = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(PEEK_DELAY).await;
            this.update_in(cx, |this, window, cx| this.open(false, window, cx)).ok();
        });
    }

    /// Whether the list has the keyboard.
    #[cfg(test)]
    pub fn has_focus(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    /// Shows the place picked in the Files panel: peeked at, the keyboard staying here, or
    /// `pinned` in a tab of its own and given the keyboard.
    fn open(&self, pinned: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((file, row)) = self.selected() else {
            return;
        };
        let file = &self.found.files[file];
        let place = Place {
            path: file.path.clone(),
            at: Some(file.rows[row].at.clone()),
        };
        (self.files.update(cx, |files, cx| {
            if pinned {
                files.view(place, window, cx);
            } else {
                files.peek(place, window, cx);
            }
        }))
        .ok();
    }

    fn close(&self, window: &mut Window, cx: &mut Context<Self>) {
        (self.files.update(cx, |files, cx| files.close_locations(window, cx))).ok();
    }
}

impl Render for Locations {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let list = if self.items.is_empty() {
            note(p).child("Nothing was found.").into_any_element()
        } else {
            uniform_list(
                "locations",
                self.items.len(),
                cx.processor(|this, range: Range<usize>, _, cx| {
                    let p = theme::palette(cx);
                    range.map(|index| this.row(index, p, cx)).collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.scroll)
            .size_full()
            .px_1()
            .into_any_element()
        };
        div()
            .h(px(HEIGHT))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(p.hairline)
            .bg(p.sunken)
            .track_focus(&self.focus)
            .key_context("Locations")
            .on_action(cx.listener(|this, _: &SelectPrevious, window, cx| this.step(-1, window, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, window, cx| this.step(1, window, cx)))
            .on_action(cx.listener(|this, _: &SelectFirst, window, cx| this.step(isize::MIN, window, cx)))
            .on_action(cx.listener(|this, _: &SelectLast, window, cx| this.step(isize::MAX, window, cx)))
            .on_action(cx.listener(|this, _: &PageUp, window, cx| {
                this.step(-page(&this.scroll, ROW), window, cx);
            }))
            .on_action(cx.listener(|this, _: &PageDown, window, cx| {
                this.step(page(&this.scroll, ROW), window, cx);
            }))
            .on_action(cx.listener(|this, _: &Activate, window, cx| this.open(true, window, cx)))
            .on_action(cx.listener(|this, _: &CloseLocations, window, cx| this.close(window, cx)))
            .child(self.header(p, cx))
            .child(div().flex_1().min_h_0().py_1().child(list))
    }
}

impl Locations {
    /// The title, and the button that closes the list.
    fn header(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        panel_header(p)
            .h(px(HEADER))
            .pl_3()
            .pr_1p5()
            .text_xs()
            .child(ellipsized(self.found.title.clone()).flex_1().text_color(p.text))
            .child(
                icon_button("locations-close", IconName::X, "Close", cx)
                    .xsmall()
                    .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            )
    }

    fn row(&mut self, index: usize, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        match self.items.get(index) {
            Some(&Item::File(file)) => self.file_row(file, p),
            Some(&Item::Place { file, row }) => {
                // The places are listed in order, so the row's is found by its index.
                let at = self.places.binary_search(&index).unwrap_or(0);
                self.place_row(at, file, row, p, cx)
            }
            None => div().into_any_element(),
        }
    }

    /// A file's header: its icon, its path and how many places it has.
    fn file_row(&self, file: usize, p: Palette) -> AnyElement {
        let file = &self.found.files[file];
        div()
            .h(px(ROW))
            .flex()
            .items_center()
            .gap_1p5()
            .px_1p5()
            .text_xs()
            .child(icon(file_icon(&name(&file.path)), 13.).text_color(p.muted))
            .child(
                ellipsized(file.relative.clone())
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.text),
            )
            .child(div().text_color(p.muted).child(file.rows.len().to_string()))
            .into_any_element()
    }

    /// The place `at` among the places: its line number, and its line with the place in
    /// the accent colour.
    fn place_row(
        &self,
        at: usize,
        file: usize,
        place: usize,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let located = &self.found.files[file].rows[place];
        let text = styled(&located.text, std::slice::from_ref(&located.matched), p.accent);
        row(("locations-row", at), self.selected == at, p)
            .w_full()
            .h(px(ROW))
            .pl(px(24.))
            .pr_1p5()
            .gap_2()
            .rounded(px(5.))
            .font_family(theme::MONO)
            .text_xs()
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                window.focus(&this.focus, cx);
                this.select(at, cx);
                this.open(event.click_count() > 1, window, cx);
            }))
            .child(
                div()
                    .w(px(NUMBER))
                    .flex_shrink_0()
                    .text_right()
                    .text_color(p.muted)
                    .child(SharedString::from((located.line + 1).to_string())),
            )
            .child(ellipsized(text).flex_1().text_color(p.text))
            .when(located.text.is_empty(), |row| {
                row.child(div().text_color(p.muted).child("(not read)"))
            })
            .into_any_element()
    }
}
