use std::ops::{Deref, Range};

use gpui::SharedString;
use ropey::Rope;

use crate::input::edits::translate_range;
use crate::input::{Position, RopeExt as _};

pub type DiagnosticRelatedInformation = lsp_types::DiagnosticRelatedInformation;
pub(super) type CodeDescription = lsp_types::CodeDescription;
pub type RelatedInformation = lsp_types::DiagnosticRelatedInformation;
pub type DiagnosticTag = lsp_types::DiagnosticTag;

#[derive(Debug, Eq, PartialEq, Clone, Default)]
pub struct Diagnostic {
    /// The range [`Position`] at which the message applies, as published:
    /// what [`DiagnosticSet::push`] converts to bytes. The entry's byte
    /// range is the one that follows edits.
    pub range: Range<Position>,

    /// The diagnostic's severity. Can be omitted. If omitted it is up to the
    /// client to interpret diagnostics as error, warning, info or hint.
    pub severity: DiagnosticSeverity,

    /// The diagnostic's code. Can be omitted.
    pub code: Option<SharedString>,

    pub code_description: Option<CodeDescription>,

    /// A human-readable string describing the source of this
    /// diagnostic, e.g. 'typescript' or 'super lint'.
    pub source: Option<SharedString>,

    /// The diagnostic's message.
    pub message: SharedString,

    /// An array of related diagnostic information, e.g. when symbol-names within
    /// a scope collide all definitions can be marked via this property.
    pub related_information: Option<Vec<DiagnosticRelatedInformation>>,

    /// Additional metadata about the diagnostic.
    pub tags: Option<Vec<DiagnosticTag>>,

    /// A data entry field that is preserved between a `textDocument/publishDiagnostics`
    /// notification and `textDocument/codeAction` request.
    ///
    /// @since 3.16.0
    pub data: Option<serde_json::Value>,
}

impl From<lsp_types::Diagnostic> for Diagnostic {
    fn from(value: lsp_types::Diagnostic) -> Self {
        Self {
            range: value.range.start..value.range.end,
            severity: value
                .severity
                .map(Into::into)
                .unwrap_or(DiagnosticSeverity::Info),
            code: value.code.map(|c| match c {
                lsp_types::NumberOrString::Number(n) => SharedString::from(n.to_string()),
                lsp_types::NumberOrString::String(s) => SharedString::from(s),
            }),
            code_description: value.code_description,
            source: value.source.map(|s| s.into()),
            message: value.message.into(),
            related_information: value.related_information,
            tags: value.tags,
            data: value.data,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiagnosticSeverity {
    #[default]
    Hint,
    Error,
    Warning,
    Info,
}

impl From<lsp_types::DiagnosticSeverity> for DiagnosticSeverity {
    fn from(value: lsp_types::DiagnosticSeverity) -> Self {
        match value {
            lsp_types::DiagnosticSeverity::ERROR => Self::Error,
            lsp_types::DiagnosticSeverity::WARNING => Self::Warning,
            lsp_types::DiagnosticSeverity::INFORMATION => Self::Info,
            lsp_types::DiagnosticSeverity::HINT => Self::Hint,
            _ => Self::Info, // Default to Info if unknown
        }
    }
}

impl Diagnostic {
    pub fn new(range: Range<impl Into<Position>>, message: impl Into<SharedString>) -> Self {
        Self {
            range: range.start.into()..range.end.into(),
            message: message.into(),
            ..Default::default()
        }
    }

    pub fn with_severity(mut self, severity: impl Into<DiagnosticSeverity>) -> Self {
        self.severity = severity.into();
        self
    }

    pub fn with_code(mut self, code: impl Into<SharedString>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_source(mut self, source: impl Into<SharedString>) -> Self {
        self.source = Some(source.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiagnosticEntry {
    /// The byte range of the diagnostic in the rope.
    pub range: Range<usize>,
    pub diagnostic: Diagnostic,
}

impl Deref for DiagnosticEntry {
    type Target = Diagnostic;

    fn deref(&self) -> &Self::Target {
        &self.diagnostic
    }
}

#[derive(Debug, Clone)]
pub struct DiagnosticSet {
    text: Rope,
    /// In the order of their ranges.
    diagnostics: Vec<DiagnosticEntry>,
}

impl DiagnosticSet {
    pub fn new(text: &Rope) -> Self {
        Self {
            text: text.clone(),
            diagnostics: Vec::new(),
        }
    }

    pub fn reset(&mut self, text: &Rope) {
        self.text = text.clone();
        self.clear();
    }

    /// The bytes `old` of the text were replaced by `new_len` bytes, making
    /// `text`: the diagnostics' ranges follow the edit, as
    /// [`translate_range`] has them, until the server sends diagnostics for
    /// the new text. One whose bytes were all replaced is about text that is
    /// gone, and goes with it.
    pub fn edited(&mut self, old: &Range<usize>, new_len: usize, text: &Rope) {
        self.text = text.clone();
        self.diagnostics.retain_mut(|entry| {
            let replaced =
                !old.is_empty() && old.start <= entry.range.start && entry.range.end <= old.end;
            if !replaced {
                entry.range = translate_range(entry.range.clone(), old, new_len);
            }
            !replaced
        });
    }

    /// Adds a diagnostic at the byte `range` of the text, clipped to it, for
    /// a caller that has the range already; diagnostics are added in the
    /// order of their ranges.
    pub fn push_range(&mut self, range: Range<usize>, diagnostic: impl Into<Diagnostic>) {
        let range = self.text.clip_range(range);
        self.diagnostics.push(DiagnosticEntry {
            range,
            diagnostic: diagnostic.into(),
        });
    }

    pub fn push(&mut self, diagnostic: impl Into<Diagnostic>) {
        let diagnostic = diagnostic.into();
        let start = self.text.position_to_offset(&diagnostic.range.start);
        let end = self.text.position_to_offset(&diagnostic.range.end);
        self.push_range(start..end, diagnostic);
    }

    pub fn extend<D, I>(&mut self, diagnostics: D)
    where
        D: IntoIterator<Item = I>,
        I: Into<Diagnostic>,
    {
        for diagnostic in diagnostics {
            self.push(diagnostic.into());
        }
    }

    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    pub fn clear(&mut self) {
        self.diagnostics.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// The diagnostics reaching `range`, in order: from the first ending at
    /// or past its start, while they start before its end.
    pub fn range(&self, range: Range<usize>) -> impl Iterator<Item = &DiagnosticEntry> {
        let first = self
            .diagnostics
            .partition_point(|entry| entry.range.end < range.start);
        self.diagnostics[first..]
            .iter()
            .take_while(move |entry| entry.range.start < range.end)
    }

    pub fn for_offset(&self, offset: usize) -> Option<&DiagnosticEntry> {
        self.range(offset..offset + 1).next()
    }

    #[allow(unused)]
    pub fn iter(&self) -> impl Iterator<Item = &DiagnosticEntry> {
        self.diagnostics.iter()
    }
}

#[cfg(test)]
mod tests {
    use crate::input::{Edit, Position};

    use super::*;

    fn at(range: Range<Position>, message: &str) -> Diagnostic {
        Diagnostic {
            range,
            message: message.into(),
            ..Default::default()
        }
    }

    #[test]
    fn diagnostics_follow_the_edits_made_around_them() {
        let mut text = Rope::from("let x = 1;\nlet y = x;\n");
        let mut set = DiagnosticSet::new(&text);
        set.extend([
            at(Position::new(0, 4)..Position::new(0, 5), "x"),
            at(Position::new(1, 8)..Position::new(1, 9), "use"),
        ]);
        let ranges = |set: &DiagnosticSet| -> Vec<Range<usize>> {
            set.iter().map(|entry| entry.range.clone()).collect()
        };
        let mut apply = |set: &mut DiagnosticSet, edit: Edit| {
            text.replace(edit.old.clone(), &edit.new_text);
            set.edited(&edit.old, edit.new_len(), &text);
        };
        assert_eq!(ranges(&set), [4..5, 19..20]);
        // Typing before both moves both; typing at the first's end grows it.
        apply(&mut set, Edit::new(0..0, "  "));
        assert_eq!(ranges(&set), [6..7, 21..22]);
        apply(&mut set, Edit::new(7..7, "y"));
        assert_eq!(ranges(&set), [6..8, 22..23]);
        assert_eq!(
            set.for_offset(7).map(|entry| entry.message.as_ref()),
            Some("x")
        );
        // Deleting the second's text takes it along; the first follows a
        // replacement that only reaches into it.
        apply(&mut set, Edit::new(22..23, ""));
        assert_eq!(ranges(&set), [6..8]);
        apply(&mut set, Edit::new(7..10, "zz"));
        assert_eq!(ranges(&set), [6..9]);
        // Pushed by byte range, a diagnostic needs no conversion, only
        // clipping.
        set.push_range(
            30..40,
            at(Position::new(1, 9)..Position::new(1, 10), "late"),
        );
        assert_eq!(ranges(&set), [6..9, 23..23]);
    }

    #[test]
    fn test_diagnostic() {
        use ropey::Rope;

        use super::{Diagnostic, DiagnosticSet, DiagnosticSeverity};

        let text = Rope::from("Hello, 你好warld!\nThis is a test.\nGoodbye, world!");
        let mut diagnostics = DiagnosticSet::new(&text);

        diagnostics.push(
            Diagnostic::new(
                Position::new(0, 7)..Position::new(0, 17),
                "Spelling mistake",
            )
            .with_severity(DiagnosticSeverity::Warning),
        );
        diagnostics.push(
            Diagnostic::new(Position::new(2, 9)..Position::new(2, 14), "Syntax error")
                .with_severity(DiagnosticSeverity::Error),
        );

        assert_eq!(diagnostics.len(), 2);
        let items = diagnostics.iter().collect::<Vec<_>>();

        assert_eq!(items[0].message.as_str(), "Spelling mistake");
        assert_eq!(items[0].range, 7..19);

        assert_eq!(items[1].message.as_str(), "Syntax error");
        assert_eq!(items[1].range, 45..50);

        let items = diagnostics.range(6..48).collect::<Vec<_>>();
        assert_eq!(items.len(), 2);

        let item = diagnostics.for_offset(10).unwrap();
        assert_eq!(item.message.as_str(), "Spelling mistake");

        let item = diagnostics.for_offset(30);
        assert!(item.is_none());

        let item = diagnostics.for_offset(46).unwrap();
        assert_eq!(item.message.as_str(), "Syntax error");

        diagnostics.push(
            Diagnostic::new(Position::new(1, 5)..Position::new(1, 7), "Info message")
                .with_severity(DiagnosticSeverity::Info),
        );
        assert_eq!(diagnostics.len(), 3);

        diagnostics.clear();
        assert_eq!(diagnostics.len(), 0);
    }
}
