//! Positions and document changes over text full of what positions get wrong: emoji (two
//! UTF-16 units, four bytes), joined emoji, CJK (one unit, three bytes), combining marks and
//! `\r\n`, checked over many random texts and edits in both encodings.

#![expect(
    clippy::unwrap_used,
    reason = "a test fails loudly at the step that broke"
)]

use std::ops::Range;
use std::str::FromStr;

use colosso_lsp::lsp_types::{Position, TextDocumentContentChangeEvent, Uri};
use colosso_lsp::{Document, EditError, Encoding, IndexedText, LineIndex};

const PIECES: [&str; 13] = [
    "a",
    "Z",
    " ",
    "é",
    "e\u{301}",
    "😀",
    "👨‍👩‍👧",
    "中",
    "文",
    "\n",
    "\r\n",
    "\t",
    "ß",
];
const ENCODINGS: [Encoding; 2] = [Encoding::Utf8, Encoding::Utf16];

/// A small deterministic generator, so a failure can be run again.
struct Random(u64);

impl Random {
    fn below(&mut self, bound: usize) -> usize {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize % bound.max(1)
    }

    fn text(&mut self, pieces: usize) -> String {
        (0..self.below(pieces))
            .map(|_| PIECES[self.below(PIECES.len())])
            .collect()
    }

    /// A character boundary of `text` that doesn't split a `\r\n`, which no position names.
    fn offset(&mut self, text: &str) -> usize {
        let offsets = boundaries(text);
        offsets[self.below(offsets.len())]
    }
}

fn boundaries(text: &str) -> Vec<usize> {
    (0..=text.len())
        .filter(|&at| text.is_char_boundary(at))
        .filter(|&at| !(text[..at].ends_with('\r') && text[at..].starts_with('\n')))
        .collect()
}

/// The position of `offset`, worked out the slow, obvious way.
fn reference(text: &str, offset: usize, encoding: Encoding) -> Position {
    let before = &text[..offset];
    let line = before.matches('\n').count();
    let column = &before[before.rfind('\n').map_or(0, |at| at + 1)..];
    let character = match encoding {
        Encoding::Utf8 => column.len(),
        Encoding::Utf16 => column.encode_utf16().count(),
    };
    Position::new(line as u32, character as u32)
}

#[test]
fn every_offset_has_the_obvious_position_and_back() {
    let mut random = Random(0x5EED);
    for _ in 0..300 {
        let text = random.text(40);
        let index = LineIndex::new(&text);
        assert_eq!(index.line_count(), text.matches('\n').count() + 1);
        for offset in boundaries(&text) {
            for encoding in ENCODINGS {
                let position = index.position(&text, offset, encoding);
                assert_eq!(
                    position,
                    reference(&text, offset, encoding),
                    "{text:?} @ {offset}"
                );
                assert_eq!(
                    index.offset(&text, position, encoding),
                    offset,
                    "{text:?} @ {position:?}"
                );
            }
        }
    }
}

#[test]
fn positions_between_characters_round_down_and_past_the_end_clamp() {
    let mut random = Random(0xC1A4);
    for _ in 0..300 {
        let text = random.text(30);
        let index = LineIndex::new(&text);
        for encoding in ENCODINGS {
            for line in 0..=index.line_count() as u32 {
                let mut last = None;
                for character in 0..40 {
                    let offset = index.offset(&text, Position::new(line, character), encoding);
                    assert!(text.is_char_boundary(offset));
                    // Offsets never go back as the column grows, and never leave the line.
                    if let Some(last) = last {
                        assert!(offset >= last, "{text:?} {line}:{character}");
                    }
                    assert!(!text[last.unwrap_or(offset)..offset].contains('\n'));
                    last = Some(offset);
                }
            }
        }
    }
}

/// What a server makes of the changes: each applied in turn, its range read in the text as
/// the changes before it left it.
fn applied(text: &str, changes: &[TextDocumentContentChangeEvent], encoding: Encoding) -> String {
    let mut text = text.to_owned();
    for change in changes {
        let range = change.range.unwrap();
        let index = LineIndex::new(&text);
        let start = index.offset(&text, range.start, encoding);
        let end = index.offset(&text, range.end, encoding);
        text.replace_range(start..end, &change.text);
    }
    text
}

#[test]
fn edits_keep_the_index_and_the_server_in_step() {
    let uri = Uri::from_str("file:///a.rs").unwrap();
    let mut random = Random(0xED17);
    for round in 0..400 {
        let original = random.text(40);
        for encoding in ENCODINGS {
            let mut text = IndexedText::new(original.clone());
            let mut edits = Vec::new();
            for _ in 0..=random.below(4) {
                let mut ends = [random.offset(text.as_str()), random.offset(text.as_str())];
                ends.sort_unstable();
                let new = random.text(6);
                edits.push(text.edit(ends[0]..ends[1], &new, encoding).unwrap());
                // Spliced, the index is the one made afresh.
                assert_eq!(
                    text.lines(),
                    &LineIndex::new(text.as_str()),
                    "round {round}: {:?}",
                    text.as_str()
                );
            }
            let mut document = Document::new(uri.clone());
            let change = document.change(edits);
            assert_eq!(change.text_document.version, 1);
            assert_eq!(
                applied(&original, &change.content_changes, encoding),
                text.as_str(),
                "round {round}, {encoding:?}: {original:?}"
            );
        }
    }
}

#[test]
fn edits_outside_the_text_are_refused() {
    let mut text = IndexedText::new("a😀b".to_owned());
    // Splitting the emoji, past the end, and backwards.
    let backwards = Range { start: 5, end: 1 };
    for range in [0..2, 0..9, backwards] {
        assert_eq!(
            text.edit(range.clone(), "", Encoding::Utf16),
            Err(EditError(range))
        );
    }
    assert_eq!(text.as_str(), "a😀b");
}

#[test]
fn whole_texts_are_sent_borrowed_as_the_next_version() {
    let uri = Uri::from_str("file:///a.rs").unwrap();
    let mut document = Document::new(uri);
    let opened = serde_json::to_value(document.open("rust", "old")).unwrap();
    assert_eq!(
        opened,
        serde_json::json!({"textDocument": {
            "uri": "file:///a.rs", "languageId": "rust", "version": 0, "text": "old",
        }})
    );
    let replaced = serde_json::to_value(document.replace("new")).unwrap();
    assert_eq!(
        replaced,
        serde_json::json!({
            "textDocument": {"uri": "file:///a.rs", "version": 1},
            "contentChanges": [{"text": "new"}],
        })
    );
    assert_eq!(document.version(), 1);
}
