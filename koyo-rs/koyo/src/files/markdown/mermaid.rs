//! A fenced code block in Mermaid, in a markdown preview: drawn as its diagram, centred on a
//! card of its own, a note in its place while it's drawn, and where Mermaid can't draw it,
//! what was wrong. Copied with the text around it, it's its source.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use gpui_kit::assets::IconName;
use gpui_kit::component::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast};
use gpui_kit::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    Styled as _, Window, div, img, px, relative,
};

use super::diagram::{Card, Diagram, Key, Look};
use crate::shell::icon;
use crate::theme::Palette;

/// The name of the nodes it makes.
const NAME: &str = "mermaid";
/// The room around a diagram on its card.
pub(super) const PADDING: f32 = 20.;
/// How tall the card is while its diagram's drawn.
const DRAWING: f32 = 96.;

/// Draws a preview's Mermaid blocks in `look` on `card`, saying what's wrong in `p`'s colours,
/// and notes each diagram it draws in `seen`, for the preview to keep.
pub struct MermaidPlugin {
    look: Look,
    card: Card,
    p: Palette,
    seen: Arc<Mutex<HashSet<Key>>>,
}

impl MermaidPlugin {
    pub fn new(look: Look, card: Card, p: Palette, seen: Arc<Mutex<HashSet<Key>>>) -> Self {
        MermaidPlugin { look, card, p, seen }
    }
}

/// A Mermaid block's source.
struct Source(SharedString);

/// Whether a fenced block's language is Mermaid's.
pub fn is_mermaid(lang: Option<&str>) -> bool {
    lang.is_some_and(|lang| lang.trim().eq_ignore_ascii_case(NAME))
}

impl MarkdownPlugin for MermaidPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        NAME
    }

    fn parse(&self, node: &markdown_ast::Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let markdown_ast::Node::Code(code) = node else {
            return None;
        };
        if !is_mermaid(code.lang.as_deref()) {
            return None;
        }
        let markdown = cx.node_source(node).unwrap_or(&code.value).to_owned();
        let source = SharedString::from(code.value.clone());
        Some(MarkdownNode::new(NAME, Source(source.clone())).text(source).markdown(markdown))
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let p = self.p;
        // A row, its diagram as wide as it's drawn or narrowed to fit, and as tall as the
        // width it ends up makes it: in a column its height would follow the column's width,
        // and from a width of its own, the width it's drawn at.
        let mut card = div()
            .debug_selector(|| "mermaid-card".into())
            .w_full()
            .flex()
            .justify_center()
            .p(px(PADDING))
            .bg(self.card.paper)
            .border_1()
            .border_color(self.card.border);
        card.style().corner_radii = self.card.corners.clone();
        let Some(Source(source)) = node.data::<Source>() else {
            return card.into_any_element();
        };
        let key = (source.clone(), self.look.clone());
        // Drawn off the main thread; this view's drawn again once it is. Still, not spinning,
        // as each frame something spins would draw the whole page again.
        let content: AnyElement = match window.use_asset::<Diagram>(&key, cx) {
            None => div()
                .h(px(DRAWING - 2. * PADDING))
                .flex()
                .items_center()
                .text_sm()
                .text_color(p.muted)
                .child("Drawing the diagram…")
                .into_any_element(),
            Some(Ok(drawn)) => div()
                .debug_selector(|| "mermaid-diagram".into())
                .flex_basis(px(drawn.width))
                .min_w_0()
                .flex_shrink(1.)
                .aspect_ratio(drawn.width / drawn.height)
                .child(img(drawn.image.clone()).size_full())
                .into_any_element(),
            Some(Err(why)) => failed(&why, p),
        };
        if let Ok(mut seen) = self.seen.lock() {
            seen.insert(key);
        }
        card.child(content).into_any_element()
    }
}

/// What a diagram Mermaid couldn't draw says instead: that it couldn't, and why.
fn failed(why: &SharedString, p: Palette) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1()
        .text_sm()
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .text_color(p.danger)
                .child(icon(IconName::TriangleAlert, 14.))
                .child("Mermaid couldn't draw this diagram"),
        )
        .child(
            div()
                .font_family(p.mono)
                .text_xs()
                .line_height(relative(1.5))
                .text_color(p.muted)
                .child(why.clone()),
        )
        .into_any_element()
}
