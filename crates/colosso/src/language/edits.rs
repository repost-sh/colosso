//! A workspace edit applied: this file's edits in the editor, as one undo step, carried
//! past whatever was typed since the server listed them; another file's on disk, which
//! the view showing it, if any, reads again, unless it has unsaved edits of its own.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::{fs, io};

use colosso_lsp::lsp_types::{
    self, AnnotatedTextEdit, DocumentChangeOperation, DocumentChanges, OneOf, TextEdit, Uri,
    WorkspaceEdit,
};
use colosso_lsp::{Encoding, IndexedText, uri};
use gpui_kit::component::input::EditorState;
use gpui_kit::{App, WeakEntity, Window};

use super::providers::Asked;
use crate::checkout::Checkout;

/// Applies `edit` to `path` through `editor`, which shows it as `asked` knows it, and
/// says which other files get which edits, on disk; none when one of them has unsaved
/// edits in `checkout`, so an edit lands whole or not at all.
pub(super) fn apply(
    edit: WorkspaceEdit,
    asked: &Asked,
    path: &Path,
    editor: &WeakEntity<EditorState>,
    checkout: &WeakEntity<Checkout>,
    window: &mut Window,
    cx: &mut App,
) -> Result<Vec<(PathBuf, Vec<TextEdit>)>, String> {
    let mut here = Vec::new();
    let mut elsewhere = Vec::new();
    for (uri, edits) in per_file(edit)? {
        let Some(file) = uri::to_path(&uri) else {
            return Err(format!("{} isn't a file", uri.as_str()));
        };
        if file == path {
            here.extend(edits);
            continue;
        }
        let dirty = (checkout.upgrade())
            .and_then(|checkout| checkout.read(cx).open_document(&file))
            .is_some_and(|document| document.read(cx).is_dirty());
        if dirty {
            return Err(format!("{} has unsaved edits", file.display()));
        }
        elsewhere.push((file, edits));
    }
    if !here.is_empty() {
        let version = asked.version();
        let applied = editor.update(cx, |editor, cx| {
            // Carried past what was typed since, while the editor still has those edits.
            let edits: Option<Vec<(Range<usize>, &str)>> =
                (byte_edits(&here, |range| asked.bytes(range)).into_iter())
                    .map(|(range, text)| Some((editor.translate_range(range, version)?, text)))
                    .collect();
            let edits = edits
                .filter(|edits| disjoint(edits))
                .ok_or_else(|| "the text changed too much meanwhile".to_owned())?;
            editor.edit(&edits, window, cx);
            Ok::<(), String>(())
        });
        applied.map_err(|error| format!("the editor is gone: {error}"))??;
    }
    Ok(elsewhere)
}

/// Every file's edits, in the order given. A document created, renamed or deleted isn't
/// done yet, and the edit is refused whole rather than done in part.
fn per_file(edit: WorkspaceEdit) -> Result<Vec<(Uri, Vec<TextEdit>)>, String> {
    let mut files = Vec::new();
    match edit.document_changes {
        Some(DocumentChanges::Edits(edits)) => {
            files.extend((edits.into_iter()).map(|edit| (edit.text_document.uri, plain(edit.edits))));
        }
        Some(DocumentChanges::Operations(operations)) => {
            for operation in operations {
                match operation {
                    DocumentChangeOperation::Edit(edit) => {
                        files.push((edit.text_document.uri, plain(edit.edits)));
                    }
                    DocumentChangeOperation::Op(_) => {
                        return Err("it creates, renames or deletes a file, which isn't done yet".to_owned());
                    }
                }
            }
        }
        None => {
            files.extend(edit.changes.into_iter().flatten());
            files.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        }
    }
    Ok(files)
}

/// The edits, their annotations dropped.
fn plain(edits: Vec<OneOf<TextEdit, AnnotatedTextEdit>>) -> Vec<TextEdit> {
    (edits.into_iter())
        .map(|edit| match edit {
            OneOf::Left(edit) => edit,
            OneOf::Right(annotated) => annotated.text_edit,
        })
        .collect()
}

/// `edits` as byte edits, by `bytes`, in ascending order; two at one place stay in the
/// order given.
fn byte_edits(
    edits: &[TextEdit],
    bytes: impl Fn(lsp_types::Range) -> Range<usize>,
) -> Vec<(Range<usize>, &str)> {
    let mut edits: Vec<_> = (edits.iter())
        .map(|edit| (bytes(edit.range), edit.new_text.as_str()))
        .collect();
    edits.sort_by_key(|(range, _)| (range.start, range.end));
    edits
}

/// Whether `edits`, ascending, leave the text between them as it is.
fn disjoint(edits: &[(Range<usize>, &str)]) -> bool {
    (edits.windows(2)).all(|pair| pair[0].0.end <= pair[1].0.start)
}

/// Puts `edits`, in `encoding`, into the file at `path`.
pub(super) fn on_disk(path: &Path, edits: &[TextEdit], encoding: Encoding) -> io::Result<()> {
    let indexed = IndexedText::new(fs::read_to_string(path)?);
    let text = indexed.as_str();
    let mut edited = String::with_capacity(text.len());
    let mut at = 0;
    for (range, new_text) in byte_edits(edits, |range| indexed.offsets(range, encoding)) {
        // One beginning before the last ended overlaps it, which a server's don't: it
        // goes in where the last ended.
        let start = range.start.max(at);
        edited.push_str(&text[at..start]);
        edited.push_str(new_text);
        at = range.end.max(start);
    }
    edited.push_str(&text[at..]);
    fs::write(path, edited)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::scratch;
    use colosso_lsp::lsp_types::{Position, Range as LspRange};

    fn edit(start: (u32, u32), end: (u32, u32), text: &str) -> TextEdit {
        TextEdit {
            range: LspRange::new(Position::new(start.0, start.1), Position::new(end.0, end.1)),
            new_text: text.to_owned(),
        }
    }

    #[test]
    fn edits_on_disk_land_where_they_were_given_whatever_their_order() {
        let folder = scratch("edits-on-disk");
        let file = folder.join("other.rs");
        fs::write(&file, "fn a() {}\nfn b() { é }\n").unwrap();
        let edits = [
            edit((1, 9), (1, 10), "ü"),
            edit((0, 0), (0, 0), "use x;\n"),
            edit((0, 3), (0, 4), "aa"),
            edit((0, 0), (0, 0), "use y;\n"),
        ];
        on_disk(&file, &edits, Encoding::Utf16).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "use x;\nuse y;\nfn aa() {}\nfn b() { ü }\n"
        );
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_workspace_edit_lists_its_files_but_not_file_operations() {
        let uri: Uri = "file:///tmp/a.rs".parse().unwrap();
        let by_change = WorkspaceEdit {
            changes: Some([(uri.clone(), vec![edit((0, 0), (0, 0), "x")])].into()),
            ..WorkspaceEdit::default()
        };
        assert_eq!(per_file(by_change).unwrap().len(), 1);

        let by_operation = WorkspaceEdit {
            document_changes: Some(DocumentChanges::Operations(vec![
                DocumentChangeOperation::Op(lsp_types::ResourceOp::Create(
                    lsp_types::CreateFile {
                        uri,
                        options: None,
                        annotation_id: None,
                    },
                )),
            ])),
            ..WorkspaceEdit::default()
        };
        assert!(per_file(by_operation).is_err());
    }
}
