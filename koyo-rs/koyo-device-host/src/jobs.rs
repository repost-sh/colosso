//! Asks that take longer than a turn of the loop (`docs/devices.md` 6): a screenshot's PNG, the
//! text read off the picture, and a simulator's apps through `simctl`. Each runs on a thread of
//! its own and is answered when it's done, so the person's input is never kept waiting.

use std::sync::Arc;
use std::sync::mpsc::{self, TryRecvError};
use std::time::Duration;

use koyo_device_wire::{Answer, App, Ask, Point, Refusal, RefusalCode, RequestId, Source};

use crate::host::{Host, refused};
use crate::look::{self, Sample};
use crate::ring::Held;
use crate::sim::access::{Shared, with_tree};
use crate::sim::{ax, simctl};
use crate::text;

/// A snapshot holds this many pieces at most.
const PIECES: usize = 300;
/// The refs a snapshot may number, set aside as it starts: two at once never share a number.
/// At least as many as a read takes elements or pieces.
pub const NUMBERED: u32 = 600;
/// How long a launch waits for its app to be in front.
const IN_FRONT_WITHIN: Duration = Duration::from_secs(10);

/// The most elements a snapshot of the accessibility tree reads, and how long it may take.
const TREE: ax::Limits = ax::Limits {
    elements: 600,
    depth: 40,
    each: Duration::from_secs(2),
    whole: Duration::from_secs(8),
};

pub enum Done {
    Answer(Result<Answer, Refusal>),
    /// A snapshot's tree, and its refs: they replace the last snapshot's.
    Snapshot {
        tree: String,
        refs: Vec<(String, Point)>,
        truncated: bool,
        source: Source,
        /// Its refs' first number.
        first: u32,
    },
    /// Done, answered once the screen has changed from `before` and gone still: an app
    /// launched, or a link opened, draws a moment after `simctl` returns.
    Settle { before: Option<Sample> },
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
                let (held, native) = (Moved(held), self.device.scale());
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
                let tree = Arc::clone(&self.tree);
                // A real device has no tree: its text is all there is, said nowhere.
                let port = if text {
                    Err("the picture's text was asked for".to_owned())
                } else if !self.device.has_tree() {
                    Err("a real device has no accessibility tree".to_owned())
                } else {
                    Ok(self.ax_port())
                };
                // The picture is taken only for the text: held meanwhile, its slot couldn't be
                // copied into.
                let first = self.next_ref;
                self.next_ref += NUMBERED;
                let (ring, scale) = (self.ring.clone(), self.device.scale());
                simctl::later(move || {
                    let read = port.and_then(|port| accessibility(&tree, port, first));
                    read.or_else(|why| {
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
        // A real device's app asks were refused before ([`crate::device::Device::refuses`]).
        let (set, udid) = (self.device.simulator()).map_or_else(Default::default, |sim| (sim.set.clone(), sim.udid.clone()));
        // The screen before a launch or a link, to tell when what they open has drawn.
        let settles = matches!(ask, Ask::Launch { .. } | Ask::OpenUrl { .. });
        let before = settles
            .then(|| self.ring.latest().and_then(|held| look::sample(&held.surface, self.device.scale())))
            .flatten();
        // A launch waits for its app to be in front, as the accessibility tree tells.
        let tree = Arc::clone(&self.tree);
        let port = matches!(ask, Ask::Launch { .. }).then(|| self.ax_port()).flatten();
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
                    crate::sim::access::wanted(&tree);
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
            if let (Ask::Launch { bundle, .. }, Ok(_)) = (&ask, &done) {
                in_front(&tree, port, bundle);
            }
            done.map(|_| {
                if settles {
                    Done::Settle { before }
                } else {
                    Done::Answer(Ok(Answer::Done))
                }
            })
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
            Ok(Done::Settle { before }) => {
                let settling = crate::acting::Waiting::settling(request, before, self);
                self.waits.push(settling);
            }
            Ok(Done::Snapshot {
                tree,
                refs,
                truncated,
                source,
                first,
            }) => {
                let elements = u32::try_from(refs.len()).unwrap_or(u32::MAX);
                // The numbers it didn't use, given back unless another snapshot started after it.
                if self.next_ref == first + NUMBERED {
                    self.next_ref = first + elements;
                }
                // A snapshot that started before the refs held now doesn't replace them.
                if first >= self.refs_first {
                    self.refs = refs.into_iter().collect();
                    self.refs_first = first;
                }
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


impl Host {
    /// The accessibility service's port, when no connection is kept: looked up only to make one.
    /// The lock is only tried, never waited on, so the host's loop goes on whatever a job holds.
    fn ax_port(&self) -> Option<u32> {
        // Locked, it is being switched: the port is looked up, should no connection be kept.
        let connected = self.tree.try_lock().is_ok_and(|held| held.connected());
        (!connected).then(|| self.device.simulator()?.lookup(ax::SERVICE).ok()).flatten()
    }
}

/// A snapshot of the simulator's accessibility tree, refs from `first`; why not, when the
/// service isn't there, doesn't answer, or has nothing to say.
fn accessibility(shared: &Shared, port: Option<u32>, first: u32) -> Result<Done, String> {
    let read = with_tree(shared, port, |tree| tree.read_settled(TREE))?;
    if read.root.children.is_empty() {
        return Err("the simulator's accessibility tree was empty".into());
    }
    let (lines, refs, truncated) = ax::lines(&read.root, first, TREE.elements);
    Ok(Done::Snapshot {
        tree: lines,
        refs,
        truncated: truncated || read.cut,
        source: Source::Accessibility,
        first,
    })
}

/// Waits, up to [`IN_FRONT_WITHIN`], until the app `bundle` is in front by the accessibility
/// tree: a launched app isn't for a moment, and its launch screen is still, so the picture alone
/// can't tell. Gives up at once when the tree can't be read.
fn in_front(shared: &Shared, port: Option<u32>, bundle: &str) {
    let started = std::time::Instant::now();
    while started.elapsed() < IN_FRONT_WITHIN {
        match with_tree(shared, port, |tree| tree.in_front(Duration::from_secs(2))) {
            Ok(Some(front)) if front == bundle => return,
            Ok(_) => std::thread::park_timeout(Duration::from_millis(300)),
            Err(_) => return,
        }
    }
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
        first,
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
