//! What's typed, formatted as markdown where it's typed (`docs/chat.md` 5.3), as in Typora or
//! Obsidian's live preview: styles laid over the text, never a change to it, so what's sent is what
//! was typed. Each construct's words take its look, and the marks that make it stay, dimmed. The
//! text is read with the parser the text views use, at each change of it, and its styles are in
//! their colours, so what's typed reads as it will once sent.

use std::ops::Range;

use gpui_kit::base::text::editor_host::RenderContext;
use gpui_kit::component::input::TextDecoration;
use gpui_kit::{
    App, Context, FontStyle, FontWeight, HighlightStyle, Hsla, StrikethroughStyle, UnderlineStyle,
    px,
};
use markdown::ParseOptions;
use markdown::mdast::Node;

use super::ChatComposer;

/// How faint a mark is: as faint as muted text is on the glass.
const MARK_FADE: f32 = 0.55;
/// Past this many bytes what's typed shows plain: parsed again for each key, it would be felt.
const MOST_STYLED: usize = 32 * 1024;

/// How a span of what's typed reads. Each sets a side of the style of its own, so spans that
/// overlap, a mark in a link or bold in a quote, add up rather than fight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Look {
    /// What makes the markdown, dimmed: `**`, `#`, a fence, `> `, a list's bullet, a table's pipes.
    Mark,
    /// Strong, a heading or a table's head.
    Bold,
    /// Emphasis, or a quote.
    Italic,
    Struck,
    /// Code in a line.
    Code,
    /// A code block's lines.
    Block,
    Link,
}

/// The colours the text views draw what markdown sets apart in, which what's typed takes.
#[derive(Clone, Copy, Debug)]
pub(super) struct Colours {
    link: Hsla,
    code: Hsla,
    block: Hsla,
}

impl Colours {
    pub(super) fn of(cx: &App) -> Self {
        let context = RenderContext::from_app(cx);
        let style = context.style();
        Colours {
            link: style.link(),
            code: (style.inline_code().background_color).unwrap_or(style.code_background()),
            block: style.code_background(),
        }
    }
}

impl ChatComposer {
    /// Lays the markdown of what's typed over it, once for each text: the textarea says it changed
    /// for each blink of its caret too.
    pub(super) fn style_text(&mut self, cx: &mut Context<Self>) {
        let text = self.text.read(cx);
        let version = text.version();
        if self.styled == Some(version) {
            return;
        }
        self.styled = Some(version);
        let decorations = if text.text().len() <= MOST_STYLED {
            decorations(&text.text().to_string(), Colours::of(cx))
        } else {
            Vec::new()
        };
        (self.text).update(cx, |text, cx| text.set_decorations(decorations, cx));
    }

    /// The text views' colours changed with the theme: what's typed is drawn in them again.
    pub(super) fn restyle(&mut self, cx: &mut Context<Self>) {
        self.styled = None;
        self.style_text(cx);
    }
}

/// The styles of `text`'s markdown, in `colours`.
pub(super) fn decorations(text: &str, colours: Colours) -> Vec<TextDecoration> {
    let style = |look| match look {
        Look::Mark => HighlightStyle {
            fade_out: Some(MARK_FADE),
            ..HighlightStyle::default()
        },
        Look::Bold => FontWeight::BOLD.into(),
        Look::Italic => FontStyle::Italic.into(),
        Look::Struck => HighlightStyle {
            strikethrough: Some(StrikethroughStyle {
                thickness: px(1.),
                color: None,
            }),
            ..HighlightStyle::default()
        },
        Look::Code => HighlightStyle {
            background_color: Some(colours.code),
            ..HighlightStyle::default()
        },
        Look::Block => HighlightStyle {
            background_color: Some(colours.block),
            ..HighlightStyle::default()
        },
        Look::Link => HighlightStyle {
            color: Some(colours.link),
            underline: Some(UnderlineStyle {
                thickness: px(1.),
                ..UnderlineStyle::default()
            }),
            ..HighlightStyle::default()
        },
    };
    (spans(text).into_iter())
        .map(|(range, look)| TextDecoration::new(range, style(look)))
        .collect()
}

/// The spans of `text` its markdown sets apart, and how each reads, as the text views read it.
pub(super) fn spans(text: &str) -> Vec<(Range<usize>, Look)> {
    let Ok(root) = markdown::to_mdast(text, &ParseOptions::gfm()) else {
        return Vec::new();
    };
    let mut spans = Spans(Vec::new());
    spans.walk(&root, text);
    spans.0.retain(|(range, _)| !range.is_empty());
    spans.0
}

struct Spans(Vec<(Range<usize>, Look)>);

impl Spans {
    fn walk(&mut self, node: &Node, text: &str) {
        if let Some(range) = node.position().map(|at| at.start.offset..at.end.offset) {
            match node {
                Node::Heading(_) | Node::Strong(_) => self.wrapped(node, range, Look::Bold),
                Node::Emphasis(_) => self.wrapped(node, range, Look::Italic),
                Node::Delete(_) => self.wrapped(node, range, Look::Struck),
                Node::Link(_) => self.wrapped(node, range, Look::Link),
                Node::InlineCode(_) => self.code(text, range),
                Node::Code(_) => self.block(text, range),
                Node::ListItem(_) => {
                    let words = child(node, <[Node]>::first).map_or(range.end, |words| words.start);
                    self.0.push((range.start..words, Look::Mark));
                }
                Node::Blockquote(_) => self.quote(text, range),
                Node::Table(_) => self.table(text, range),
                Node::ThematicBreak(_) => self.0.push((range, Look::Mark)),
                _ => {}
            }
        }
        for child in node.children().into_iter().flatten() {
            self.walk(child, text);
        }
    }

    /// What's between a construct's marks, which `look`s as it says, and the marks round it.
    fn wrapped(&mut self, node: &Node, range: Range<usize>, look: Look) {
        let words = child(node, <[Node]>::first)
            .zip(child(node, <[Node]>::last))
            .map_or(range.end..range.end, |(first, last)| first.start..last.end);
        self.0.push((range.start..words.start, Look::Mark));
        self.0.push((words.end..range.end, Look::Mark));
        self.0.push((words, look));
    }

    /// Code in a line, its backticks dimmed.
    fn code(&mut self, text: &str, range: Range<usize>) {
        let code = &text[range.clone()];
        let opening = code.bytes().take_while(|byte| *byte == b'`').count();
        let closing = code.bytes().rev().take_while(|byte| *byte == b'`').count();
        self.0
            .push((range.start..range.start + opening, Look::Mark));
        self.0.push((
            range.end - closing.min(code.len() - opening)..range.end,
            Look::Mark,
        ));
        self.0.push((range, Look::Code));
    }

    /// A code block: a fenced one's fences, the language with the first, dimmed, and its lines.
    fn block(&mut self, text: &str, range: Range<usize>) {
        let block = &text[range.clone()];
        let fence: String = (block.trim_start().chars())
            .take_while(|c| *c == '`' || *c == '~')
            .collect();
        if fence.len() < 3 {
            self.0.push((range, Look::Block));
            return;
        }
        let opened = block.find('\n').map_or(range.end, |at| range.start + at);
        let last = block
            .rfind('\n')
            .map_or(range.end, |at| range.start + at + 1);
        let closed = last > opened && text[last..range.end].trim_start().starts_with(&fence);
        let lines_end = if closed { last - 1 } else { range.end };
        self.0.push((range.start..opened, Look::Mark));
        self.0
            .push(((opened + 1).min(lines_end)..lines_end, Look::Block));
        if closed {
            self.0.push((last..range.end, Look::Mark));
        }
    }

    /// A quote in italics, the `>` that start its lines dimmed.
    fn quote(&mut self, text: &str, range: Range<usize>) {
        for (at, line) in lines(text, range.clone()) {
            let mut marked = 0;
            for (offset, c) in line.char_indices() {
                match c {
                    '>' => marked = offset + 1,
                    ' ' | '\t' => {}
                    _ => break,
                }
            }
            if marked > 0 {
                let spaced = marked + usize::from(line[marked..].starts_with(' '));
                self.0.push((at..at + spaced, Look::Mark));
            }
        }
        self.0.push((range, Look::Italic));
    }

    /// A table: its head bold, the line under it and the pipes between its cells dimmed.
    fn table(&mut self, text: &str, range: Range<usize>) {
        for (row, (at, line)) in lines(text, range).enumerate() {
            match row {
                0 => self.0.push((at..at + line.len(), Look::Bold)),
                1 => self.0.push((at..at + line.len(), Look::Mark)),
                _ => {}
            }
            let pipes = line.match_indices('|').map(|(offset, _)| offset);
            let unescaped = pipes.filter(|offset| !line[..*offset].ends_with('\\'));
            (self.0).extend(unescaped.map(|offset| (at + offset..at + offset + 1, Look::Mark)));
        }
    }
}

/// The lines of `text` in `range`, each with where it starts, its first starting where `range` does.
fn lines(text: &str, range: Range<usize>) -> impl Iterator<Item = (usize, &str)> {
    let start = range.start;
    text[range].split('\n').scan(start, |at, line| {
        let from = *at;
        *at += line.len() + 1;
        Some((from, line))
    })
}

/// Where the child of `node` that `which` picks, its first or its last, is in the text.
fn child(node: &Node, which: fn(&[Node]) -> Option<&Node>) -> Option<Range<usize>> {
    let at = which(node.children()?)?.position()?;
    Some(at.start.offset..at.end.offset)
}
