//! What a tab and a checkout's row say of a chat (`docs/chat.md` 5.3): a spinner while it works, a
//! dot in the text's colour when it needs you, red if it failed and a muted dot if it finished,
//! until a person has seen it. The spinner moves only while the app's in front, in steps, and as a
//! view of its own: a turn can take a long time, and each step draws only it, not the shell.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::{
    Animation, AnimationExt as _, App, AppContext as _, Context, Div, Entity, Global, IntoElement,
    ParentElement as _, Render, Styled as _, Transformation, Window, div, percentage, px,
};
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::methods::{ThreadParams, ThreadsRead};
use koyo_agent::protocol::model::{Status, ThreadSummary};

use super::{Chats, ChatsEvent, logged};
use crate::model::Tab;
use crate::pane::dot;
use crate::shell::{Shell, icon};
use crate::theme::{self, Palette};

const SPINNER: f32 = 12.;
const TURN: Duration = Duration::from_secs(1);
/// How often the spinner moves: every 30 degrees.
const SPINNER_FPS: f32 = 12.;

/// Whether the person is at the app, as far as a chat cares.
pub(super) fn is_frontmost(cx: &App) -> bool {
    cx.active_window().is_some()
}

/// How a chat is doing, least urgent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ChatMark {
    Unread,
    Working,
    Failed,
    NeedsYou,
}

impl ChatMark {
    fn of(summary: &ThreadSummary) -> Option<Self> {
        if summary.needs.is_some() {
            return Some(ChatMark::NeedsYou);
        }
        match summary.status {
            Status::Working => Some(ChatMark::Working),
            Status::Failed => summary.unread.then_some(ChatMark::Failed),
            Status::Idle => summary.unread.then_some(ChatMark::Unread),
        }
    }

    /// As a tab or a row shows it, at the place that names.
    fn shown(self, place: String, p: Palette, cx: &mut App) -> Div {
        match self {
            ChatMark::Working => spinner(place, cx),
            ChatMark::NeedsYou => dot(p.text),
            ChatMark::Failed => dot(p.danger),
            ChatMark::Unread => dot(p.muted),
        }
    }
}

/// A spinner where a person sees it, whose steps draw it alone.
struct Spin;

impl Render for Spin {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let spinner = icon(IconName::LoaderCircle, SPINNER).text_color(theme::palette(cx).muted);
        let holder = div().size(px(SPINNER));
        if is_frontmost(cx) {
            let turning = Animation::new(TURN).repeat().with_max_fps(SPINNER_FPS);
            holder.child(
                spinner.with_animation("chat-working", turning, |spinner, delta| {
                    spinner.transform(Transformation::rotate(percentage(delta)))
                }),
            )
        } else {
            holder.child(spinner)
        }
    }
}

/// The spinners by place, each of which is a view drawn in one spot, for as long as a chat works.
#[derive(Default)]
struct Spinners(HashMap<String, Entity<Spin>>);

impl Global for Spinners {}

fn spinner(place: String, cx: &mut App) -> Div {
    let held = cx.default_global::<Spinners>().0.get(&place).cloned();
    let spin = held.unwrap_or_else(|| {
        let spin = cx.new(|_| Spin);
        cx.default_global::<Spinners>()
            .0
            .insert(place, spin.clone());
        spin
    });
    div().flex_shrink_0().child(spin)
}

impl Chats {
    /// A person's chats, a subagent's not among them, in the checkout at `worktree` if it's given,
    /// latest first.
    pub(crate) fn latest_first(&self, worktree: Option<&Path>) -> Vec<&ThreadSummary> {
        let in_it = |summary: &&ThreadSummary| {
            worktree.is_none_or(|worktree| Path::new(&summary.cwd) == worktree)
        };
        let mut chats: Vec<&ThreadSummary> = self.summaries.values().filter(in_it).collect();
        chats.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        chats
    }

    /// The summaries changed: what the rows say follows, and whoever shows them hears.
    pub(super) fn summaries_changed(&mut self, cx: &mut Context<Self>) {
        self.checkout_marks = checkout_marks(self.summaries.values());
        let working = self
            .summaries
            .values()
            .any(|summary| summary.status == Status::Working);
        if !working && cx.has_global::<Spinners>() {
            cx.remove_global::<Spinners>();
        }
        cx.emit(ChatsEvent::Summaries);
    }

    /// A person has seen thread `id`, so what it says it has new isn't.
    pub(super) fn saw(&mut self, id: &ThreadId, cx: &mut Context<Self>) {
        if let Some(summary) = self.summaries.get_mut(id).filter(|summary| summary.unread) {
            summary.unread = false;
            let params = ThreadParams {
                thread_id: id.clone(),
            };
            logged(
                self.ordered::<ThreadsRead>(params),
                "noting a chat was seen",
                cx,
            );
            self.summaries_changed(cx);
        }
    }
}

/// The most urgent of each checkout's chats: those with tabs, and any other still at it, as one a
/// person closed has nothing new to say until it's opened.
fn checkout_marks<'a>(
    summaries: impl Iterator<Item = &'a ThreadSummary>,
) -> HashMap<PathBuf, ChatMark> {
    let mut marks = HashMap::new();
    for summary in summaries {
        let Some(mark) = ChatMark::of(summary) else {
            continue;
        };
        if summary.tab_open || matches!(mark, ChatMark::Working | ChatMark::NeedsYou) {
            let held = marks.entry(PathBuf::from(&summary.cwd)).or_insert(mark);
            *held = mark.max(*held);
        }
    }
    marks
}

impl Shell {
    /// The mark on a tab with chats, the most urgent of theirs.
    pub fn chat_tab_mark(&self, tab: &Tab, p: Palette, cx: &mut App) -> Option<Div> {
        let chats = self.chat_tabs.chats.read(cx);
        let mark = (tab.panes().into_iter())
            .filter_map(|pane| chats.summary(pane.chat.as_ref()?).and_then(ChatMark::of))
            .max()?;
        Some(mark.shown(format!("tab-{}", tab.first().id), p, cx))
    }

    /// The mark on the row of the checkout at `worktree`.
    pub fn checkout_chat_mark(&self, worktree: &Path, p: Palette, cx: &mut App) -> Option<Div> {
        let mark = *self.chat_tabs.chats.read(cx).checkout_marks.get(worktree)?;
        Some(mark.shown(format!("row-{}", worktree.display()), p, cx))
    }
}

#[cfg(test)]
impl Chats {
    /// Takes `summaries` in as the host would say them.
    pub(crate) fn hear_summaries(&mut self, summaries: Vec<ThreadSummary>, cx: &mut Context<Self>) {
        let said = summaries.into_iter().map(|summary| {
            koyo_agent::agent_client::Notice::Event(Box::new(
                koyo_agent::protocol::events::Event::ThreadSummary(summary),
            ))
        });
        self.take_in(said.collect(), cx);
    }

    /// Takes `summaries` in as the host's list of its chats, which a connection starts with.
    pub(crate) fn hear_list(&mut self, summaries: Vec<ThreadSummary>, cx: &mut Context<Self>) {
        self.listed(Ok(summaries), cx);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use koyo_agent::protocol::model::{Status, ThreadSummary};
    use koyo_agent::protocol::requests::RequestKind;

    use super::{ChatMark, checkout_marks};
    use crate::chat::tests::summary as tests_summary;

    fn summary(status: Status, unread: bool) -> ThreadSummary {
        ThreadSummary {
            status,
            unread,
            ..tests_summary("0001", "/work".as_ref(), "A chat", true)
        }
    }

    #[test]
    fn a_chat_says_what_it_is_up_to_and_nothing_once_seen() {
        assert_eq!(
            ChatMark::of(&summary(Status::Working, false)),
            Some(ChatMark::Working)
        );
        assert_eq!(
            ChatMark::of(&summary(Status::Idle, true)),
            Some(ChatMark::Unread)
        );
        assert_eq!(ChatMark::of(&summary(Status::Idle, false)), None);
        assert_eq!(
            ChatMark::of(&summary(Status::Failed, true)),
            Some(ChatMark::Failed)
        );
        assert_eq!(ChatMark::of(&summary(Status::Failed, false)), None);
    }

    #[test]
    fn what_it_needs_comes_before_all_else() {
        let asking = ThreadSummary {
            needs: Some(RequestKind::Approval),
            ..summary(Status::Working, true)
        };
        assert_eq!(ChatMark::of(&asking), Some(ChatMark::NeedsYou));
        assert!(ChatMark::NeedsYou > ChatMark::Failed);
        assert!(ChatMark::Failed > ChatMark::Working);
        assert!(ChatMark::Working > ChatMark::Unread);
    }

    #[test]
    fn a_row_says_the_most_urgent_of_its_chats_and_nothing_of_a_closed_one_at_rest() {
        let working = summary(Status::Working, false);
        let failed = summary(Status::Failed, true);
        let closed = ThreadSummary {
            cwd: "/other".into(),
            tab_open: false,
            ..summary(Status::Idle, true)
        };
        let closed_at_it = ThreadSummary {
            cwd: "/busy".into(),
            tab_open: false,
            ..working.clone()
        };

        let marks = checkout_marks([&working, &failed, &closed, &closed_at_it].into_iter());

        assert_eq!(marks.get(Path::new("/work")), Some(&ChatMark::Failed));
        assert_eq!(marks.get(Path::new("/busy")), Some(&ChatMark::Working));
        assert_eq!(marks.get(Path::new("/other")), None);
    }
}
