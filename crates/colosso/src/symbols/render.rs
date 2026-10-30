//! How the palette looks: the field, then a row for each symbol, its kind in a word, its
//! name in bold with the characters that matched in the accent colour, what the server says
//! of it dimmed after, and at the end what it's in and its file; and under the rows how
//! many there are.

use std::ops::Range;

use colosso_lsp::lsp_types::SymbolKind;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use super::view::Symbols;
use crate::files::ROW;
use crate::list::styled;
use crate::list;
use crate::modal;
use crate::right_panel::{PageDown, PageUp, SelectNext, SelectPrevious, note};
use crate::shell::{self, ellipsized};
use crate::theme::{self, Palette};

/// Room for a kind's word.
const KIND: f32 = 54.;

impl Render for Symbols {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let list = if self.shown.is_empty() {
            note(p).child(self.note())
        } else {
            let rows = cx.processor(move |this: &mut Self, range: Range<usize>, _, cx| {
                range.map(|index| this.row(index, p, cx)).collect()
            });
            list::scrolling("symbols", self.shown.len(), &self.scroll, rows)
        };
        modal::palette(Some(&self.input), list, self.status(), p)
            .key_context("Symbols")
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.page(-1, cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.page(1, cx)))
    }
}

impl Symbols {
    fn row(&self, index: usize, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some((symbol, matched)) = self.row_symbol(index) else {
            return div().into_any_element();
        };
        let selected = index == self.selected;
        let trailing: Option<SharedString> = match (&symbol.container, self.file_of(symbol)) {
            (Some(container), Some(file)) => Some(format!("{container} · {file}").into()),
            (Some(container), None) => Some(container.clone()),
            (None, file) => file,
        };
        shell::row(("symbol-row", index), selected, p)
            .w_full()
            .h(px(ROW))
            .gap_2()
            .rounded(px(5.))
            .on_click(cx.listener(move |this, _, window, cx| this.open(index, window, cx)))
            .child(
                div()
                    .w(px(KIND))
                    .flex_shrink_0()
                    .text_xs()
                    .font_family(theme::MONO)
                    .text_color(p.muted)
                    .child(word(symbol.kind)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(styled(&symbol.name, matched, p.accent)),
            )
            .when_some(symbol.detail.clone(), |row, detail| {
                row.child(ellipsized(detail).min_w_0().text_color(p.muted))
            })
            .child(div().flex_1())
            .when_some(trailing, |row, trailing| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(p.muted)
                        .child(trailing),
                )
            })
            .into_any_element()
    }

    /// What the list says when it has no rows.
    fn note(&self) -> &'static str {
        if self.asking.is_some() {
            "Listing…"
        } else if !self.served {
            "No language server is running for the checkout. Open one of its files to start one."
        } else if self.root.is_some() && self.query.is_empty() {
            "Type a name to find a symbol of the checkout."
        } else if self.root.is_none() && self.known.is_empty() {
            "This file has no symbols."
        } else {
            "No symbols match."
        }
    }

    /// How many symbols are shown, of how many known.
    fn status(&self) -> SharedString {
        let (shown, known) = (self.shown.len(), self.known.len());
        let counted = match (shown == known, known) {
            (_, 0) => String::new(),
            (true, 1) => "1 symbol".to_owned(),
            (true, _) => format!("{known} symbols"),
            (false, _) => format!("{shown} of {known} symbols match"),
        };
        let listing = if self.asking.is_some() { " listing…" } else { "" };
        format!("{counted}{listing}").trim().to_owned().into()
    }
}

/// A kind's word, as the row says it.
fn word(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::FILE => "file",
        SymbolKind::MODULE => "mod",
        SymbolKind::NAMESPACE => "ns",
        SymbolKind::PACKAGE => "pkg",
        SymbolKind::CLASS => "class",
        SymbolKind::METHOD => "method",
        SymbolKind::PROPERTY => "prop",
        SymbolKind::FIELD => "field",
        SymbolKind::CONSTRUCTOR => "new",
        SymbolKind::ENUM => "enum",
        SymbolKind::INTERFACE => "iface",
        SymbolKind::FUNCTION => "fn",
        SymbolKind::VARIABLE => "var",
        SymbolKind::CONSTANT => "const",
        SymbolKind::STRING => "str",
        SymbolKind::NUMBER => "num",
        SymbolKind::BOOLEAN => "bool",
        SymbolKind::ARRAY => "array",
        SymbolKind::OBJECT => "object",
        SymbolKind::KEY => "key",
        SymbolKind::NULL => "null",
        SymbolKind::ENUM_MEMBER => "variant",
        SymbolKind::STRUCT => "struct",
        SymbolKind::EVENT => "event",
        SymbolKind::OPERATOR => "op",
        SymbolKind::TYPE_PARAMETER => "type",
        _ => "",
    }
}
