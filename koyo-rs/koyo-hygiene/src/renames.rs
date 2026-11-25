use std::collections::BTreeMap;
use std::path::Path;

use crate::failure::Failure;
use crate::files::{SOURCES, git_on};
use crate::scratch::Scratch;

/// The files git finds renamed between the last commit and the working tree: each one's path at
/// the last commit by its path now, both in the checkout. A file the working tree still holds
/// wasn't renamed, whatever was copied from it, and one file is renamed to one.
pub struct Renames(BTreeMap<String, String>);

impl Renames {
    /// The renames as `git diff --find-renames` finds them, over the working tree as it is:
    /// staged, or not, or of a file git was never told of. The checkout's index isn't asked and
    /// doesn't change: git compares through one that holds the last commit's files and, as
    /// files it's told to expect, the others it would add.
    pub fn since_head(root: &Path) -> Result<Self, Failure> {
        let scratch = Scratch::new(root, "renames")?;
        let index = scratch.path("index");
        let git = |arguments: &[&str]| git_on(root, &index, arguments);
        git(&["read-tree", "HEAD"])?;
        let others = ["ls-files", "-z", "--others", "--exclude-standard", "--"];
        scratch.write("others", &git(&[others.as_slice(), &SOURCES].concat())?)?;
        git(&[
            "--literal-pathspecs",
            "add",
            "--intent-to-add",
            "--pathspec-file-nul",
            &format!("--pathspec-from-file={}", scratch.name("others")),
        ])?;
        let renamed = git(&[
            "diff-index",
            "-z",
            "--name-status",
            "--find-renames",
            "-l0",
            "--diff-filter=R",
            "HEAD",
        ])?;
        let words: Vec<_> = renamed
            .split(|&byte| byte == 0)
            .map(String::from_utf8_lossy)
            .collect();
        let (entries, _) = words.as_chunks();
        Ok(Self(
            entries
                .iter()
                .map(|[_status, before, now]| (now.to_string(), before.to_string()))
                .collect(),
        ))
    }

    /// The path at the last commit of the file renamed to `path`.
    pub fn before(&self, path: &str) -> Option<&str> {
        self.0.get(path).map(String::as_str)
    }
}
