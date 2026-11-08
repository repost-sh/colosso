//! A project's picture on its tile (`docs/arc-sidebar.md` 3): the one the person picked, or the
//! avatar its git host shows for it. The `origin` remote says which host and owner; the avatar
//! is fetched once, with the system's `curl` as the search suggestions are, and kept in the
//! app's data folder, where a later launch finds it. With no remote, or nothing to fetch, the
//! tile shows the project's initials.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use gpui_kit::{App, AppContext as _, Global};
use koyo_git::Repo;

use crate::settings;

/// How long a kept avatar is shown before it's fetched again, in case it changed.
const FRESH: Duration = Duration::from_hours(7 * 24);
/// How long a host has to answer each request.
const TIMEOUT: &str = "10";

/// The avatars known by project.
#[derive(Default)]
struct Avatars {
    /// `None` once looked for and not found.
    known: HashMap<PathBuf, Option<PathBuf>>,
}

impl Global for Avatars {}

/// The picture `project`'s tile shows: the one picked for it, or its git host's avatar once
/// it's been fetched, which asking for it the first time starts.
pub fn picture(project: &Path, cx: &mut App) -> Option<PathBuf> {
    if let Some(picked) = settings::get(cx).project_icon(project) {
        return Some(picked.to_owned());
    }
    if let Some(known) = cx.default_global::<Avatars>().known.get(project) {
        return known.clone();
    }
    cx.default_global::<Avatars>()
        .known
        .insert(project.to_owned(), None);
    if cfg!(test) {
        return None;
    }
    let folder = settings::data_dir().map(|data| data.join("avatars"));
    let project = project.to_owned();
    let fetching = cx.background_spawn({
        let project = project.clone();
        async move { fetch(&project, &folder?) }
    });
    cx.spawn(async move |cx| {
        let Some(found) = fetching.await else {
            return;
        };
        cx.update(|cx| {
            cx.default_global::<Avatars>()
                .known
                .insert(project, Some(found));
            cx.refresh_windows();
        });
    })
    .detach();
    None
}

/// The host and owner a remote's URL names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remote {
    pub host: String,
    /// Everything before the repository's name: a user, an organisation, or GitLab's groups.
    pub owner: String,
    pub repo: String,
}

impl Remote {
    /// Reads `git@host:owner/repo.git`, `ssh://git@host:22/owner/repo` and
    /// `https://host/owner/repo` alike.
    pub fn parse(url: &str) -> Option<Remote> {
        let url = url.trim();
        let (rest, scp) = match url.split_once("://") {
            Some((_, rest)) => (rest, false),
            None => (url, true),
        };
        let rest = rest.rsplit_once('@').map_or(rest, |(_, rest)| rest);
        let (authority, path) = if scp {
            rest.split_once(':')?
        } else {
            rest.split_once('/')?
        };
        let host = authority.split(':').next()?.to_lowercase();
        let path = path.trim_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        let (owner, repo) = path.rsplit_once('/')?;
        let owner = owner.trim_start_matches('/');
        (!host.is_empty() && !owner.is_empty() && !repo.is_empty()).then(|| Remote {
            host,
            owner: owner.to_owned(),
            repo: repo.to_owned(),
        })
    }

    /// Where the avatar is kept: a file per host and owner, the repository's own on hosts that
    /// give repositories theirs.
    fn kept_at(&self, folder: &Path) -> PathBuf {
        let name = format!("{}__{}", self.owner, self.repo).replace(['/', '\\', ':'], "_");
        folder
            .join(self.host.replace(['/', '\\', ':'], "_"))
            .join(name)
    }

    /// The addresses to try for the picture, in order: an image's, or an API's that names
    /// one in its JSON, with the path to it.
    fn sources(&self) -> Vec<Source> {
        let Remote { host, owner, repo } = self;
        let first = owner.split('/').next().unwrap_or(owner);
        let encoded =
            |text: &str| form_urlencoded::byte_serialize(text.as_bytes()).collect::<String>();
        let project = encoded(&format!("{owner}/{repo}"));
        let gitlab = |host: &str| {
            vec![
                Source::Json(
                    format!("https://{host}/api/v4/projects/{project}"),
                    &["avatar_url"],
                ),
                Source::Json(
                    format!("https://{host}/api/v4/groups/{}", encoded(owner)),
                    &["avatar_url"],
                ),
                Source::Json(
                    format!("https://{host}/api/v4/users?username={}", encoded(first)),
                    &["0", "avatar_url"],
                ),
            ]
        };
        match host.as_str() {
            // The avatars' own host first: it answers by name too, and stays up when github.com
            // doesn't.
            "github.com" => vec![
                Source::Image(format!(
                    "https://avatars.githubusercontent.com/{first}?size=128"
                )),
                Source::Image(format!("https://github.com/{first}.png?size=128")),
            ],
            "bitbucket.org" => vec![
                Source::Json(
                    format!("https://api.bitbucket.org/2.0/repositories/{first}/{repo}"),
                    &["links", "avatar", "href"],
                ),
                Source::Json(
                    format!("https://api.bitbucket.org/2.0/workspaces/{first}"),
                    &["links", "avatar", "href"],
                ),
            ],
            host if host.contains("gitlab") => gitlab(host),
            // Gitea and Forgejo, then GitLab on a host of its own.
            host => {
                let mut sources = vec![
                    Source::Json(
                        format!("https://{host}/api/v1/repos/{first}/{repo}"),
                        &["avatar_url"],
                    ),
                    Source::Json(
                        format!("https://{host}/api/v1/repos/{first}/{repo}"),
                        &["owner", "avatar_url"],
                    ),
                ];
                sources.extend(gitlab(host));
                sources
            }
        }
    }
}

/// Where a picture may be found.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    Image(String),
    /// An API's address, and the path through its JSON to the picture's.
    Json(String, &'static [&'static str]),
}

/// The avatar of the project at `project`, kept in `folder`: the kept one while it's fresh, or
/// fetched from its git host. Blocks; call it off the main thread.
fn fetch(project: &Path, folder: &Path) -> Option<PathBuf> {
    let url = Repo::open(project).ok()?.remote_url()?;
    let remote = Remote::parse(&url)?;
    let kept = remote.kept_at(folder);
    let fresh = std::fs::metadata(&kept)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age < FRESH);
    if fresh {
        return Some(kept);
    }
    let fetched = remote.sources().into_iter().find_map(|source| {
        let image = match source {
            Source::Image(url) => url,
            Source::Json(url, path) => {
                let body = get(&url, "application/json")?;
                let json: serde_json::Value = serde_json::from_slice(&body).ok()?;
                let found =
                    path.iter()
                        .try_fold(&json, |value, key| match key.parse::<usize>() {
                            Ok(index) => value.get(index),
                            Err(_) => value.get(key),
                        })?;
                found.as_str()?.to_owned()
            }
        };
        get(&image, "image/*").filter(|bytes| is_image(bytes))
    });
    match fetched {
        Some(bytes) => {
            std::fs::create_dir_all(kept.parent()?).ok()?;
            crate::atomic_file::write(&kept, &bytes);
            Some(kept)
        }
        // Offline, an old picture beats none.
        None => kept.exists().then_some(kept),
    }
}

/// What `url` answers, following redirects, if it answers well.
fn get(url: &str, accept: &str) -> Option<Vec<u8>> {
    let output = Command::new("curl")
        // First, so no `.curlrc` changes what's asked.
        .arg("--disable")
        .args(["--fail", "--silent", "--location", "--compressed"])
        .args(["--proto", "=https", "--proto-redir", "=https"])
        .args(["--max-time", TIMEOUT, "--max-filesize", "2000000"])
        .args(["--header", &format!("Accept: {accept}")])
        .arg(url)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .inspect_err(|error| log::debug!("couldn't fetch {url}: {error}"))
        .ok()?;
    (output.status.success() && !output.stdout.is_empty()).then_some(output.stdout)
}

/// Whether `bytes` read as a picture the tile can show.
fn is_image(bytes: &[u8]) -> bool {
    let svg = bytes.len() < 500_000
        && std::str::from_utf8(&bytes[..bytes.len().min(512)])
            .is_ok_and(|start| start.contains("<svg"));
    image::guess_format(bytes).is_ok() || svg
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(host: &str, owner: &str, repo: &str) -> Remote {
        Remote {
            host: host.into(),
            owner: owner.into(),
            repo: repo.into(),
        }
    }

    #[test]
    fn remotes_are_read_whatever_their_form() {
        let github = Some(remote("github.com", "koyo", "colosso"));
        assert_eq!(Remote::parse("git@github.com:koyo/colosso.git"), github);
        assert_eq!(Remote::parse("https://github.com/koyo/colosso"), github);
        assert_eq!(
            Remote::parse("https://me@github.com/koyo/colosso.git/"),
            github
        );
        assert_eq!(
            Remote::parse("ssh://git@gitlab.example.com:2222/team/web/site.git"),
            Some(remote("gitlab.example.com", "team/web", "site"))
        );
        assert_eq!(Remote::parse("/srv/git/project.git"), None);
        assert_eq!(Remote::parse("git@github.com:lonely"), None);
    }

    #[test]
    fn each_host_is_asked_where_it_keeps_its_avatars() {
        let github = Remote::parse("git@github.com:koyo/colosso.git").unwrap();
        assert_eq!(
            github.sources(),
            vec![
                Source::Image("https://avatars.githubusercontent.com/koyo?size=128".into()),
                Source::Image("https://github.com/koyo.png?size=128".into()),
            ]
        );
        let gitlab = Remote::parse("https://gitlab.com/team/web/site").unwrap();
        let Source::Json(first, _) = &gitlab.sources()[0] else {
            panic!("GitLab is asked through its API");
        };
        assert_eq!(
            first,
            "https://gitlab.com/api/v4/projects/team%2Fweb%2Fsite"
        );
        let bitbucket = Remote::parse("git@bitbucket.org:team/site.git").unwrap();
        assert!(matches!(
            &bitbucket.sources()[0],
            Source::Json(url, _) if url == "https://api.bitbucket.org/2.0/repositories/team/site"
        ));
    }

    #[test]
    fn only_pictures_are_kept() {
        assert!(is_image(b"\x89PNG\r\n\x1a\n...."));
        assert!(is_image(b"<?xml version=\"1.0\"?><svg></svg>"));
        assert!(!is_image(b"<html>Not found</html>"));
    }
}
