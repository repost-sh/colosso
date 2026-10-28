//! The checkouts of a repository, as `git worktree list` reports them.

use std::path::PathBuf;

/// What a checkout has checked out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    /// A branch, by its short name. It may have no commits yet.
    Branch(String),
    /// A commit, by its full hash.
    Detached(String),
}

impl Head {
    pub fn branch(&self) -> Option<&str> {
        match self {
            Head::Branch(name) => Some(name),
            Head::Detached(_) => None,
        }
    }
}

/// A checkout of a repository: its main checkout or a linked worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Head,
    /// The checkout the repository was cloned or initialized into. Git can't remove it.
    pub is_main: bool,
}

/// Reads `git worktree list --porcelain -z`. Bare entries are left out, having nothing checked
/// out, and so are prunable ones, whose folder is gone. The first entry is the main checkout,
/// unless the repository is bare.
pub(crate) fn parse_list(output: &str) -> Vec<Worktree> {
    let mut worktrees = Vec::new();
    let mut record = Vec::new();
    let mut records = 0;
    // Each attribute ends in a NUL and an empty attribute ends each record.
    for field in output.split('\0') {
        if !field.is_empty() {
            record.push(field);
            continue;
        }
        if record.is_empty() {
            continue;
        }
        if let Some(worktree) = parse_record(&record, records == 0) {
            worktrees.push(worktree);
        }
        records += 1;
        record.clear();
    }
    worktrees
}

fn parse_record(record: &[&str], first: bool) -> Option<Worktree> {
    let mut path = None;
    let mut commit = None;
    let mut branch = None;
    for field in record {
        let (key, value) = field.split_once(' ').unwrap_or((field, ""));
        match key {
            "worktree" => path = Some(PathBuf::from(value)),
            "HEAD" => commit = Some(value),
            "branch" => branch = Some(value.strip_prefix("refs/heads/").unwrap_or(value)),
            "bare" | "prunable" => return None,
            _ => {}
        }
    }
    let head = match (branch, commit) {
        (Some(branch), _) => Head::Branch(branch.to_owned()),
        (None, Some(commit)) => Head::Detached(commit.to_owned()),
        (None, None) => return None,
    };
    Some(Worktree {
        path: path?,
        head,
        is_main: first,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn porcelain(records: &[&[&str]]) -> String {
        records
            .iter()
            .map(|fields| {
                fields
                    .iter()
                    .map(|field| format!("{field}\0"))
                    .collect::<String>()
                    + "\0"
            })
            .collect()
    }

    #[test]
    fn the_first_checkout_is_the_main_one() {
        let output = porcelain(&[
            &["worktree /src/app", "HEAD 1111", "branch refs/heads/main"],
            &[
                "worktree /work/app/feat",
                "HEAD 2222",
                "branch refs/heads/me/feat",
            ],
        ]);
        let worktrees = parse_list(&output);
        assert_eq!(worktrees.len(), 2);
        assert!(worktrees[0].is_main);
        assert_eq!(worktrees[0].head, Head::Branch("main".into()));
        assert!(!worktrees[1].is_main);
        assert_eq!(worktrees[1].path, PathBuf::from("/work/app/feat"));
        assert_eq!(worktrees[1].head.branch(), Some("me/feat"));
    }

    #[test]
    fn detached_checkouts_say_so_and_prunable_ones_are_left_out() {
        let output = porcelain(&[
            &["worktree /src/app", "HEAD 1111", "branch refs/heads/main"],
            &[
                "worktree /work/a",
                "HEAD 2222",
                "detached",
                "locked on a USB stick",
            ],
            &[
                "worktree /work/b",
                "HEAD 3333",
                "branch refs/heads/b",
                "prunable gitdir file points to non-existent location",
            ],
        ]);
        let worktrees = parse_list(&output);
        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[1].head, Head::Detached("2222".into()));
    }

    #[test]
    fn a_bare_repository_has_no_main_checkout() {
        let output = porcelain(&[
            &["worktree /src/app.git", "bare"],
            &["worktree /work/a", "HEAD 2222", "branch refs/heads/a"],
        ]);
        let worktrees = parse_list(&output);
        assert_eq!(worktrees.len(), 1);
        assert!(!worktrees[0].is_main);
    }

    #[test]
    fn paths_keep_their_spaces() {
        let output = porcelain(&[&[
            "worktree /Users/me/My Repos/app",
            "HEAD 1",
            "branch refs/heads/main",
        ]]);
        assert_eq!(
            parse_list(&output)[0].path,
            PathBuf::from("/Users/me/My Repos/app")
        );
    }
}
