//! How a list's rows are ranked and matched against the word typed: by the match, then the row
//! the server preselected, then its sort text; matched on a row's filter text, painted on its
//! label; and a list kept for a longer word, moved past what was typed, unless it's incomplete.

use super::*;

fn item(label: &str, sort_text: Option<&str>, preselect: bool) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        sort_text: sort_text.map(str::to_owned),
        preselect: Some(preselect),
        ..CompletionItem::default()
    }
}

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|item| item.label.as_str()).collect()
}

fn ranked(items: &[CompletionItem], query: &str) -> Vec<CompletionItem> {
    rank(
        items,
        query,
        &mut Matcher::new(Config::DEFAULT),
        &mut Vec::new(),
    )
}

#[test]
fn rows_are_ranked_by_match_then_preselection_then_sort_text() {
    let items = [
        item("other_thing", None, false),
        item("grab", None, false),
        item("greet", None, true),
        item("GREP_ALL", Some("0000"), false),
    ];
    assert_eq!(labels(&ranked(&items, "gr")), ["greet", "GREP_ALL", "grab"]);
    assert_eq!(labels(&ranked(&items, "grb")), ["grab"]);
    assert_eq!(labels(&ranked(&items, "zzz")), Vec::<&str>::new());
    // Nothing typed: the server's order, the preselected row first.
    assert_eq!(
        labels(&ranked(&items, "")),
        ["greet", "GREP_ALL", "grab", "other_thing"]
    );
}

#[test]
fn a_row_is_matched_by_its_filter_text_and_painted_on_its_label() {
    let mut full = item("greet(…)", None, false);
    full.filter_text = Some("greet".to_owned());
    assert_eq!(labels(&ranked(&[full.clone()], "gre")), ["greet(…)"]);
    let highlight = highlighter();
    assert_eq!(highlight("gre", &full), vec![0..3]);
    assert_eq!(highlight("gt", &full), vec![0..1, 4..5]);
    assert_eq!(highlight("", &full), Vec::<std::ops::Range<usize>>::new());
}

#[test]
fn a_list_serves_longer_words_moved_past_what_was_typed_unless_incomplete() {
    let edit = |start: u32, end: u32| lsp_types::TextEdit {
        range: lsp_types::Range::new(Position::new(1, start), Position::new(1, end)),
        new_text: "greet()".to_owned(),
    };
    let mut greet = item("greet", None, false);
    greet.text_edit = Some(CompletionTextEdit::Edit(edit(4, 5)));
    greet.additional_text_edits = Some(vec![
        lsp_types::TextEdit::new(
            lsp_types::Range::new(Position::new(0, 0), Position::new(0, 0)),
            "use g;\n".into(),
        ),
        edit(9, 9),
    ]);
    let mut completions = Completions::default();
    assert!(completions.ranked(4, "g").is_none(), "nothing listed yet");
    completions.keep(
        4,
        Position::new(1, 5),
        "g".to_owned(),
        false,
        vec![greet, item("grab", None, false)],
    );
    let served = completions.ranked(4, "gre").unwrap();
    assert_eq!(labels(&served), ["greet"]);
    // The word's edit ends at the caret, which the "re" typed since moved; so did an
    // edit after it on the line, but not one on another line.
    let Some(CompletionTextEdit::Edit(text_edit)) = &served[0].text_edit else {
        panic!("the edit as given");
    };
    assert_eq!(
        text_edit.range,
        lsp_types::Range::new(Position::new(1, 4), Position::new(1, 7))
    );
    let additional = served[0].additional_text_edits.as_ref().unwrap();
    assert_eq!(additional[0].range.end, Position::new(0, 0));
    assert_eq!(
        additional[1].range,
        lsp_types::Range::new(Position::new(1, 11), Position::new(1, 11))
    );
    assert!(completions.ranked(4, "a").is_none(), "another word");
    assert!(completions.ranked(5, "gre").is_none(), "another start");
    // A row filled in stays so, brought up to the word.
    let mut filled = item("greet", None, false);
    filled.detail = Some("fn greet()".to_owned());
    filled.text_edit = Some(CompletionTextEdit::Edit(edit(4, 5)));
    assert!(completions.filled(&filled).is_none());
    let back = completions.fill(filled.clone());
    assert_eq!(back.detail.as_deref(), Some("fn greet()"));
    let kept = completions.filled(&filled).unwrap();
    let Some(CompletionTextEdit::Edit(text_edit)) = &kept.text_edit else {
        panic!("the edit as given");
    };
    assert_eq!(text_edit.range.end, Position::new(1, 7));
    assert_eq!(
        completions.ranked(4, "gree").unwrap()[0].detail.as_deref(),
        Some("fn greet()")
    );
    let mut incomplete = Completions::default();
    incomplete.keep(4, Position::new(1, 5), "g".to_owned(), true, Vec::new());
    assert!(
        incomplete.ranked(4, "gre").is_none(),
        "incomplete: asked again"
    );
    assert!(incomplete.incomplete_at(4));
}
