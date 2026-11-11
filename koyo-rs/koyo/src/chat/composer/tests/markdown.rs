//! What's typed formatted as markdown where it's typed: which spans each construct styles and
//! which it dims, that edits next to and inside its marks style it again, and that what's sent is
//! what was typed.

use gpui_kit::{AppContext as _, FontWeight, TestAppContext};

use super::{chat, type_in};
use crate::chat::composer::markdown::{Look, spans};
use crate::chat::composer::sending::sent;
use crate::chat::tests::asks::Asking;
use crate::test_support::draw;

/// What `text`'s markdown sets apart, as the words each span holds and how it reads, in order.
fn looks(text: &str) -> Vec<(&str, Look)> {
    let mut spans = spans(text);
    spans.sort_by_key(|(range, look)| (range.start, range.end, *look));
    (spans.into_iter())
        .map(|(range, look)| (&text[range], look))
        .collect()
}

#[test]
fn each_construct_styles_its_words_and_dims_its_marks() {
    use Look::{Block, Bold, Code, Italic, Link, Mark, Struck};
    assert_eq!(looks("## Title"), [("## ", Mark), ("Title", Bold)]);
    assert_eq!(
        looks("**bold**"),
        [("**", Mark), ("bold", Bold), ("**", Mark)]
    );
    assert_eq!(looks("*it*"), [("*", Mark), ("it", Italic), ("*", Mark)]);
    assert_eq!(looks("_it_"), [("_", Mark), ("it", Italic), ("_", Mark)]);
    assert_eq!(
        looks("~~gone~~"),
        [("~~", Mark), ("gone", Struck), ("~~", Mark)]
    );
    assert_eq!(
        looks("`code`"),
        [("`", Mark), ("`code`", Code), ("`", Mark)]
    );
    assert_eq!(
        looks("```rust\nfn main() {}\n```"),
        [("```rust", Mark), ("fn main() {}", Block), ("```", Mark)]
    );
    assert_eq!(looks("- one\n- two"), [("- ", Mark), ("- ", Mark)]);
    assert_eq!(looks("1. one"), [("1. ", Mark)]);
    assert_eq!(looks("- [ ] to do"), [("- [ ] ", Mark)]);
    assert_eq!(
        looks("[Koyo](https://koyo.dev)"),
        [("[", Mark), ("Koyo", Link), ("](https://koyo.dev)", Mark)]
    );
    assert_eq!(
        looks("> said\n> again"),
        [("> ", Mark), ("> said\n> again", Italic), ("> ", Mark)]
    );
}

#[test]
fn a_table_has_its_head_bold_and_its_rules_and_pipes_dimmed() {
    let table = "| a | b |\n|---|---|\n| 1 | 2 |";
    let found = looks(table);
    assert!(found.contains(&("| a | b |", Look::Bold)), "{found:?}");
    assert!(found.contains(&("|---|---|", Look::Mark)), "{found:?}");
    let pipes = (found.iter()).filter(|look| **look == ("|", Look::Mark));
    assert_eq!(pipes.count(), 9);
}

#[test]
fn what_makes_no_markdown_is_left_plain() {
    assert_eq!(looks("just words"), []);
    assert_eq!(looks("**not yet"), [], "an opening alone is no mark");
    assert_eq!(looks("2 * 3 * 4"), []);
}

#[test]
fn whats_sent_is_what_was_typed_but_the_blank_lines_round_it() {
    let typed = "# Plan\n\n- **one** and `two`\n\n> [docs](https://koyo.dev)";
    assert_eq!(sent(typed), typed);
    assert_eq!(sent(&format!("\n\n{typed}\n  \n")), typed);
    assert_eq!(
        sent("\n    indented code\n"),
        "    indented code",
        "its indent makes it code"
    );
}

/// The words of each of the composer's styles and whether it's a mark or bold, in order.
fn styled(chat: &Asking, cx: &TestAppContext) -> Vec<(String, &'static str)> {
    chat.composer.read_with(cx, |composer, cx| {
        let text = composer.text.read(cx);
        let typed = text.value();
        let mut found: Vec<_> = (text.decorations().iter())
            .map(|decoration| {
                let look = if decoration.style.fade_out.is_some() {
                    "mark"
                } else if decoration.style.font_weight == Some(FontWeight::BOLD) {
                    "bold"
                } else {
                    "other"
                };
                (decoration.range.clone(), look)
            })
            .collect();
        found.sort_by_key(|(range, _)| (range.start, range.end));
        (found.into_iter())
            .map(|(range, look)| (typed[range].to_owned(), look))
            .collect()
    })
}

fn caret_at(chat: &Asking, at: std::ops::Range<usize>, cx: &mut TestAppContext) {
    let text = chat
        .composer
        .read_with(cx, |composer, _| composer.text.clone());
    text.update(cx, |text, cx| text.set_selected_range(at, cx));
}

/// Types `typed` where the caret is, as the keyboard does.
fn typing(chat: &Asking, typed: &str, cx: &mut TestAppContext) {
    cx.simulate_input(chat.window, typed);
    draw(chat.window, cx);
}

fn owned(found: &[(&str, &'static str)]) -> Vec<(String, &'static str)> {
    (found.iter())
        .map(|(words, look)| ((*words).to_owned(), *look))
        .collect()
}

#[gpui_kit::test]
fn typing_styles_whats_typed_in_place_and_edits_by_its_marks_style_it_again(
    cx: &mut TestAppContext,
) {
    let chat = chat("composer-markdown", cx);
    typing(&chat, "**bold", cx);
    assert_eq!(styled(&chat, cx), [], "not bold until it's closed");

    typing(&chat, "**", cx);
    let bold = [("**", "mark"), ("bold", "bold"), ("**", "mark")];
    assert_eq!(styled(&chat, cx), owned(&bold));
    assert_eq!(chat.typed(cx), "**bold**", "the marks stay");

    // Inside the marks, the words grow; next to them, what's typed is plain.
    caret_at(&chat, 4..4, cx);
    typing(&chat, "ld bo", cx);
    caret_at(&chat, 13..13, cx);
    typing(&chat, " then", cx);
    let grown = [("**", "mark"), ("bold bold", "bold"), ("**", "mark")];
    assert_eq!(styled(&chat, cx), owned(&grown));
    assert_eq!(chat.typed(cx), "**bold bold** then");

    // A mark taken away undoes what it made.
    caret_at(&chat, 12..12, cx);
    chat.keys("backspace", cx);
    assert_eq!(chat.typed(cx), "**bold bold* then");
    assert!(
        !styled(&chat, cx).iter().any(|(_, look)| *look == "bold"),
        "{:?}",
        styled(&chat, cx)
    );
}

#[gpui_kit::test]
fn a_message_goes_as_the_markdown_that_was_typed(cx: &mut TestAppContext) {
    let chat = chat("composer-markdown-sent", cx);
    let typed = "## Plan\n\n1. **Read** `main.rs`\n2. [Docs](https://koyo.dev)\n\n> ~~not~~ this";
    type_in(&chat, typed, cx);
    assert!(!styled(&chat, cx).is_empty());

    // These chats have no host, so it doesn't go, and what it would have said is kept.
    let composer = chat.composer.clone();
    cx.update_window(chat.window, |_, window, cx| {
        composer.update(cx, |composer, cx| composer.send(window, cx));
    })
    .unwrap();
    draw(chat.window, cx);
    let said = chat.composer.read_with(cx, |composer, _| {
        composer.unsent.as_ref().map(|prompt| prompt.text.clone())
    });
    assert_eq!(said.as_deref(), Some(typed));
    assert_eq!(chat.typed(cx), typed, "and it's back as it was");
}
