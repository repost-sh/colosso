//! The launcher's state: the kind picked, what's typed and the host completing it in the field,
//! the rows for them and the row picked, and the search engine's suggestions as they come. Text
//! dismissed is offered again for a while, as Zen keeps it. Opened on a page (⌘L), the field
//! starts with the page's address, and a page or a search picked loads in that page.

use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, Focusable as _, Global, Subscription,
    Task, WeakEntity, Window,
};

use super::Kind;
use super::here::Here;
use super::rows::{self, Act, Query, Row};
use crate::history;
use crate::list::stepped;
use crate::modal;
use crate::shell::Shell;

/// How wide the launcher is.
pub const WIDTH: f32 = 680.;
/// How high its field is, a row, the room round the rows and the line over them.
pub const FIELD: f32 = 56.;
pub const ROW: f32 = 38.;
pub const LIST_PADDING: f32 = 6.;
/// How long text typed and dismissed is offered again, as Zen's `zen.urlbar.wait-to-clear`.
const REMEMBERED: Duration = Duration::from_secs(45);

/// Text typed in the launcher when it was dismissed, and its kind.
struct Dismissed {
    text: String,
    kind: Kind,
    at: Instant,
}

impl Global for Dismissed {}

/// The page the launcher opened on (⌘L): what's typed loads in it rather than in a new tab.
pub struct Page {
    /// Its pane.
    pub pane: u64,
    /// Its address as the launcher opened, which the field starts with.
    pub address: String,
}

/// The text dismissed lately and its kind, taken; nothing, for a browser tab, otherwise.
fn take_dismissed(cx: &mut App) -> (String, Kind) {
    let kept = match cx.try_global::<Dismissed>() {
        Some(dismissed) if dismissed.at.elapsed() < REMEMBERED => {
            (dismissed.text.clone(), dismissed.kind)
        }
        _ => (String::new(), Kind::Browser),
    };
    if cx.has_global::<Dismissed>() {
        cx.remove_global::<Dismissed>();
    }
    kept
}

/// Opens the launcher over `window`, which `shell` shows, for a tab on the checkout `here` says,
/// or on `page`, whose address it starts with.
pub fn open(
    shell: WeakEntity<Shell>,
    here: Here,
    page: Option<Page>,
    window: &mut Window,
    cx: &mut App,
) {
    let behind = window.window_handle();
    let (text, kind) = match &page {
        // Its address whatever was dismissed lately, which is left for the next ⌘T.
        Some(page) => (page.address.clone(), Kind::Browser),
        None => take_dismissed(cx),
    };
    let query = Query {
        kind,
        typed: &text,
        autofilled: None,
        here: &here,
        suggested: &[],
    };
    let rows = rows::rows(&query, cx);
    modal::open_floating(WIDTH, height(rows.len()), window, cx, |window, cx| {
        Launcher::new((shell, behind), here, page, (text, kind), rows, window, cx)
    });
}

/// How high the launcher is with `rows` rows.
pub fn height(rows: usize) -> f32 {
    if rows == 0 {
        FIELD
    } else {
        FIELD + 1. + 2. * LIST_PADDING + rows as f32 * ROW
    }
}

pub struct Launcher {
    pub(super) input: Entity<InputState>,
    pub(super) kind: Kind,
    pub(super) rows: Vec<Row>,
    /// The row Enter picks: none with nothing typed, so Enter opens an empty tab.
    pub(super) selected: Option<usize>,
    /// What the person typed, a completion aside.
    typed: String,
    /// The host completing what's typed, selected in the field after it, and the hosts that may,
    /// worked out once something's typed.
    autofilled: Option<String>,
    origins: Option<history::Origins>,
    here: Here,
    /// The page it opened on, if it did.
    page: Option<Page>,
    /// The search engine's suggestions for what's typed, and the fetch on its way.
    suggested: Vec<String>,
    suggesting: Option<Task<()>>,
    shell: WeakEntity<Shell>,
    /// The window the launcher sits over, where tabs open.
    behind: AnyWindowHandle,
    /// Whether a row was picked, so what's typed isn't kept as dismissed.
    picked: bool,
    /// The height the window was last made, so it's only asked to change when the rows do.
    pub(super) fitted: f32,
    _subscriptions: Vec<Subscription>,
}

impl Launcher {
    /// The launcher over the window `behind`, which `shell` shows, on the checkout `here`
    /// says and on `page` if it opened on one, starting with `text` for a tab of `kind`.
    fn new(
        (shell, behind): (WeakEntity<Shell>, AnyWindowHandle),
        here: Here,
        page: Option<Page>,
        (text, kind): (String, Kind),
        rows: Vec<Row>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(kind.placeholder()));
        if !text.is_empty() {
            input.update(cx, |input, cx| {
                input.set_value(text.clone(), window, cx);
                input.select_all(window, cx);
            });
        }
        input.focus_handle(cx).focus(window, cx);
        let _subscriptions = vec![
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.typed_changed(window, cx),
                InputEvent::PressEnter { .. } => this.pick(this.selected, window, cx),
                _ => {}
            }),
            // Gone without a pick, what's typed is offered again when it next opens; not a
            // page's address left as it was, which nobody typed.
            cx.on_release(|this, cx| {
                let untouched = (this.page.as_ref()).is_some_and(|page| page.address == this.typed);
                if !this.picked && !untouched && !this.typed.trim().is_empty() {
                    let (text, kind) = (this.typed.clone(), this.kind);
                    cx.set_global(Dismissed {
                        text,
                        kind,
                        at: Instant::now(),
                    });
                }
            }),
        ];
        let selected = (!text.trim().is_empty() && !rows.is_empty()).then_some(0);
        let mut launcher = Launcher {
            input,
            kind,
            fitted: height(rows.len()),
            rows,
            selected,
            typed: text,
            autofilled: None,
            origins: None,
            here,
            page,
            suggested: Vec::new(),
            suggesting: None,
            shell,
            behind,
            picked: false,
            _subscriptions,
        };
        launcher.suggest(cx);
        launcher
    }

    /// What's typed changed: a host completes it if it grew at its end, as Firefox autofills, and
    /// the rows and suggestions follow.
    fn typed_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (value, cursor) = {
            let input = self.input.read(cx);
            (input.value().to_string(), input.cursor())
        };
        let grew = value.len() > self.typed.len() && value.starts_with(&self.typed);
        self.typed = value;
        self.autofilled = None;
        if self.kind == Kind::Browser && grew && cursor == self.typed.len() {
            self.autofill(window, cx);
        }
        self.requery(cx);
    }

    /// Completes what's typed with the host the history offers, the completion selected so typing
    /// on replaces it.
    fn autofill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.typed.is_ascii() {
            return;
        }
        let origins =
            (self.origins).get_or_insert_with(|| history::get(cx).origins(history::now()));
        let typed = &self.typed;
        let host = origins.complete(typed);
        let Some(rest) = host
            .as_ref()
            .and_then(|host| host.get(typed.len()..))
            .filter(|rest| !rest.is_empty())
        else {
            return;
        };
        let filled = format!("{typed}{rest}");
        let at = typed.len()..filled.len();
        self.input.update(cx, |input, cx| {
            input.set_value(filled, window, cx);
            input.set_selected_range(at, cx);
        });
        self.autofilled = host;
    }

    /// The rows and the search engine's suggestions follow what's typed, or the kind.
    fn requery(&mut self, cx: &mut Context<Self>) {
        self.refresh(cx);
        self.suggest(cx);
    }

    /// Makes the rows for what's typed: the first picked once something is, none with nothing.
    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        let query = Query {
            kind: self.kind,
            typed: &self.typed,
            autofilled: self.autofilled.as_deref(),
            here: &self.here,
            suggested: &self.suggested,
        };
        self.rows = rows::rows(&query, cx);
        self.selected = (!self.typed.trim().is_empty() && !self.rows.is_empty()).then_some(0);
        cx.notify();
    }

    /// Asks the search engine what it suggests for what's typed, in a browser, once typing pauses
    /// and if the person lets it; suggestions that no longer fit what's typed go meanwhile.
    fn suggest(&mut self, cx: &mut Context<Self>) {
        let typed = self.typed.trim().to_owned();
        let lower = typed.to_lowercase();
        self.suggested
            .retain(|suggested| suggested.to_lowercase().starts_with(&lower));
        self.suggesting = None;
        if self.kind != Kind::Browser || !super::suggest::wanted(&typed, cx) {
            self.suggested.clear();
            return;
        }
        let fetched = super::suggest::fetch(typed.clone(), cx);
        self.suggesting = Some(cx.spawn(async move |this, cx| {
            let Some(suggested) = fetched.await else {
                return;
            };
            this.update(cx, |this, cx| {
                if this.typed.trim() == typed {
                    this.suggested = suggested;
                    let selected = this.selected;
                    this.refresh(cx);
                    this.selected = selected
                        .filter(|at| *at < this.rows.len())
                        .or(this.selected);
                }
            })
            .ok();
        }));
    }

    /// Goes to the kind `by` steps on, what's typed kept, a completion let go.
    pub(super) fn step_kind(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.kind = self.kind.step(by);
        let (typed, kind) = (self.typed.clone(), self.kind);
        let completed = self.autofilled.take().is_some();
        self.input.update(cx, |input, cx| {
            input.set_placeholder(kind.placeholder(), window, cx);
            if completed {
                input.set_value(typed, window, cx);
            }
        });
        self.requery(cx);
    }

    /// Moves the pick `by` rows, from none to the first going down or the last going up.
    pub(super) fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        self.selected = stepped(self.selected, by, self.rows.len());
        cx.notify();
    }

    /// → takes the completion in the field, or the picked row's text into it, at the field's end;
    /// elsewhere it moves the caret.
    pub(super) fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (value, cursor, selection) = {
            let input = self.input.read(cx);
            (
                input.value().to_string(),
                input.cursor(),
                input.selected_range(),
            )
        };
        let text = if self.autofilled.is_some() && selection.end == value.len() {
            value
        } else {
            let row = self.selected.and_then(|at| self.rows.get(at));
            match row.and_then(Row::text) {
                Some(text) if cursor == value.len() && text != value => text.to_owned(),
                _ => return cx.propagate(),
            }
        };
        let end = text.len();
        self.input.update(cx, |input, cx| {
            input.set_value(text.clone(), window, cx);
            input.set_selected_range(end..end, cx);
        });
        (self.typed, self.autofilled) = (text, None);
        self.requery(cx);
    }

    /// Forgets the picked row's entry, if it shows one; otherwise ⇧⌫ edits the field.
    pub(super) fn remove_selected(&mut self, cx: &mut Context<Self>) {
        let entry = self
            .selected
            .and_then(|at| self.rows.get(at)?.entry.clone());
        let Some((kind, text)) = entry else {
            return cx.propagate();
        };
        self.remove(kind, &text, cx);
    }

    /// Forgets the history's entry of `kind` with `text`, the pick staying where it was.
    pub(super) fn remove(&mut self, kind: history::Kind, text: &str, cx: &mut Context<Self>) {
        let selected = self.selected;
        history::remove(kind, text, cx);
        self.refresh(cx);
        self.selected = selected.and_then(|at| stepped(Some(at), 0, self.rows.len()));
    }

    /// ⌘L again: the field's text all selected, as an address bar's is.
    pub(super) fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.select_all(window, cx));
    }

    /// Picks row `at`, or with none picked opens an empty tab of the kind: the launcher closes and
    /// the window behind does it.
    pub(super) fn pick(&mut self, at: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.typed.trim().to_owned();
        let row = at.and_then(|at| self.rows.get(at));
        let act = match row {
            Some(row) => row.act.clone(),
            None if self.kind == Kind::Worktree => Act::StartWorktree(typed.clone()),
            // On a page, no address is nowhere to go, as in any address bar.
            None if self.kind == Kind::Browser && self.page.is_some() => return,
            None if typed.is_empty() => Act::Empty(self.kind),
            None => return,
        };
        // What was typed led to an entry the history has, or a page a tab shows: typing it again
        // offers that first.
        let learned = match (row.and_then(|row| row.entry.clone()), &act) {
            (Some((kind, text)), _) => Some((kind, text)),
            (None, Act::Switch(pane)) => (self.here.tabs.iter())
                .find(|tab| tab.pane == *pane && !tab.url.is_empty())
                .map(|tab| (history::Kind::Page, tab.url.clone())),
            _ => None,
        };
        if let Some((kind, text)) = learned {
            history::picked(kind, &typed, &text, cx);
        }
        self.picked = true;
        let (shell, behind, place) = (self.shell.clone(), self.behind, self.here.place.clone());
        let page = self.page.as_ref().map(|page| page.pane);
        modal::dismiss(window, cx);
        super::act::launch(&shell, behind, act, (&place, page), cx);
    }
}
