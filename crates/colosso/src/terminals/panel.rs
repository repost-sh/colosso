//! The activity panel beside a terminal (`docs/terminals.md` 7.4): its logs, read on by
//! offset while the panel is open, in the tabs agent-terminal's dashboard has: Activity,
//! Commands, Screenshots, Output and Info. Closed, it's dropped, so nothing reads and nothing
//! listens.

mod log;
mod rows;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use colosso_term::{Notice, TermClient};
use colosso_term_control::log::{read_since, tail_start};
use colosso_term_control::{
    ActivityEntry, CommandRecord, EventKind, Handle, LogKind, LogRead, RequestBody, ResponseBody,
    ScreenshotEntry, TerminalRecord,
};
use futures::StreamExt as _;
use gpui_kit::{AppContext as _, Context, ScrollHandle, Task};
use log::{Log, ReadFrom};

/// How much of a log the panel reads when it opens: its newest lines.
const TAIL_BYTES: u64 = 2 << 20;
/// How much it reads at a time after that.
const READ_BYTES: usize = 1 << 20;
/// How much of the output since the mark the Output tab shows, the newest.
const OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PanelTab {
    Activity,
    Commands,
    Screenshots,
    Output,
    Info,
}

impl PanelTab {
    pub const ALL: [PanelTab; 5] = [
        PanelTab::Activity,
        PanelTab::Commands,
        PanelTab::Screenshots,
        PanelTab::Output,
        PanelTab::Info,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PanelTab::Activity => "Activity",
            PanelTab::Commands => "Commands",
            PanelTab::Screenshots => "Screenshots",
            PanelTab::Output => "Output",
            PanelTab::Info => "Info",
        }
    }
}

/// The output since the mark the person set.
#[derive(Default)]
struct Output {
    mark: Option<u64>,
    text: String,
    next: u64,
}

impl Output {
    /// Adds `text`, keeping the newest [`OUTPUT_BYTES`].
    fn append(&mut self, text: &str) {
        self.text.push_str(text);
        let over = self.text.len().saturating_sub(OUTPUT_BYTES);
        if over > 0 {
            let cut = (over..self.text.len())
                .find(|&at| self.text.is_char_boundary(at))
                .unwrap_or(self.text.len());
            self.text.drain(..cut);
        }
    }
}

pub struct ActivityPanel {
    client: Arc<TermClient>,
    handle: Handle,
    tab: PanelTab,
    activity: Log<ActivityEntry>,
    commands: Log<CommandRecord>,
    screenshots: Log<ScreenshotEntry>,
    /// The number of the first activity row Clear left shown, in this view only.
    cleared: usize,
    following: bool,
    expanded: HashSet<(PanelTab, usize)>,
    failed_only: bool,
    output: Output,
    info: Option<TerminalRecord>,
    scroll: ScrollHandle,
    _listening: Task<()>,
    /// Reads the output since the mark while the Output tab is shown.
    polling: Option<Task<()>>,
}

impl ActivityPanel {
    pub fn new(client: Arc<TermClient>, handle: Handle, cx: &mut Context<Self>) -> Self {
        let mut inbox = client.subscribe();
        let ours = handle.clone();
        let listening = cx.spawn(async move |this, cx| {
            for kind in LogKind::ALL {
                if this.update(cx, |this, cx| this.read(kind, cx)).is_err() {
                    return;
                }
            }
            while let Some(notice) = inbox.next().await {
                let Notice::Event(event) = notice else {
                    continue;
                };
                if event.handle != ours {
                    continue;
                }
                // Anything else may change the record, which only Info shows.
                let heard = this.update(cx, |this, cx| match event.kind {
                    EventKind::Logged { log, .. } => this.read(log, cx),
                    _ if this.tab == PanelTab::Info => this.refresh_info(cx),
                    _ => {}
                });
                if heard.is_err() {
                    return;
                }
            }
        });
        ActivityPanel {
            client,
            handle,
            tab: PanelTab::Activity,
            activity: Log::default(),
            commands: Log::default(),
            screenshots: Log::default(),
            cleared: 0,
            following: true,
            expanded: HashSet::new(),
            failed_only: false,
            output: Output::default(),
            info: None,
            scroll: ScrollHandle::new(),
            _listening: listening,
            polling: None,
        }
    }

    fn path(&self, kind: LogKind) -> PathBuf {
        self.client.dir().journal(&self.handle, kind)
    }

    /// Reads what `kind`'s log gained since the panel last read it: its newest lines the
    /// first time.
    fn read(&mut self, kind: LogKind, cx: &mut Context<Self>) {
        let start = match kind {
            LogKind::Activity => self.activity.start(),
            LogKind::Commands => self.commands.start(),
            LogKind::Screenshots => self.screenshots.start(),
        };
        let Some(next) = start else {
            return;
        };
        let path = self.path(kind);
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move { read_all(&path, next) })
                .await;
            let read = (read.inspect_err(|error| {
                ::log::warn!("couldn't read a terminal's {kind:?}: {error}");
            }))
            .ok();
            this.update(cx, |this, cx| {
                let (more, again) = match kind {
                    LogKind::Activity => this.activity.finish(read),
                    LogKind::Commands => this.commands.finish(read),
                    LogKind::Screenshots => this.screenshots.finish(read),
                };
                if again {
                    this.read(kind, cx);
                }
                if more {
                    if this.following {
                        this.scroll.scroll_to_bottom();
                    }
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// The terminal's record, for the Info tab.
    fn refresh_info(&mut self, cx: &mut Context<Self>) {
        let client = Arc::clone(&self.client);
        let handle = self.handle.clone();
        cx.spawn(async move |this, cx| {
            let listed = cx
                .background_spawn(async move {
                    client.request(RequestBody::ListTerminals {
                        worktree: None,
                        status: None,
                    })
                })
                .await;
            let Ok(ResponseBody::Terminals { terminals }) = listed else {
                return;
            };
            let record = (terminals.into_iter())
                .map(|summary| summary.record)
                .find(|record| record.handle == handle);
            this.update(cx, |this, cx| {
                this.info = record;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn show(&mut self, tab: PanelTab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.following = true;
        self.scroll.scroll_to_bottom();
        self.polling = (tab == PanelTab::Output).then(|| self.poll_output(cx));
        if tab == PanelTab::Info {
            self.refresh_info(cx);
        }
        cx.notify();
    }

    /// Marks where the Output tab's text starts: the output from now on.
    fn set_mark(&mut self, cx: &mut Context<Self>) {
        self.output = Output::default();
        self.read_output(None, cx);
    }

    fn poll_output(&self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let polled = this.update(cx, |this, cx| {
                    if this.output.mark.is_some() {
                        this.read_output(Some(this.output.next), cx);
                    }
                });
                if polled.is_err() {
                    return;
                }
            }
        })
    }

    /// Reads the output from `since`, or only where it ends now when `None`, which sets the
    /// mark. A read overtaken by another, or by a new mark, is dropped.
    fn read_output(&self, since: Option<u64>, cx: &mut Context<Self>) {
        let client = Arc::clone(&self.client);
        let max = if since.is_some() { OUTPUT_BYTES } else { 1 };
        let request = RequestBody::ReadStream {
            handle: self.handle.clone(),
            since,
            max_bytes: Some(u32::try_from(max).unwrap_or(u32::MAX)),
        };
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_spawn(async move { client.request(request) })
                .await;
            let Ok(ResponseBody::Stream { text, next, .. }) = read else {
                return;
            };
            this.update(cx, |this, cx| {
                let output = &mut this.output;
                match since {
                    None if output.mark.is_none() => output.mark = Some(next),
                    Some(since) if output.mark.is_some() && since == output.next => {
                        output.next = next;
                        if text.is_empty() {
                            return;
                        }
                        output.append(&text);
                    }
                    _ => return,
                }
                output.next = next;
                if this.following {
                    this.scroll.scroll_to_bottom();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// Every line `path` holds from `from` on.
fn read_all(path: &std::path::Path, from: ReadFrom) -> std::io::Result<LogRead> {
    let mut at = match from {
        ReadFrom::Offset(offset) => offset,
        ReadFrom::Tail => tail_start(path, TAIL_BYTES)?,
    };
    let mut all = LogRead::default();
    loop {
        let read = read_since(path, at, READ_BYTES)?;
        all.rotated |= read.rotated;
        at = read.next;
        if read.lines.is_empty() {
            all.next = at;
            return Ok(all);
        }
        all.lines.extend(read.lines);
    }
}

/// What the visual check reads of the panel, and the buttons it presses.
#[cfg(feature = "visual-check")]
impl ActivityPanel {
    pub fn actions(&self) -> Vec<String> {
        (self.activity.rows().iter())
            .map(|entry| entry.action.clone())
            .collect()
    }

    pub fn commands(&self) -> Vec<(String, Option<i32>)> {
        (self.commands.rows().iter())
            .map(|record| (record.command.clone(), record.exit_code))
            .collect()
    }

    /// The output since the mark, once it's set.
    pub fn output(&self) -> Option<&str> {
        self.output.mark.map(|_| self.output.text.as_str())
    }

    pub fn has_info(&self) -> bool {
        self.info.is_some()
    }

    pub fn press_failed_only(&mut self, cx: &mut Context<Self>) {
        self.failed_only = !self.failed_only;
        cx.notify();
    }

    pub fn press_mark(&mut self, cx: &mut Context<Self>) {
        self.set_mark(cx);
    }
}
