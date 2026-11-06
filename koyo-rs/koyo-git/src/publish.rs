//! Getting a checkout's work out: committing it, pushing it, and its pull request, which is the
//! GitHub CLI's to ask for, when it's installed and the remote is GitHub's.

use crate::Checkout;
use crate::command::{Git, GitError, Result};
use crate::create::fetch_remote_branch;
use crate::refs::{self, Ref};

/// What's asked of a pull request, and how it's printed: its title last, as it can say anything.
const FIELDS: &str = "number,state,isDraft,url,title";
const TEMPLATE: &str = "{{.number}}\t{{.state}}\t{{.isDraft}}\t{{.url}}\t{{.title}}";

/// How a checkout's branch stands against where it's published.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Published {
    /// The remote it goes to, if the repository has one.
    pub remote: Option<String>,
    /// The branch it tracks, once it's been pushed.
    pub upstream: Option<Ref>,
    pub ahead: u32,
    pub behind: u32,
    /// Whether it's the branch pull requests go into.
    pub on_default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullRequestState {
    Draft,
    Open,
    Merged,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub state: PullRequestState,
    pub url: String,
}

impl Checkout {
    /// Commits everything changed, as a person would with `git add --all`, hooks and all.
    pub fn commit_all(&self, message: &str) -> Result<()> {
        let message = message.trim();
        if message.is_empty() {
            return Err(GitError::refused("A commit needs a message."));
        }
        Git::new(self.root()).args(["add", "--all"]).run()?;
        Git::new(self.root())
            .args(["commit", "--message", message])
            .run()
    }

    /// How its branch stands, as far as git knows without asking the remote.
    pub fn published(&self) -> Published {
        let upstream = self.upstream();
        let counts = upstream.as_ref().and_then(|_| {
            Git::new(self.root())
                .args(["rev-list", "--left-right", "--count", "@{upstream}...HEAD"])
                .read_optional()
        });
        let (behind, ahead) = counts.as_deref().map_or((0, 0), parse_counts);
        Published {
            remote: self.remote(),
            upstream,
            ahead,
            behind,
            on_default: self.on_default(),
        }
    }

    /// The remote it goes to: `origin`, or the first there is.
    fn remote(&self) -> Option<String> {
        let remotes = Git::new(self.root())
            .arg("remote")
            .read_optional()
            .unwrap_or_default();
        let origin = remotes.lines().find(|remote| *remote == "origin");
        origin.or_else(|| remotes.lines().next()).map(str::to_owned)
    }

    /// The branch its branch tracks.
    fn upstream(&self) -> Option<Ref> {
        let full = Git::new(self.root())
            .args(["rev-parse", "--symbolic-full-name", "@{upstream}"])
            .read_optional()?;
        Ref::from_full(&full)
    }

    /// Whether it's on the branch new worktrees start from, as pull requests go into that.
    fn on_default(&self) -> bool {
        let dir = self.root();
        let Some(branch) = Git::new(dir)
            .args(["branch", "--show-current"])
            .read_optional()
        else {
            return false;
        };
        let origin_head = Git::new(dir)
            .args(["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"])
            .read_optional();
        let listed = Git::new(dir)
            .args([
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads",
                "refs/remotes",
            ])
            .read_optional()
            .unwrap_or_default();
        refs::default_base(origin_head.as_deref(), &refs::parse_refs(&listed))
            .is_some_and(|base| base.branch() == branch)
    }

    /// Asks the remote its branch tracks what's new there, for `published` to know of.
    pub fn fetch(&self) -> Result<()> {
        match self.upstream() {
            Some(Ref::Remote { remote, branch }) => {
                fetch_remote_branch(self.root(), &remote, &branch)
            }
            _ => Ok(()),
        }
    }

    /// Pushes its branch, publishing it if it never was.
    pub fn push(&self) -> Result<()> {
        let Some(remote) = self.remote() else {
            return Err(GitError::refused(
                "This repository has no remote to push to.",
            ));
        };
        let push = Git::new(self.root()).arg("push");
        if self.upstream().is_some() {
            push.run()
        } else {
            push.args(["--set-upstream", remote.as_str(), "HEAD"]).run()
        }
    }

    /// Brings in what's new on its upstream, only if that needs no merge.
    pub fn pull(&self) -> Result<()> {
        Git::new(self.root()).args(["pull", "--ff-only"]).run()
    }

    /// Its branch's pull request, if it has one. It's an error if GitHub can't be asked, which is
    /// also what a repository not on GitHub gets.
    pub fn pull_request(&self) -> Result<Option<PullRequest>> {
        let args = ["pr", "view", "--json", FIELDS, "--template", TEMPLATE];
        match Git::github_cli(self.root())?.args(args).read() {
            Ok(said) => Ok(parse_pull_request(&said)),
            Err(error) if error.message.contains("no pull requests found") => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Opens a pull request of its branch, in the words of its commits.
    pub fn create_pull_request(&self) -> Result<()> {
        Git::github_cli(self.root())?
            .args(["pr", "create", "--fill"])
            .run()
    }
}

/// `rev-list --left-right --count`'s "behind\tahead".
fn parse_counts(counts: &str) -> (u32, u32) {
    let (behind, ahead) = counts.split_once('\t').unwrap_or(("0", "0"));
    (
        behind.trim().parse().unwrap_or(0),
        ahead.trim().parse().unwrap_or(0),
    )
}

/// A pull request as `TEMPLATE` prints it.
fn parse_pull_request(said: &str) -> Option<PullRequest> {
    let mut fields = said.trim().splitn(5, '\t');
    let number = fields.next()?.parse().ok()?;
    let state = fields.next()?;
    let draft = fields.next()? == "true";
    let url = fields.next()?.to_owned();
    let title = fields.next()?.to_owned();
    let state = match state {
        "MERGED" => PullRequestState::Merged,
        "CLOSED" => PullRequestState::Closed,
        _ if draft => PullRequestState::Draft,
        _ => PullRequestState::Open,
    };
    Some(PullRequest {
        number,
        title,
        state,
        url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pull_request_reads_as_the_template_prints_it() {
        let said = "12\tOPEN\tfalse\thttps://github.com/a/b/pull/12\tA title\twith a tab\n";
        let pull = parse_pull_request(said).unwrap();
        assert_eq!(pull.number, 12);
        assert_eq!(pull.state, PullRequestState::Open);
        assert_eq!(pull.title, "A title\twith a tab");
        let draft = parse_pull_request("3\tOPEN\ttrue\tu\tt").unwrap();
        assert_eq!(draft.state, PullRequestState::Draft);
        let merged = parse_pull_request("3\tMERGED\tfalse\tu\tt").unwrap();
        assert_eq!(merged.state, PullRequestState::Merged);
        assert_eq!(parse_pull_request(""), None);
    }

    #[test]
    fn counts_read_behind_then_ahead() {
        assert_eq!(parse_counts("2\t5"), (2, 5));
        assert_eq!(parse_counts("nonsense"), (0, 0));
    }
}
