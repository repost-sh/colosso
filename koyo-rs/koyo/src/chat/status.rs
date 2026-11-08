//! What a tab and a checkout's row say of a chat (`docs/chat.md` 5.3): a spinner while it works, a
//! dot in the text's colour when it needs you, red if it failed and a muted dot if it finished,
//! until a person has seen it. The spinner is a terminal's: braille dots going round, a step every
//! 80 ms, drawn as dots rather than a glyph so it looks the same in any font. It moves only while
//! the app's in front, and as a view of its own: a turn can take a long time, and each step draws
//! only it, not the shell.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{
    Animation, AnimationExt as _, App, AppContext as _, Context, Div, Entity, Global, IntoElement,
    ParentElement as _, Render, Styled as _, Window, div, px,
};
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::methods::{ThreadParams, ThreadsRead};
use koyo_agent::protocol::model::{Status, ThreadSummary};

use super::{Chats, ChatsEvent, logged};
use crate::model::Tab;
use crate::pane::dot;
use crate::shell::Shell;
use crate::theme::{self, Palette};

/// The spinner's square, the same as a dot's room.
const SPINNER: f32 = 10.;
/// The spinner's steps, as the braille cells terminal spinners use (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`): each a
/// cell's dots, 1 to 3 down the left and 4 to 6 down the right, as Unicode numbers them.
const STEPS: [u8; 10] = [0x0B, 0x19, 0x39, 0x38, 0x3C, 0x34, 0x26, 0x27, 0x07, 0x0F];
/// How long each step shows, as terminal spinners take.
const STEP: Duration = Duration::from_millis(80);
/// How often the spinner's drawn: often enough that no step is missed or late.
const SPINNER_FPS: f32 = 25.;
/// A dot of the cell, and the room between them.
const DOT: f32 = 2.5;
const DOT_GAP: f32 = 1.5;

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
        let p = theme::palette(cx);
        let holder = div().size(px(SPINNER)).flex_shrink_0();
        if is_frontmost(cx) {
            let round = STEP * STEPS.len() as u32;
            let going = Animation::new(round).repeat().with_max_fps(SPINNER_FPS);
            holder
                .with_animation("chat-working", going, move |holder, delta| {
                    let step = ((delta * STEPS.len() as f32) as usize).min(STEPS.len() - 1);
                    holder.child(cell(STEPS[step], p))
                })
                .into_any_element()
        } else {
            holder.child(cell(STEPS[0], p)).into_any_element()
        }
    }
}

/// A braille cell of the spinner, the dots of `lit` bright and the rest faint.
fn cell(lit: u8, p: Palette) -> Div {
    let dot = |bit: u8| {
        let on = lit & bit != 0;
        div().size(px(DOT)).rounded_full().bg(if on {
            p.text.opacity(0.85)
        } else {
            p.muted.opacity(0.18)
        })
    };
    let column = |bits: [u8; 3]| {
        div()
            .flex()
            .flex_col()
            .gap(px(DOT_GAP))
            .children(bits.map(dot))
    };
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(DOT_GAP))
        .child(column([0x01, 0x02, 0x04]))
        .child(column([0x08, 0x10, 0x20]))
}

/// The spinners by place, each of which is a view drawn in one spot, for as long as a chat works.
#[derive(Default)]
struct Spinners(HashMap<String, Entity<Spin>>);

impl Global for Spinners {}

/// The spinner `place` shows while a chat works there: a tab, a checkout's row, a project's
/// tile.
pub fn spinner(place: String, cx: &mut App) -> Div {
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
        let mark = self.tab_chat_marks(tab, cx).max()?;
        Some(mark.shown(format!("tab-{}", tab.first().id), p, cx))
    }

    /// The marks of `tab`'s chats, those with something to show.
    fn tab_chat_marks(&self, tab: &Tab, cx: &App) -> impl Iterator<Item = ChatMark> {
        let chats = self.chat_tabs.chats.read(cx);
        (tab.panes().into_iter())
            .filter_map(move |pane| chats.summary(pane.chat.as_ref()?).and_then(ChatMark::of))
    }

    /// The mark on the row of the checkout at `worktree`.
    pub fn checkout_chat_mark(&self, worktree: &Path, p: Palette, cx: &mut App) -> Option<Div> {
        let mark = *self.chat_tabs.chats.read(cx).checkout_marks.get(worktree)?;
        Some(mark.shown(format!("row-{}", worktree.display()), p, cx))
    }

    /// Whether a tab has something for the person (`docs/arc-sidebar.md` 5): a terminal waiting
    /// for them, or a chat that's unread, failed or needs them.
    pub fn tab_wants_attention(&self, tab: &Tab, cx: &App) -> bool {
        self.tab_chat_marks(tab, cx)
            .any(|mark| mark != ChatMark::Working)
            || self.tab_terminal_waits(tab)
    }

    /// How the checkout at `worktree` is doing, as a project's tile sums it up.
    pub fn checkout_activity(&self, worktree: &Path, cx: &App) -> Activity {
        if self.terminals_needing_input(worktree) > 0 {
            return Activity::Attention;
        }
        match self.chat_tabs.chats.read(cx).checkout_marks.get(worktree) {
            None => Activity::Quiet,
            Some(ChatMark::Working) => Activity::Working,
            Some(_) => Activity::Attention,
        }
    }

    /// How many chats are at work in the checkout at `worktree`.
    pub fn chats_working(&self, worktree: &Path, cx: &App) -> usize {
        (self
            .chat_tabs
            .chats
            .read(cx)
            .latest_first(Some(worktree))
            .into_iter())
        .filter(|summary| summary.status == Status::Working)
        .count()
    }

    /// Asks the chats `tab` shows to stop whatever they're running.
    pub fn stop_chats_in(&self, tab: &Tab, cx: &mut App) {
        let threads: Vec<ThreadId> = (tab.panes().into_iter())
            .filter_map(|pane| pane.chat.clone())
            .collect();
        if threads.is_empty() {
            return;
        }
        self.chat_tabs.chats.update(cx, |chats, cx| {
            for thread in &threads {
                let working = chats.summary(thread).map(|summary| summary.status);
                if working == Some(Status::Working) {
                    chats.stop(thread, cx);
                }
            }
        });
    }
}

/// What's going on in a project or a checkout, the most urgent of it: what a tile's corner shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Activity {
    Quiet,
    /// A chat is at work: a spinner.
    Working,
    /// Something waits for the person: a dot.
    Attention,
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
