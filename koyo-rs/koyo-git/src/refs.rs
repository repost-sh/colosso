//! Branches, local and remote-tracking, that a new worktree can start from.

use std::fmt;

/// A branch of this repository or a remote-tracking copy of one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ref {
    Local(String),
    /// A branch of `remote` as last fetched. A remote's name is taken to have no slash.
    Remote {
        remote: String,
        branch: String,
    },
}

impl Ref {
    /// Reads a full ref name like `refs/heads/main` or `refs/remotes/origin/main`.
    pub(crate) fn from_full(full: &str) -> Option<Ref> {
        if let Some(branch) = full.strip_prefix("refs/heads/") {
            return Some(Ref::Local(branch.to_owned()));
        }
        let (remote, branch) = full.strip_prefix("refs/remotes/")?.split_once('/')?;
        Some(Ref::Remote {
            remote: remote.to_owned(),
            branch: branch.to_owned(),
        })
    }

    /// The branch's name without its remote.
    pub fn branch(&self) -> &str {
        match self {
            Ref::Local(branch) | Ref::Remote { branch, .. } => branch,
        }
    }

    pub fn full(&self) -> String {
        match self {
            Ref::Local(branch) => format!("refs/heads/{branch}"),
            Ref::Remote { remote, branch } => format!("refs/remotes/{remote}/{branch}"),
        }
    }
}

/// The short name git shows: `main` or `origin/main`.
impl fmt::Display for Ref {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ref::Local(branch) => f.write_str(branch),
            Ref::Remote { remote, branch } => write!(f, "{remote}/{branch}"),
        }
    }
}

/// Reads `for-each-ref --format=%(refname)`, leaving out `origin/HEAD` and the like: they
/// only point at another branch in the list.
pub(crate) fn parse_refs(output: &str) -> Vec<Ref> {
    output
        .lines()
        .filter_map(Ref::from_full)
        .filter(|r| !matches!(r, Ref::Remote { branch, .. } if branch == "HEAD"))
        .collect()
}

/// The branch new worktrees start from when nobody picks one: the branch `origin/HEAD`
/// names, else the first of `origin/main`, `origin/master`, `main` and `master` that exists.
pub(crate) fn default_base(origin_head: Option<&str>, refs: &[Ref]) -> Option<Ref> {
    let remote = |branch: &str| Ref::Remote {
        remote: "origin".into(),
        branch: branch.into(),
    };
    origin_head
        .and_then(Ref::from_full)
        .into_iter()
        .chain([
            remote("main"),
            remote("master"),
            Ref::Local("main".into()),
            Ref::Local("master".into()),
        ])
        .find(|candidate| refs.contains(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(branch: &str) -> Ref {
        Ref::Local(branch.into())
    }

    fn origin(branch: &str) -> Ref {
        Ref::Remote {
            remote: "origin".into(),
            branch: branch.into(),
        }
    }

    #[test]
    fn refs_read_as_local_or_remote_without_symbolic_heads() {
        let output = "refs/heads/me/feat\nrefs/remotes/origin/HEAD\nrefs/remotes/origin/main\nrefs/remotes/fork/a/b\n";
        assert_eq!(
            parse_refs(output),
            vec![
                local("me/feat"),
                origin("main"),
                Ref::Remote {
                    remote: "fork".into(),
                    branch: "a/b".into()
                },
            ]
        );
    }

    #[test]
    fn refs_print_and_expand_like_git() {
        assert_eq!(origin("me/feat").to_string(), "origin/me/feat");
        assert_eq!(origin("me/feat").full(), "refs/remotes/origin/me/feat");
        assert_eq!(local("main").full(), "refs/heads/main");
        assert_eq!(origin("me/feat").branch(), "me/feat");
    }

    #[test]
    fn the_default_base_follows_origin_head_then_common_names() {
        let refs = [local("main"), origin("main"), origin("develop")];
        assert_eq!(
            default_base(Some("refs/remotes/origin/develop"), &refs),
            Some(origin("develop"))
        );
        assert_eq!(default_base(None, &refs), Some(origin("main")));
        assert_eq!(
            default_base(None, &[local("master")]),
            Some(local("master"))
        );
        assert_eq!(default_base(None, &[local("trunk")]), None);
    }

    #[test]
    fn a_stale_origin_head_falls_back() {
        let refs = [origin("main")];
        assert_eq!(
            default_base(Some("refs/remotes/origin/gone"), &refs),
            Some(origin("main"))
        );
    }
}
