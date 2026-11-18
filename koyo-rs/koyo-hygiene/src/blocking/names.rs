use std::collections::BTreeMap;

use crate::shipped::Token;

use super::calls::{blocks, named};

/// What the file's `use` items name: each name with the path it stands for, where each item
/// starts and the token after it, the globs that import from `std::fs` or `std::process`, and
/// what a `pub use` hands on that blocks, each by the index of its token.
pub struct Names {
    paths: BTreeMap<String, Vec<String>>,
    pub declarations: BTreeMap<usize, usize>,
    pub globs: Vec<(usize, String)>,
    pub exported: Vec<(usize, String)>,
}

impl Names {
    pub fn of(tokens: &[Token<'_>]) -> Self {
        let mut names = Names {
            paths: BTreeMap::new(),
            declarations: BTreeMap::new(),
            globs: Vec::new(),
            exported: Vec::new(),
        };
        for (index, _) in tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.kind == "use")
        {
            let mut tree = Tree {
                tokens,
                named: Vec::new(),
                globs: Vec::new(),
            };
            if let Some(end) = tree.read(index + 1, &[])
                && tokens.get(end).is_some_and(|after| after.kind == ";")
            {
                names.declarations.insert(index, end + 1);
                if public(tokens, index) {
                    names.exported.extend(
                        (tree.named.iter())
                            .filter(|(_, path, _)| blocks(path))
                            .map(|(_, path, at)| (*at, path.join("::"))),
                    );
                }
                names
                    .paths
                    .extend(tree.named.into_iter().map(|(name, path, _)| (name, path)));
                names.globs.extend(tree.globs);
            }
        }
        names
    }

    /// The path `segments` stand for, its first segment read through the file's `use` items.
    pub fn resolve(&self, segments: &[&str]) -> Vec<String> {
        let Some((first, rest)) = segments.split_first() else {
            return Vec::new();
        };
        let mut path = match self.paths.get(*first) {
            Some(named) => named.clone(),
            None => vec![(*first).to_owned()],
        };
        path.extend(rest.iter().map(|segment| (*segment).to_owned()));
        path
    }
}

/// Whether the `use` at `index` is public: `pub use`, or `pub(crate) use` and the like.
fn public(tokens: &[Token<'_>], index: usize) -> bool {
    let before = |at: usize| {
        at.checked_sub(1)
            .and_then(|at| tokens.get(at).map(|token| (at, token)))
    };
    match before(index) {
        Some((_, token)) if token.kind == "pub" => true,
        Some((mut at, token)) if token.kind == ")" => {
            while let Some((earlier, token)) = before(at) {
                at = earlier;
                if token.kind == "(" {
                    return before(at).is_some_and(|(_, token)| token.kind == "pub");
                }
            }
            false
        }
        _ => false,
    }
}

/// One `use` item's tree, read from the token after `use`.
struct Tree<'tokens, 'source> {
    tokens: &'tokens [Token<'source>],
    /// Each name with the path it stands for and the index of the token that names it.
    named: Vec<(String, Vec<String>, usize)>,
    globs: Vec<(usize, String)>,
}

impl Tree<'_, '_> {
    /// Reads the tree at `index` under `prefix`, and returns the index after it, or `None`
    /// where there's no tree, as after the `use` of `impl Trait + use<'a>`.
    fn read(&mut self, mut index: usize, prefix: &[String]) -> Option<usize> {
        let mut path = prefix.to_vec();
        loop {
            let token = self.tokens.get(index)?;
            match token.kind {
                "{" => return self.list(index + 1, &path),
                "*" => {
                    let words: Vec<&str> = path.iter().map(String::as_str).collect();
                    if matches!(words.as_slice(), ["std", "fs" | "process", ..]) {
                        self.globs.push((index, format!("{}::*", path.join("::"))));
                    }
                    return Some(index + 1);
                }
                _ if named(token) => {
                    if token.kind != "self" || path.is_empty() {
                        path.push(token.text.to_owned());
                    }
                    index += 1;
                    if self.tokens.get(index).is_some_and(|next| next.kind == "::") {
                        index += 1;
                    } else {
                        return Some(self.name(index, path));
                    }
                }
                _ => return None,
            }
        }
    }

    /// Names `path` by its last segment, or by what an `as` at `index` names it, and returns
    /// the index after.
    fn name(&mut self, index: usize, path: Vec<String>) -> usize {
        let renamed = self
            .tokens
            .get(index)
            .is_some_and(|token| token.kind == "as");
        let (name, after) = if renamed {
            let alias = self
                .tokens
                .get(index + 1)
                .map(|token| token.text.to_owned());
            (alias, index + 2)
        } else {
            (path.last().cloned(), index)
        };
        if let Some(name) = name.filter(|name| name != "_") {
            self.named.push((name, path, index - 1));
        }
        after
    }

    /// The elements of the list opening before `index` under `prefix`, and the index after
    /// its `}`.
    fn list(&mut self, mut index: usize, prefix: &[String]) -> Option<usize> {
        loop {
            match self.tokens.get(index)?.kind {
                "}" => return Some(index + 1),
                "," => index += 1,
                _ => index = self.read(index, prefix)?,
            }
        }
    }
}
