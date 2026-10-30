//! The palette's state: the query, what the finder found for it, the row picked, and the
//! checkout's files followed as they change while it's open. Searches run in the background,
//! each giving way to the next; the main thread only swaps their results in, and moving the
//! pick touches nothing but the pick.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use colosso_project::{Feed, Finder, MOST_FOUND, Results};
use futures::StreamExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Focusable as _, ScrollStrategy,
    SharedString, Subscription, Task, UniformListScrollHandle, WeakEntity, Window,
};

use super::render::Row;
use crate::checkout::Checkout;
use crate::files::{At, Place, ROW};
use crate::list::{page, stepped};
use crate::modal;
use crate::shell::Shell;

/// How often the checkout's files are taken up again while its first scan lists them, when
/// listings come by the hundred.
const WHILE_SCANNING: Duration = Duration::from_millis(300);

/// Opens the palette over `window`, on `checkout`, searched by `finder`; `recent` are the
/// files opened from it lately, the latest first.
pub fn open(
    shell: WeakEntity<Shell>,
    checkout: Entity<Checkout>,
    finder: Arc<Finder>,
    recent: &[Arc<str>],
    window: &mut Window,
    cx: &mut App,
) {
    let behind = window.window_handle();
    modal::open_palette(window, cx, |window, cx| {
        GoToFile::new(shell, checkout, finder, recent, behind, window, cx)
    });
}

pub struct GoToFile {
    pub(super) input: Entity<InputState>,
    finder: Arc<Finder>,
    checkout: Entity<Checkout>,
    pub(super) results: Results,
    /// Each row as it's drawn, made the first time it's shown.
    pub(super) rows: Vec<Option<Row>>,
    /// The line under the rows, made when what it says changes.
    pub(super) status: SharedString,
    /// Whether the checkout's files are all listed, so what wasn't found isn't there.
    pub(super) listed: bool,
    pub(super) selected: usize,
    pub(super) scroll: UniformListScrollHandle,
    searching: Option<Task<()>>,
    /// Takes up the checkout's files as they change, once its project is open.
    following: Option<Task<()>>,
    shell: WeakEntity<Shell>,
    /// The window the palette sits over, where files open.
    behind: AnyWindowHandle,
    _subscriptions: [Subscription; 2],
}

impl GoToFile {
    fn new(
        shell: WeakEntity<Shell>,
        checkout: Entity<Checkout>,
        finder: Arc<Finder>,
        recent: &[Arc<str>],
        behind: AnyWindowHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Go to file"));
        input.focus_handle(cx).focus(window, cx);
        let _subscriptions = [
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.search(cx),
                InputEvent::PressEnter { .. } => this.open(this.selected, window, cx),
                _ => {}
            }),
            // The checkout's project opens a moment after the checkout does.
            cx.observe(&checkout, |this, _, cx| this.follow(cx)),
        ];
        // Told once, before the first search, and never waited on.
        finder.set_recent(recent);
        let mut palette = GoToFile {
            input,
            finder,
            checkout,
            results: Results::default(),
            rows: Vec::new(),
            status: SharedString::default(),
            listed: false,
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            searching: None,
            following: None,
            shell,
            behind,
            _subscriptions,
        };
        palette.search(cx);
        palette.follow(cx);
        palette
    }

    /// Takes up the checkout's files now, and again each time a listing changes, while the
    /// palette is open.
    fn follow(&mut self, cx: &mut Context<Self>) {
        if self.following.is_some() {
            return;
        }
        let Some(project) = self.checkout.read(cx).project().cloned() else {
            return;
        };
        let mut updates = project.subscribe(Feed::Listings);
        let finder = self.finder.clone();
        self.following = Some(cx.spawn(async move |this, cx| {
            loop {
                let (finder, project) = (finder.clone(), project.clone());
                let changed = (cx
                    .background_spawn(async move { finder.set_files(&project.files_by_folder()) }))
                .await;
                let listed = updates.project().scanned().is_some();
                let followed = this.update(cx, |this, cx| {
                    if changed {
                        this.search(cx);
                    }
                    if this.listed != listed {
                        this.listed = listed;
                        this.status = this.status_line();
                        cx.notify();
                    }
                });
                if followed.is_err() {
                    return;
                }
                if !listed {
                    cx.background_executor().timer(WHILE_SCANNING).await;
                }
                // A listing changed, or the scan's done. The subscription merges whatever
                // came meanwhile into the one update, so there's never a backlog to drain.
                if updates.next().await.is_none() {
                    return;
                }
            }
        }));
    }

    /// Searches for what's typed, in the background, and shows what's found once it's back,
    /// unless a later search was asked for meanwhile, which it gives way to.
    fn search(&mut self, cx: &mut Context<Self>) {
        let typed = self.input.read(cx).value().to_string();
        let finder = self.finder.clone();
        let found = cx.background_spawn(async move { finder.search(&typed, MOST_FOUND) });
        self.searching = Some(cx.spawn(async move |this, cx| {
            if let Some(results) = found.await {
                this.update(cx, |this, cx| this.show(results, cx)).ok();
            }
        }));
    }

    /// Shows `results`: the first picked for a new query, or the file picked before kept
    /// picked as the files change under the same one. Their rows are made as they're shown.
    fn show(&mut self, results: Results, cx: &mut Context<Self>) {
        let picked = (self.results.query == results.query)
            .then(|| self.results.found.get(self.selected))
            .flatten()
            .and_then(|picked| (results.found.iter()).position(|found| found.path == picked.path));
        self.selected = picked.unwrap_or(0);
        if picked.is_none() {
            self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        self.rows = std::iter::repeat_with(|| None)
            .take(results.found.len())
            .collect();
        self.results = results;
        self.status = self.status_line();
        cx.notify();
    }

    /// Moves the pick `by` rows, stopping at either end.
    pub(super) fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let len = self.results.found.len();
        self.selected = stepped(Some(self.selected), by, len).unwrap_or(0);
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    pub(super) fn page(&mut self, direction: isize, cx: &mut Context<Self>) {
        self.step(direction * page(&self.scroll, ROW), cx);
    }

    /// Shows the file at row `index` in the window behind as the Files panel shows the files
    /// picked in it, closing the palette; at the line typed after a colon, if one was.
    pub(super) fn open(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(found) = self.results.found.get(index) else {
            return;
        };
        // Lines are counted from 1 as typed, from 0 as the editor's told.
        let at = (self.results.query.line).map(|line| At::Line(line.saturating_sub(1)));
        let place = Place {
            path: Path::new(&*found.path).to_owned(),
            at,
        };
        modal::dismiss(window, cx);
        Shell::view_behind(&self.shell, self.behind, place, cx);
    }
}

impl Drop for GoToFile {
    /// What the searches kept for erasing back to is let go once the palette's gone; the
    /// finder never makes this wait.
    fn drop(&mut self) {
        self.finder.forget();
    }
}
