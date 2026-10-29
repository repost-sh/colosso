//! The files a server asked to hear about: what the checkout's project says was written,
//! matched against the globs the server registered, and sent to it as changed or deleted.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use colosso_lsp::lsp_types::notification::DidChangeWatchedFiles;
use colosso_lsp::lsp_types::{
    DidChangeWatchedFilesParams, FileChangeType, FileEvent, FileSystemWatcher, GlobPattern, OneOf,
};
use colosso_lsp::uri;
use colosso_project::{Feed, Project, Written};
use futures::StreamExt as _;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use gpui_kit::{AppContext as _, Context, Task};

use super::server::Server;

/// What the server asked to hear about, and the checkout's writes followed meanwhile.
#[derive(Default)]
pub(in crate::language) struct Watch {
    globs: Option<Arc<GlobSet>>,
    project: Option<Arc<Project>>,
    following: Option<Task<()>>,
}

impl Watch {
    /// Forgets the globs: the server that registered them has gone.
    pub(super) fn clear(&mut self) {
        self.globs = None;
    }
}

impl Server {
    /// Follows what's written in the checkout, through its project, unless it does already.
    pub(super) fn follow_writes(&mut self, project: &Arc<Project>, cx: &mut Context<Self>) {
        if self.watch.following.is_some() {
            return;
        }
        let mut updates = project.subscribe(Feed::Written);
        self.watch.project = Some(project.clone());
        self.watch.following = Some(cx.spawn(async move |this, cx| {
            while let Some(first) = updates.next().await {
                let mut written = vec![first.written];
                while let Some(more) = updates.try_next() {
                    written.push(more.written);
                }
                if this
                    .update(cx, |this, cx| this.written(written, cx))
                    .is_err()
                {
                    return;
                }
            }
        }));
    }

    /// Takes the globs the server registered now.
    pub(super) fn registrations_changed(&mut self, _: &mut Context<Self>) {
        let Some(client) = &self.client else {
            return;
        };
        self.watch.globs = globs(&client.watched_files()).map(Arc::new);
    }

    /// Tells the server about the files it watches among those `written`, looked at on a
    /// background thread for whether they're still there.
    fn written(&mut self, written: Vec<Written>, cx: &mut Context<Self>) {
        let (Some(client), Some(globs), Some(project)) = (
            self.client.clone(),
            self.watch.globs.clone(),
            self.watch.project.clone(),
        ) else {
            return;
        };
        cx.background_spawn(async move {
            let changes = changes(&written, &project, &globs);
            if changes.is_empty() {
                return;
            }
            let params = DidChangeWatchedFilesParams { changes };
            if let Err(error) = client.notify::<DidChangeWatchedFiles>(&params) {
                log::debug!("couldn't say which watched files changed: {error}");
            }
        })
        .detach();
    }
}

/// The files written that the globs match, each changed if it's there and deleted if not. A
/// folder written stands for every file listed in it, and everything for every file.
fn changes(written: &[Written], project: &Project, globs: &GlobSet) -> Vec<FileEvent> {
    let root = project.root();
    let mut paths: HashSet<PathBuf> = HashSet::new();
    for written in written {
        match written {
            Written::Paths(written) => paths.extend(written.iter().map(|path| root.join(&**path))),
            Written::Folders(folders) => {
                let inside = |file: &str| {
                    (folders.iter()).any(|folder| {
                        folder.is_empty()
                            || file
                                .strip_prefix(&**folder)
                                .is_some_and(|rest| rest.starts_with('/'))
                    })
                };
                let files = project.files();
                let files = files.iter().filter(|file| inside(file));
                paths.extend(files.map(|file| root.join(&**file)));
            }
            Written::Everything => {
                paths.extend(project.files().iter().map(|file| root.join(&**file)));
            }
        }
    }
    (paths.into_iter())
        .filter(|path| globs.is_match(path))
        .filter_map(|path| {
            let typ = if path.exists() {
                FileChangeType::CHANGED
            } else {
                FileChangeType::DELETED
            };
            Some(FileEvent::new(uri::from_path(&path)?, typ))
        })
        .collect()
}

/// The globs of the server's watchers, matched against absolute paths: a relative pattern
/// under its base folder, and a bare one anywhere.
fn globs(watchers: &[FileSystemWatcher]) -> Option<GlobSet> {
    let mut set = GlobSetBuilder::new();
    let mut any = false;
    for watcher in watchers {
        let pattern = match &watcher.glob_pattern {
            GlobPattern::String(pattern) if pattern.starts_with(['/', '*']) => pattern.clone(),
            GlobPattern::String(pattern) => format!("**/{pattern}"),
            GlobPattern::Relative(relative) => {
                let base = match &relative.base_uri {
                    OneOf::Left(folder) => &folder.uri,
                    OneOf::Right(uri) => uri,
                };
                let Some(base) = uri::to_path(base)
                    .as_deref()
                    .and_then(Path::to_str)
                    .map(str::to_owned)
                else {
                    continue;
                };
                let base = globset::escape(base.trim_end_matches('/'));
                format!("{base}/{}", relative.pattern)
            }
        };
        match GlobBuilder::new(&pattern).literal_separator(true).build() {
            Ok(glob) => {
                set.add(glob);
                any = true;
            }
            Err(error) => log::debug!("skipped the watched glob {pattern}: {error}"),
        }
    }
    let set = set.build().inspect_err(|error| log::debug!("{error}")).ok();
    set.filter(|_| any)
}

#[cfg(test)]
mod tests {
    use colosso_lsp::lsp_types::{RelativePattern, Uri, WorkspaceFolder};

    use super::*;

    #[test]
    fn globs_match_under_their_base_or_anywhere() {
        let base: Uri = uri::from_path(Path::new("/work/my app")).unwrap();
        let watchers = [
            FileSystemWatcher {
                glob_pattern: GlobPattern::Relative(RelativePattern {
                    base_uri: OneOf::Left(WorkspaceFolder {
                        uri: base,
                        name: "my app".into(),
                    }),
                    pattern: "**/*.rs".into(),
                }),
                kind: None,
            },
            FileSystemWatcher {
                glob_pattern: GlobPattern::String("Cargo.{toml,lock}".into()),
                kind: None,
            },
        ];
        let globs = globs(&watchers).unwrap();
        assert!(globs.is_match("/work/my app/src/lib.rs"));
        assert!(!globs.is_match("/work/other/src/lib.rs"));
        assert!(globs.is_match("/anywhere/Cargo.lock"));
        assert!(!globs.is_match("/work/my app/README.md"));
    }
}
