//! Asks that take longer than a turn of the loop (`docs/devices.md` 6): a screenshot's PNG, the
//! text read off the picture, and a simulator's apps through `simctl`. Each runs on a thread of
//! its own and is answered when it's done, so the person's input is never kept waiting.

use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use koyo_device_wire::{Answer, App, Ask, Point, Refusal, RefusalCode, RequestId, Source};

use crate::host::{Host, refused};
use crate::look;
use crate::ring::Held;
use crate::sim::{ax, simctl};
use crate::text;

/// A snapshot holds this many pieces at most.
const PIECES: usize = 300;
/// The most elements a snapshot of the accessibility tree reads, and how long it may take.
const TREE: ax::Limits = ax::Limits {
    elements: 600,
    depth: 40,
    each: Duration::from_secs(1),
    whole: Duration::from_secs(6),
};

pub enum Done {
    Answer(Result<Answer, Refusal>),
    /// A snapshot's tree, and its refs: they replace the last snapshot's.
    Snapshot {
        tree: String,
        refs: Vec<(String, Point)>,
        truncated: bool,
        source: Source,
    },
}

pub struct Job {
    pub request: RequestId,
    done: mpsc::Receiver<Result<Done, Refusal>>,
}

/// A held slot, moved to the thread that reads it.
struct Moved(Held);

// SAFETY: an `IOSurface` may be read from any thread; the slot is held, so nothing writes it.
unsafe impl Send for Moved {}

impl Host {
    /// Starts `ask` on a thread of its own.
    pub fn job(&mut self, request: RequestId, ask: Ask) {
        let done = match ask {
            Ask::Screenshot { scale } => {
                let Some(held) = self.ring.latest() else {
                    return self.answer(
                        request,
                        refused(RefusalCode::NotReady, "there is no picture yet"),
                    );
                };
                let (held, native) = (Moved(held), self.sim.scale);
                simctl::later(move || {
                    let held = held;
                    look::screenshot(&held.0.surface, native, scale)
                        .map(|(png, size)| {
                            Done::Answer(Ok(Answer::Screenshot {
                                png: png.into(),
                                size,
                                scale: scale.clamp(1, 3),
                            }))
                        })
                        .map_err(|why| Refusal::new(RefusalCode::Failed, why))
                })
            }
            Ask::Snapshot { text } => {
                // The simulator's own tree first, unless the text is asked for; the text on the
                // picture when the tree has none.
                let port = if text {
                    Err("the picture's text was asked for".to_owned())
                } else {
                    self.sim.lookup(ax::SERVICE)
                };
                // The picture is taken only for the text: held meanwhile, its slot couldn't be
                // copied into.
                let (ring, scale, first) = (self.ring.clone(), self.sim.scale, self.next_ref);
                simctl::later(move || {
                    let tree = port.and_then(|port| accessibility(port, first));
                    tree.or_else(|why| {
                        log_fallback(&why);
                        let held = ring.latest().map(Moved).ok_or_else(|| {
                            Refusal::new(RefusalCode::NotReady, "there is no picture yet")
                        })?;
                        on_the_picture(&held, scale, first)
                    })
                })
            }
            other => self.app_job(other),
        };
        self.jobs.push(Job { request, done });
    }

    fn app_job(&self, ask: Ask) -> mpsc::Receiver<Result<Done, Refusal>> {
        let (set, udid) = (self.sim.set.clone(), self.sim.udid.clone());
        simctl::later(move || {
            let run = |args: &[&str], limit: u64| {
                simctl::run(&set, args, Duration::from_secs(limit))
                    .map_err(|why| Refusal::new(RefusalCode::Failed, why))
            };
            let done = match &ask {
                Ask::Apps => {
                    return apps(&run(&["listapps", &udid], 30)?)
                        .map(|apps| Done::Answer(Ok(Answer::Apps { apps })));
                }
                Ask::Install { path } => run(&["install", &udid, path], 85),
                Ask::Launch { bundle, arguments } => {
                    let mut args = vec![
                        "launch",
                        "--terminate-running-process",
                        udid.as_str(),
                        bundle.as_str(),
                    ];
                    args.extend(arguments.iter().map(String::as_str));
                    run(&args, 28)
                }
                Ask::Terminate { bundle } => run(&["terminate", &udid, bundle], 20),
                Ask::OpenUrl { url } => run(&["openurl", &udid, url], 20),
                _ => Err(Refusal::new(RefusalCode::Invalid, "not an ask for apps")),
            };
            done.map(|_| Done::Answer(Ok(Answer::Done)))
        })
    }

    /// Answers the jobs that are done; whether any is still under way.
    pub fn jobs_on(&mut self) -> bool {
        let mut jobs = std::mem::take(&mut self.jobs);
        jobs.retain(|job| match job.done.try_recv() {
            Ok(done) => {
                self.finish(job.request, done);
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                self.answer(
                    job.request,
                    refused(RefusalCode::Failed, "it ended without an answer"),
                );
                false
            }
        });
        self.jobs.extend(jobs);
        !self.jobs.is_empty()
    }

    fn finish(&mut self, request: RequestId, done: Result<Done, Refusal>) {
        match done {
            Ok(Done::Answer(result)) => self.answer(request, result),
            Ok(Done::Snapshot {
                tree,
                refs,
                truncated,
                source,
            }) => {
                self.next_ref += u32::try_from(refs.len()).unwrap_or(0);
                let elements = u32::try_from(refs.len()).unwrap_or(u32::MAX);
                self.refs = refs.into_iter().collect();
                self.answer(
                    request,
                    Ok(Answer::Snapshot {
                        tree,
                        source,
                        elements,
                        truncated,
                    }),
                );
            }
            Err(refusal) => self.answer(request, Err(refusal)),
        }
    }
}

/// A simulator's apps from `simctl listapps`, a property list, by its bundle ids and names.
fn apps(listing: &str) -> Result<Vec<App>, Refusal> {
    let mut command = std::process::Command::new("/usr/bin/plutil");
    command.args(["-convert", "json", "-o", "-", "--", "-"]);
    let json = with_input(command, listing).map_err(|why| {
        Refusal::new(
            RefusalCode::Failed,
            format!("the apps' list didn't read: {why}"),
        )
    })?;
    Ok(parse_apps(&json))
}

/// The bundle ids and display names in `plutil`'s JSON of the listing: an object of apps by
/// their bundle ids.
fn parse_apps(json: &str) -> Vec<App> {
    let Ok(serde_json::Value::Object(listed)) = serde_json::from_str(json) else {
        return Vec::new();
    };
    let mut apps: Vec<App> = listed
        .into_iter()
        .map(|(bundle, app)| {
            let name = ["CFBundleDisplayName", "CFBundleName"]
                .iter()
                .find_map(|key| app.get(key).and_then(serde_json::Value::as_str))
                .map_or_else(|| bundle.clone(), str::to_owned);
            App { name, bundle }
        })
        .collect();
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps
}

fn with_input(mut command: std::process::Command, input: &str) -> Result<String, String> {
    use std::io::Write as _;
    use std::process::Stdio;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    let out = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}


/// A snapshot of the simulator's accessibility tree, refs from `first`; why not, when the
/// service isn't there, doesn't answer, or has nothing to say.
fn accessibility(port: u32, first: u32) -> Result<Done, String> {
    let read = ax::Tree::connect(port)?.read_settled(TREE)?;
    if read.root.children.is_empty() {
        return Err("the simulator's accessibility tree was empty".into());
    }
    let (tree, refs, truncated) = ax::lines(&read.root, first, TREE.elements);
    Ok(Done::Snapshot {
        tree,
        refs,
        truncated: truncated || read.cut,
        source: Source::Accessibility,
    })
}

fn log_fallback(why: &str) {
    crate::link::debug(&format!("snapshot from the picture's text: {why}"));
}

/// A snapshot of the text on the picture, refs from `first`.
fn on_the_picture(held: &Moved, scale: f64, first: u32) -> Result<Done, Refusal> {
    if !text::warm() {
        return Err(Refusal::new(
            RefusalCode::NotReady,
            "macOS is preparing its text recognition for this build of Koyo, which takes up to a \
             minute once: ask for the snapshot again shortly, or take a screenshot meanwhile",
        ));
    }
    let pieces = text::read(&held.0.surface, scale).map_err(|why| Refusal::new(RefusalCode::Failed, why))?;
    let (tree, refs, truncated) = text::tree(&pieces, first, PIECES);
    Ok(Done::Snapshot {
        tree,
        refs,
        truncated,
        source: Source::Text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_read_by_their_ids_and_names() {
        let json = r#"{"com.apple.Preferences":{"CFBundleIdentifier":"com.apple.Preferences","CFBundleDisplayName":"Settings"},"com.apple.mobilesafari":{"CFBundleName":"Safari","CFBundleIdentifier":"com.apple.mobilesafari"}}"#;
        let apps = parse_apps(json);
        assert_eq!(
            apps,
            [
                App {
                    name: "Safari".into(),
                    bundle: "com.apple.mobilesafari".into()
                },
                App {
                    name: "Settings".into(),
                    bundle: "com.apple.Preferences".into()
                }
            ]
        );
    }
}
