//! The card's git: its branch's pull request, and what can be done next with what's changed. What's
//! known of the branch is read off the main thread, as it asks the remote and GitHub, and goes old
//! after a while. It's read as the card is drawn, so a chat nobody's looking at asks nothing.

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AppContext as _, ClickEvent, Context, Div, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, px,
};
use koyo_git::{Checkout as Git, Published, PullRequest, PullRequestState};

use super::super::ChatView;
use super::{clickable, commit_dialog, counted, meta, row};
use crate::checkout::{Checkout, HeadMoved};
use crate::modal;
use crate::settings::{self, DetailsItem};
use crate::shell::ellipsized;
use crate::theme::Palette;

/// How long what's known of the branch stays good.
const GOOD_FOR: Duration = Duration::from_secs(60);
/// How recent a read has to be for a window active again not to want another: a person may only
/// have been in one of Koyo's own menus or dialogs, which are windows too.
const RECENT: Duration = Duration::from_secs(10);

/// What's known of the branch.
struct Facts {
    published: Published,
    pull_request: Option<PullRequest>,
    /// Whether GitHub answered, so a pull request can be opened.
    on_github: bool,
}

/// What can be done with what's committed.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    Publish,
    Push(u32),
    Pull(u32),
    /// Neither can be: its branch and its upstream each have what the other hasn't.
    Diverged {
        ahead: u32,
        behind: u32,
    },
    CreatePullRequest,
}

/// What GitHub said of the branch, which it may not: that's also what a person offline or signed
/// out gets, so what it said last holds.
enum Asked {
    NoAnswer,
    Answered(Option<PullRequest>),
}

#[derive(Default)]
pub(super) struct GitState {
    facts: Option<Facts>,
    /// Reads the branch and waits for that to go old; with nothing here, it's to be read.
    fresh: Option<Task<()>>,
    read_at: Option<Instant>,
    /// What's being done now, which is all that's done at a time.
    doing: Option<&'static str>,
    error: Option<SharedString>,
    _head: Option<Subscription>,
}

impl GitState {
    /// It's another branch: nothing known of the last one holds.
    pub(super) fn moved(&mut self, checkout: &Entity<Checkout>, cx: &mut Context<ChatView>) {
        self.facts = None;
        self.fresh = None;
        self.read_at = None;
        self.error = None;
        self._head = Some(cx.subscribe(checkout, |view, _, _: &HeadMoved, cx| went_old(view, cx)));
    }

    fn heard(&mut self, published: Published, asked: Asked) {
        let known = self.facts.take();
        let (pull_request, on_github) = match asked {
            Asked::Answered(pull_request) => (pull_request, true),
            Asked::NoAnswer => {
                known.map_or((None, false), |known| (known.pull_request, known.on_github))
            }
        };
        self.facts = Some(Facts {
            published,
            pull_request,
            on_github,
        });
    }
}

fn next(facts: &Facts) -> Option<Next> {
    let Published {
        remote,
        upstream,
        ahead,
        behind,
        on_default,
    } = &facts.published;
    remote.as_ref()?;
    if upstream.is_none() {
        return Some(Next::Publish);
    }
    let lone = facts.on_github && facts.pull_request.is_none() && !on_default;
    match (*ahead, *behind) {
        (0, 0) => lone.then_some(Next::CreatePullRequest),
        (ahead, 0) => Some(Next::Push(ahead)),
        (0, behind) => Some(Next::Pull(behind)),
        (ahead, behind) => Some(Next::Diverged { ahead, behind }),
    }
}

/// Reads the branch, unless what's known of it is still good or nothing would show it. Each step
/// runs on its own, so a read let go of takes none after the one it's on.
pub(in super::super) fn keep_fresh(view: &mut ChatView, cx: &mut Context<ChatView>) {
    let shown = view.details.is_up(cx) && settings::get(cx).chat_details.shows(DetailsItem::Git);
    if view.details.git.fresh.is_some() || !shown {
        return;
    }
    let Some(git) = view
        .details
        .place
        .as_ref()
        .and_then(|place| place.checkout.read(cx).git())
    else {
        return;
    };
    view.details.git.fresh = Some(cx.spawn(async move |this, cx| {
        let here = git.clone();
        let published = cx.background_spawn(async move { here.published() }).await;
        // What's known here shows first: the network may take a while, or never answer.
        this.update(cx, |view, cx| {
            view.details.git.heard(published, Asked::NoAnswer);
            cx.notify();
        })
        .ok();
        let fetching = git.clone();
        // Without the network, what's known here still holds.
        cx.background_spawn(async move { fetching.fetch().ok() })
            .await;
        let (published, asked) = cx
            .background_spawn(async move {
                let published = git.published();
                let asks = published.upstream.is_some() && !published.on_default;
                let asked = (asks.then(|| git.pull_request().ok()).flatten())
                    .map_or(Asked::NoAnswer, Asked::Answered);
                (published, asked)
            })
            .await;
        this.update(cx, |view, cx| {
            view.details.git.heard(published, asked);
            view.details.git.read_at = Some(Instant::now());
            cx.notify();
        })
        .ok();
        cx.background_executor().timer(GOOD_FOR).await;
        this.update(cx, went_old).ok();
    }));
}

/// What's known of the branch is to be read again, by the next drawing.
pub(in crate::chat) fn went_old(view: &mut ChatView, cx: &mut Context<ChatView>) {
    view.details.git.fresh = None;
    cx.notify();
}

/// The window's active again: a person may have done something with the branch elsewhere.
pub(in super::super) fn window_active_again(view: &mut ChatView, cx: &mut Context<ChatView>) {
    let recent = view
        .details
        .git
        .read_at
        .is_some_and(|at| at.elapsed() < RECENT);
    if !recent {
        went_old(view, cx);
    }
}

/// Does `does`, called `what`, and says if it went wrong. The checkout says `HEAD` moved after,
/// which has the branch read again.
fn act(
    view: &mut ChatView,
    what: &'static str,
    does: fn(&Git) -> koyo_git::Result<()>,
    cx: &mut Context<ChatView>,
) {
    let Some(checkout) = view
        .details
        .place
        .as_ref()
        .map(|place| place.checkout.clone())
    else {
        return;
    };
    let git = &mut view.details.git;
    if git.doing.is_some() {
        return;
    }
    git.doing = Some(what);
    git.error = None;
    cx.notify();
    let running = checkout.update(cx, |checkout, cx| checkout.run(does, cx));
    cx.spawn(async move |this, cx| {
        let done = running.await;
        this.update(cx, |view, cx| {
            let git = &mut view.details.git;
            git.doing = None;
            if let Err(error) = done {
                modal::fail(&mut git.error, what, error.message);
            }
            cx.notify();
        })
        .ok();
    })
    .detach();
}

/// The section: the pull request's line, and what can be done next.
pub(super) fn rows(view: &ChatView, p: Palette, cx: &Context<ChatView>) -> Option<Div> {
    let place = view.details.place.as_ref()?;
    let state = &view.details.git;
    let files = place.checkout.read(cx).files().len();
    let facts = state.facts.as_ref();
    let pull_request = facts.and_then(|facts| facts.pull_request.as_ref());
    let next = facts.and_then(next);
    let empty = pull_request.is_none() && files == 0 && next.is_none() && state.error.is_none();
    let section = div()
        .flex()
        .flex_col()
        .children(pull_request.map(|pull| pull_request_row(pull, p)))
        .when(files > 0, |section| section.child(commit_row(files, p, cx)))
        .children(next.map(|next| next_row(view, &next, p, cx)))
        .children(state.error.clone().map(|error| {
            div()
                .px(px(10.))
                .py(px(4.))
                .text_xs()
                .text_color(p.danger)
                .child(error)
        }));
    (!empty).then_some(section)
}

fn next_row(view: &ChatView, next: &Next, p: Palette, cx: &Context<ChatView>) -> AnyElement {
    let push = IconName::ArrowUp;
    let (what, mark, says, does): (_, _, _, fn(&Git) -> koyo_git::Result<()>) = match next {
        Next::Publish => ("publish", push, "Publish branch".to_owned(), Git::push),
        Next::Push(ahead) => (
            "push",
            push,
            format!("Push {}", counted(*ahead, "commit")),
            Git::push,
        ),
        Next::Pull(behind) => (
            "pull",
            IconName::ArrowDown,
            format!("Pull {}", counted(*behind, "commit")),
            Git::pull,
        ),
        Next::CreatePullRequest => (
            "create a pull request",
            IconName::GitPullRequest,
            "Create pull request".to_owned(),
            Git::create_pull_request,
        ),
        Next::Diverged { ahead, behind } => {
            let says = format!("{ahead} ahead, {behind} behind its upstream");
            return row(IconName::GitBranch, p)
                .child(ellipsized(says).flex_1())
                .into_any_element();
        }
    };
    let busy = view.details.git.doing == Some(what);
    let line = row(mark, p)
        .child(ellipsized(says).flex_1())
        .when(busy, |line| {
            line.child(
                div()
                    .relative()
                    .flex_shrink_0()
                    .size(px(13.))
                    .child(modal::spinner(p.muted)),
            )
        });
    clickable(line, what, p)
        .debug_selector(move || what.to_owned())
        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| act(view, what, does, cx)))
        .into_any_element()
}

fn commit_row(files: usize, p: Palette, cx: &Context<ChatView>) -> impl IntoElement + use<> {
    let says = format!("Commit {}…", counted(files, "file"));
    let line = row(IconName::Check, p).child(ellipsized(says).flex_1());
    clickable(line, "chat-details-commit", p).on_click(
        cx.listener(move |view, _: &ClickEvent, window, cx| commit_files(view, files, window, cx)),
    )
}

/// Opens the dialog that commits `files`.
pub(in crate::chat) fn commit_files(
    view: &ChatView,
    files: usize,
    window: &mut Window,
    cx: &mut Context<ChatView>,
) {
    if let Some(place) = &view.details.place {
        commit_dialog::open(files, place.checkout.clone(), window, cx);
    }
}

fn pull_request_row(pull: &PullRequest, p: Palette) -> Stateful<Div> {
    let (word, color) = match pull.state {
        PullRequestState::Open => ("Open", p.added),
        PullRequestState::Draft => ("Draft", p.muted),
        PullRequestState::Merged => ("Merged", p.text),
        PullRequestState::Closed => ("Closed", p.danger),
    };
    let url = pull.url.clone();
    let line = row(IconName::GitPullRequest, p)
        .child(ellipsized(format!("#{} {}", pull.number, pull.title)).flex_1())
        .child(meta(word, color));
    clickable(line, "chat-details-pull-request", p).on_click(move |_, _, cx| cx.open_url(&url))
}

#[cfg(test)]
mod tests {
    use koyo_git::Ref;

    use super::*;

    fn facts(tracked: bool, (ahead, behind): (u32, u32)) -> Facts {
        let upstream = Ref::Remote {
            remote: "origin".to_owned(),
            branch: "work".to_owned(),
        };
        Facts {
            published: Published {
                remote: Some("origin".to_owned()),
                upstream: tracked.then_some(upstream),
                ahead,
                behind,
                on_default: false,
            },
            pull_request: None,
            on_github: true,
        }
    }

    fn pull_request() -> PullRequest {
        PullRequest {
            number: 1,
            title: "Title".to_owned(),
            state: PullRequestState::Open,
            url: "https://example.com".to_owned(),
        }
    }

    #[test]
    fn what_can_be_done_next_goes_by_how_the_branch_stands() {
        assert_eq!(next(&facts(false, (0, 0))), Some(Next::Publish));
        assert_eq!(next(&facts(true, (2, 0))), Some(Next::Push(2)));
        assert_eq!(next(&facts(true, (0, 3))), Some(Next::Pull(3)));
        assert_eq!(
            next(&facts(true, (2, 3))),
            Some(Next::Diverged {
                ahead: 2,
                behind: 3
            })
        );
        assert_eq!(next(&facts(true, (0, 0))), Some(Next::CreatePullRequest));
    }

    #[test]
    fn a_pull_request_is_only_offered_where_there_can_be_a_new_one() {
        let settled = || facts(true, (0, 0));
        let not_github = Facts {
            on_github: false,
            ..settled()
        };
        assert_eq!(next(&not_github), None);
        let mut default_branch = settled();
        default_branch.published.on_default = true;
        assert_eq!(next(&default_branch), None);
        assert_eq!(
            next(&Facts {
                pull_request: Some(pull_request()),
                ..settled()
            }),
            None
        );
    }

    #[test]
    fn nothing_can_be_done_without_a_remote() {
        let mut lone = facts(false, (0, 0));
        lone.published.remote = None;
        assert_eq!(next(&lone), None);
    }

    #[test]
    fn what_github_said_holds_while_it_doesnt_answer() {
        let published = facts(true, (0, 0)).published;
        let mut state = GitState::default();
        state.heard(published.clone(), Asked::Answered(Some(pull_request())));
        state.heard(published.clone(), Asked::NoAnswer);
        let known = state.facts.as_ref().unwrap();
        assert!(known.on_github && known.pull_request == Some(pull_request()));
        state.heard(published, Asked::Answered(None));
        let known = state.facts.as_ref().unwrap();
        assert!(known.on_github && known.pull_request.is_none());
    }
}
