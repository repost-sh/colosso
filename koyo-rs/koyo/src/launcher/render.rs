//! How the launcher looks, after Zen's floating address bar: the field, the pill on its left
//! saying, in the kind's own colour, what kind of tab Enter opens, and the kind Tab goes to on
//! its right; then, past a hairline, a row for each thing found, its icon, its title with what
//! matched in the accent colour, its address dimmed after it, and what picking it does at its
//! end. The row picked that the history holds has a × that forgets it. The window grows and
//! shrinks with the rows.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, Keystroke,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window, div, img, px,
};

use super::rows::{Act, Glyph, Row};
use super::view::{FIELD, LIST_PADDING, Launcher, ROW, height};
use super::{AcceptSuggestion, NewTab, NextKind, OpenAddress, PreviousKind, RemoveFromHistory};
use crate::list::{SelectNext, SelectPrevious, styled};
use crate::modal;
use crate::pane::close_button;
use crate::shell::{self, ellipsized, icon};
use crate::theme::{self, Palette};

/// The size of the field's text, and of a row's icon.
const TEXT: f32 = 18.;
const GLYPH: f32 = 16.;

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fits = height(self.rows.len());
        if (fits - self.fitted).abs() >= 0.5 {
            self.fitted = fits;
            modal::fit_height(fits, window, cx);
        }
        let p = theme::palette(cx);
        let rows: Vec<AnyElement> = (0..self.rows.len()).map(|at| self.row(at, p, cx)).collect();
        div()
            .key_context("Launcher")
            .size_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(|this, _: &NextKind, window, cx| this.step_kind(1, window, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousKind, window, cx| this.step_kind(-1, window, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.step(-1, cx)))
            .on_action(
                cx.listener(|this, _: &AcceptSuggestion, window, cx| this.accept(window, cx)),
            )
            .on_action(cx.listener(|this, _: &RemoveFromHistory, _, cx| this.remove_selected(cx)))
            // ⌘T again closes it, as Zen's does.
            .on_action(|_: &NewTab, window, cx| modal::dismiss(window, cx))
            .on_action(cx.listener(|this, _: &OpenAddress, window, cx| this.select_all(window, cx)))
            .child(self.field(p, cx))
            .when(!rows.is_empty(), |launcher| {
                launcher
                    .child(div().h(px(1.)).bg(p.hairline))
                    .child(div().flex().flex_col().p(px(LIST_PADDING)).children(rows))
            })
    }
}

impl Launcher {
    /// The pill, the field and, on its right, the kind Tab goes to.
    fn field(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let next = self.kind.step(1);
        let input = Input::new(&self.input)
            .appearance(false)
            .text_size(px(TEXT))
            .line_height(px(TEXT + 6.))
            .h(px(FIELD - 16.))
            .flex_1();
        div()
            .h(px(FIELD))
            .flex_shrink_0()
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .id("launcher-kind")
                    .debug_selector(|| "launcher-kind".into())
                    .h(px(28.))
                    .px(px(10.))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(6.))
                    .rounded_full()
                    .bg(self.kind.color())
                    .text_color(gpui_kit::white())
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .child(icon(self.kind.icon(), 14.))
                    .child(self.kind.label())
                    .on_click(cx.listener(|this, _, window, cx| this.step_kind(1, window, cx))),
            )
            .child(input)
            .child(
                div()
                    .id("launcher-next-kind")
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(6.))
                    .text_xs()
                    .text_color(p.muted)
                    .cursor_pointer()
                    .child(next.label())
                    .children(Keystroke::parse("tab").ok().map(Kbd::new))
                    .on_click(cx.listener(|this, _, window, cx| this.step_kind(1, window, cx))),
            )
    }

    /// Row `at`, picked when clicked.
    fn row(&self, at: usize, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(at) else {
            return div().into_any_element();
        };
        let selected = self.selected == Some(at);
        let removable = selected && row.entry.is_some();
        shell::row(("launcher-row", at), selected, p)
            .h(px(ROW))
            .px(px(10.))
            .gap(px(10.))
            .rounded(px(8.))
            .child(glyph(row, p))
            .child(text(row, p))
            .children(
                row.hint
                    .map(|hint| hint_of(hint, matches!(row.act, Act::Switch(_)), selected, p)),
            )
            .when(removable, |line| {
                line.child(self.remove_button(at, row, p, cx))
            })
            .on_click(cx.listener(move |this, _, window, cx| this.pick(Some(at), window, cx)))
            .into_any_element()
    }

    /// The × that forgets what row `at` shows.
    fn remove_button(
        &self,
        at: usize,
        row: &Row,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let entry = row.entry.clone();
        close_button(("launcher-remove", at), p)
            .debug_selector(|| "launcher-remove".into())
            .text_color(p.muted)
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                if let Some((kind, text)) = &entry {
                    this.remove(*kind, text, cx);
                }
            }))
    }
}

/// A row's icon: the page's own, or the kind's.
fn glyph(row: &Row, p: Palette) -> AnyElement {
    match &row.glyph {
        Glyph::Image(image) => img(image.clone())
            .size(px(GLYPH))
            .flex_shrink_0()
            .into_any_element(),
        Glyph::Icon(name) => icon(*name, GLYPH)
            .text_color(p.muted)
            .flex_shrink_0()
            .into_any_element(),
    }
}

/// A row's title, its detail dimmed after it, cut short together to fit.
fn text(row: &Row, p: Palette) -> impl IntoElement {
    let detail = (!row.detail.is_empty()).then(|| {
        div()
            .flex()
            .min_w_0()
            .text_color(p.muted)
            .child(" — ")
            .child(ellipsized(styled(
                &row.detail,
                &row.detail_matched,
                p.accent,
            )))
    });
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .text_sm()
        .child(
            ellipsized(styled(&row.title, &row.title_matched, p.accent))
                .flex_shrink_0()
                .max_w_full(),
        )
        .children(detail)
}

/// What picking a row does, at its end: plain, and brighter on the row picked; an arrow after
/// it for a switch to a tab.
fn hint_of(hint: &'static str, switch: bool, selected: bool, p: Palette) -> impl IntoElement {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(4.))
        .text_xs()
        .text_color(if selected { p.text } else { p.muted })
        .child(hint)
        .when(switch, |hint| hint.child(icon(IconName::ArrowRight, 12.)))
}
