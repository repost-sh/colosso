//! Which checkout a path is in, read from `.git` without running git: the root, the
//! repository's main checkout and the branch; and the worktrees a selector names.
//! `docs/terminals.md` 5.2 and 5.5.

use std::path::{Path, PathBuf};

use colosso_term_control::{ErrorCode, ErrorDetails, ProtocolError, Selector, WorktreeRef};

/// The worktree `path` is in, or `None` outside any checkout.
pub fn discover(path: &Path) -> Option<WorktreeRef> {
    let start = std::fs::canonicalize(path).ok()?;
    let root = start
        .ancestors()
        .find(|dir| dir.join(".git").exists())?
        .to_path_buf();
    let git_dir = git_dir(&root)?;
    let repo_main = main_checkout(&root, &git_dir);
    Some(WorktreeRef {
        path: root,
        repo_main,
        branch: head_branch(&git_dir),
    })
}

/// The folder holding the checkout's HEAD: `.git` itself, or the one a linked worktree's
/// `.git` file points at.
fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let target = text.strip_prefix("gitdir:")?.trim();
    let target = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        root.join(target)
    };
    std::fs::canonicalize(target).ok()
}

/// A linked worktree's git dir is `<main>/.git/worktrees/<name>`; anything else is its own main.
fn main_checkout(root: &Path, git_dir: &Path) -> PathBuf {
    let linked = git_dir
        .parent()
        .filter(|worktrees| {
            worktrees
                .file_name()
                .is_some_and(|name| name == "worktrees")
        })
        .and_then(Path::parent)
        .filter(|dot_git| dot_git.file_name().is_some_and(|name| name == ".git"))
        .and_then(Path::parent);
    linked.map_or_else(|| root.to_path_buf(), Path::to_path_buf)
}

fn head_branch(git_dir: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned)
}

/// Every checkout of the repository whose main checkout is `repo_main`, the main one first.
pub fn project_worktrees(repo_main: &Path) -> Vec<WorktreeRef> {
    let mut found: Vec<WorktreeRef> = discover(repo_main).into_iter().collect();
    let Ok(entries) = std::fs::read_dir(repo_main.join(".git").join("worktrees")) else {
        return found;
    };
    for entry in entries.flatten() {
        // `gitdir` holds the path of the linked checkout's `.git` file.
        let Ok(gitdir) = std::fs::read_to_string(entry.path().join("gitdir")) else {
            continue;
        };
        let Some(root) = Path::new(gitdir.trim()).parent() else {
            continue;
        };
        if let Some(worktree) = discover(root)
            && !found.iter().any(|known| known.path == worktree.path)
        {
            found.push(worktree);
        }
    }
    found
}

/// The worktree a selector means, from where the caller is and what the daemon knows.
pub fn resolve(
    selector: Option<&Selector>,
    cwd: Option<&Path>,
    known: &[WorktreeRef],
) -> Result<WorktreeRef, ProtocolError> {
    let current = || {
        cwd.and_then(discover).ok_or_else(|| {
            ProtocolError::new(
                ErrorCode::WorktreeRequired,
                "the caller isn't in a git checkout, so `worktree` has to say which one",
            )
        })
    };
    match selector {
        None | Some(Selector::Current) => current(),
        Some(Selector::Main) => {
            let here = current()?;
            discover(&here.repo_main)
                .ok_or_else(|| not_found(&here.repo_main.display().to_string()))
        }
        Some(Selector::Path(path)) => {
            let path = expand_home(path);
            discover(&path).ok_or_else(|| not_found(&path.display().to_string()))
        }
        Some(Selector::Branch(branch)) => pick(
            candidates(cwd, known),
            |worktree| worktree.branch.as_deref() == Some(branch),
            branch,
        ),
        Some(Selector::Name(name)) => pick(
            candidates(cwd, known),
            |worktree| {
                worktree
                    .path
                    .file_name()
                    .is_some_and(|folder| folder == name.as_str())
            },
            name,
        ),
    }
}

/// The caller's project's checkouts, then every checkout with terminals, without repeats.
fn candidates(cwd: Option<&Path>, known: &[WorktreeRef]) -> Vec<WorktreeRef> {
    let mut all = cwd
        .and_then(discover)
        .map(|here| project_worktrees(&here.repo_main))
        .unwrap_or_default();
    for worktree in known {
        if !all.iter().any(|seen| seen.path == worktree.path) {
            all.push(worktree.clone());
        }
    }
    all
}

fn pick(
    candidates: Vec<WorktreeRef>,
    matches: impl Fn(&WorktreeRef) -> bool,
    wanted: &str,
) -> Result<WorktreeRef, ProtocolError> {
    let mut found: Vec<WorktreeRef> = candidates.into_iter().filter(matches).collect();
    match found.len() {
        0 => Err(not_found(wanted)),
        1 => found.pop().ok_or_else(|| not_found(wanted)),
        _ => Err(ProtocolError::new(
            ErrorCode::AmbiguousWorktree,
            format!("{wanted} names more than one checkout; use path:<path>"),
        )
        .with_details(ErrorDetails::Worktrees {
            paths: found.into_iter().map(|worktree| worktree.path).collect(),
        })),
    }
}

fn not_found(wanted: &str) -> ProtocolError {
    ProtocolError::new(
        ErrorCode::WorktreeNotFound,
        format!("no git checkout matches {wanted}"),
    )
}

fn expand_home(path: &Path) -> PathBuf {
    let Ok(rest) = path.strip_prefix("~") else {
        return path.to_path_buf();
    };
    dirs::home_dir().map_or_else(|| path.to_path_buf(), |home| home.join(rest))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A main checkout and a linked worktree, laid out as git would, under `target/`.
    pub(crate) fn fake_repo(name: &str) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-termd")
            .join(name);
        let _ = std::fs::remove_dir_all(&root);
        let main = root.join("main");
        let feat = root.join("feat");
        std::fs::create_dir_all(main.join(".git/worktrees/feat")).unwrap();
        std::fs::create_dir_all(feat.join("crates")).unwrap();
        std::fs::write(main.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(
            main.join(".git/worktrees/feat/HEAD"),
            "ref: refs/heads/feat-x\n",
        )
        .unwrap();
        std::fs::write(
            main.join(".git/worktrees/feat/gitdir"),
            format!("{}/.git\n", feat.display()),
        )
        .unwrap();
        std::fs::write(
            feat.join(".git"),
            format!("gitdir: {}/.git/worktrees/feat\n", main.display()),
        )
        .unwrap();
        std::fs::canonicalize(root).unwrap()
    }

    #[test]
    fn discovers_a_linked_worktree_and_its_main() {
        let root = fake_repo("discover");
        let feat = discover(&root.join("feat/crates")).unwrap();
        assert_eq!(feat.path, root.join("feat"));
        assert_eq!(feat.repo_main, root.join("main"));
        assert_eq!(feat.branch.as_deref(), Some("feat-x"));
        let main = discover(&root.join("main")).unwrap();
        assert_eq!(main.repo_main, main.path);
        assert!(main.is_main());
        assert_eq!(discover(Path::new("/")), None);
    }

    #[test]
    fn lists_a_projects_worktrees_and_resolves_selectors() {
        let root = fake_repo("resolve");
        let listed = project_worktrees(&root.join("main"));
        assert_eq!(listed.len(), 2);
        let cwd = root.join("feat/crates");
        assert_eq!(
            resolve(None, Some(&cwd), &[]).unwrap().path,
            root.join("feat")
        );
        assert_eq!(
            resolve(Some(&Selector::Main), Some(&cwd), &[])
                .unwrap()
                .path,
            root.join("main")
        );
        assert_eq!(
            resolve(Some(&Selector::Branch("main".into())), Some(&cwd), &[])
                .unwrap()
                .path,
            root.join("main")
        );
        assert_eq!(
            resolve(Some(&Selector::Name("feat".into())), Some(&cwd), &[])
                .unwrap()
                .path,
            root.join("feat")
        );
        assert_eq!(
            resolve(Some(&Selector::Path(root.join("main"))), None, &[])
                .unwrap()
                .path,
            root.join("main")
        );
        assert_eq!(
            resolve(None, None, &[]).unwrap_err().code,
            ErrorCode::WorktreeRequired
        );
        assert_eq!(
            resolve(Some(&Selector::Branch("nope".into())), Some(&cwd), &[])
                .unwrap_err()
                .code,
            ErrorCode::WorktreeNotFound
        );
        // A checkout the daemon knows from its terminals counts even without a cwd.
        let known = [discover(&root.join("feat")).unwrap()];
        assert_eq!(
            resolve(Some(&Selector::Branch("feat-x".into())), None, &known)
                .unwrap()
                .path,
            root.join("feat")
        );
    }
}
