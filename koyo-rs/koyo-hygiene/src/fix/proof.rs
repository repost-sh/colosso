use std::ops::Range;

use tree_sitter::{Tree, TreeCursor};

use crate::language::{Language, Role};

pub fn same_code(
    language: Language,
    original: (&Tree, &str, &[Range<usize>]),
    rewritten: (&Tree, &str),
) -> bool {
    let (tree, source, doomed) = original;
    Tokens::new(language, tree, source, doomed).eq(Tokens::new(
        language,
        rewritten.0,
        rewritten.1,
        &[],
    ))
}

struct Tokens<'a> {
    language: Language,
    cursor: TreeCursor<'a>,
    source: &'a str,
    doomed: &'a [Range<usize>],
    finished: bool,
}

impl<'a> Tokens<'a> {
    fn new(
        language: Language,
        tree: &'a Tree,
        source: &'a str,
        doomed: &'a [Range<usize>],
    ) -> Self {
        Self {
            language,
            cursor: tree.walk(),
            source,
            doomed,
            finished: false,
        }
    }

    fn is_doomed(&self, start: usize) -> bool {
        self.doomed
            .binary_search_by_key(&start, |range| range.start)
            .is_ok()
    }

    fn advance(&mut self, descend: bool) {
        if descend && self.cursor.goto_first_child() {
            return;
        }
        while !self.cursor.goto_next_sibling() {
            if !self.cursor.goto_parent() {
                self.finished = true;
                return;
            }
        }
    }
}

impl<'a> Iterator for Tokens<'a> {
    type Item = (u16, u32, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        while !self.finished {
            let node = self.cursor.node();
            let doomed = self.language.role(node, self.source) == Role::PlainComment
                && self.is_doomed(node.start_byte());
            if doomed {
                self.advance(false);
                continue;
            }
            let depth = self.cursor.depth();
            let text = if node.child_count() == 0 {
                &self.source[node.byte_range()]
            } else {
                ""
            };
            self.advance(true);
            return Some((node.kind_id(), depth, text));
        }
        None
    }
}
