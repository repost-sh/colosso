//! What goes on in each place that shows chats (`docs/activity.md`): a tab, a checkout, a project,
//! a space, put together from the chats' summaries (`activity.rs`) and said in signs (`sign.rs`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui_kit::{App, Context, SharedString};
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::methods::{ThreadParams, ThreadsRead};
use koyo_agent::protocol::model::ThreadSummary;

use super::activity::Activity;
use super::spinner;
use super::{Chats, ChatsEvent, logged};
use crate::model::{Project, Tab};
use crate::shell::Shell;

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
        self.checkouts = checkouts(self.summaries.values());
        if !self.summaries.values().any(|summary| summary.work().any()) {
            spinner::forget(cx);
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

/// What a chat's tab and its checkout's row draw of it, which the shell hears of as it changes.
#[derive(PartialEq, Eq)]
pub(super) struct Shown<'a> {
    title: &'a str,
    cwd: &'a str,
    activity: Activity,
    tab_open: bool,
    said: Option<&'a str>,
}

impl<'a> Shown<'a> {
    pub(super) fn of(summary: &'a ThreadSummary) -> Self {
        Shown {
            title: &summary.title,
            cwd: &summary.cwd,
            activity: Activity::of(summary),
            tab_open: summary.tab_open,
            said: summary.said.as_deref(),
        }
    }
}

/// What goes on in a checkout's chats.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct CheckoutActivity {
    /// In them all.
    all: Activity,
    /// In those in no tab, which no tab's row says.
    tabless: Activity,
}

/// What goes on in each checkout's chats: those with tabs, and what's left to say of those without.
fn checkouts<'a>(
    summaries: impl Iterator<Item = &'a ThreadSummary>,
) -> HashMap<PathBuf, CheckoutActivity> {
    let mut checkouts: HashMap<PathBuf, CheckoutActivity> = HashMap::new();
    for summary in summaries {
        let activity = Activity::of(summary);
        let activity = if summary.tab_open {
            activity
        } else {
            activity.of_a_closed_chat()
        };
        if activity == Activity::default() {
            continue;
        }
        let held = checkouts.entry(PathBuf::from(&summary.cwd)).or_default();
        held.all = held.all + activity;
        if !summary.tab_open {
            held.tabless = held.tabless + activity;
        }
    }
    checkouts
}

impl Shell {
    /// What goes on in `tab`, its chats and whether a terminal of its waits for the person.
    pub fn tab_activity(&self, tab: &Tab, cx: &App) -> Activity {
        self.tab_chats_activity(tab, cx).with_waiting(self.tab_terminal_waits(tab))
    }

    /// What goes on in `tab`'s chats, as the host last said.
    pub fn tab_chats_activity(&self, tab: &Tab, cx: &App) -> Activity {
        let chats = self.chat_tabs.chats.read(cx);
        (tab.panes().into_iter())
            .filter_map(|pane| chats.summary(pane.chat()?))
            .map(Activity::of)
            .sum()
    }

    /// What a single pane's `tab` says under its title (`docs/sidebar.md` 5): the start of what
    /// was said last in its chat, as the host sums it up.
    pub fn tab_said(&self, tab: &Tab, cx: &App) -> Option<SharedString> {
        let Tab::Single(pane) = tab else {
            return None;
        };
        let summary = self.chat_tabs.chats.read(cx).summary(pane.chat()?)?;
        summary.said.clone().map(SharedString::from)
    }

    /// What goes on in the checkout at `worktree`, as its row says, and how many of its terminals
    /// wait for the person, as its row counts them. With its tabs `listed` under it, what they
    /// show is theirs to say, and the row says what they don't: a chat in no tab.
    pub fn checkout_status(&self, worktree: &Path, listed: bool, cx: &App) -> (Activity, usize) {
        let chats = self.chat_tabs.chats.read(cx).checkouts.get(worktree);
        let chats = chats.copied().unwrap_or_default();
        if listed {
            return (chats.tabless, 0);
        }
        let waiting = self.terminals_needing_input(worktree);
        (chats.all.with_waiting(waiting > 0), waiting)
    }

    /// What goes on in the checkout at `worktree`, all of it.
    pub fn checkout_activity(&self, worktree: &Path, cx: &App) -> Activity {
        self.checkout_status(worktree, false, cx).0
    }

    /// What goes on in `project`, in all its checkouts: what its row says, and, with its space's
    /// other projects, its space's mark while the space isn't shown.
    pub fn project_activity(&self, project: &Project, cx: &App) -> Activity {
        (project.branches.iter())
            .map(|unit| self.checkout_activity(&unit.worktree.path, cx))
            .sum()
    }
}

#[cfg(any(test, feature = "visual-check"))]
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
    #[cfg(test)]
    pub(crate) fn hear_list(&mut self, summaries: Vec<ThreadSummary>, cx: &mut Context<Self>) {
        self.listed(Ok(summaries), cx);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use koyo_agent::protocol::model::{Beside, Status, ThreadSummary};
    use koyo_agent::work::Work;

    use super::{CheckoutActivity, checkouts};
    use crate::chat::activity::Attention;
    use crate::chat::tests::summary as tests_summary;

    fn chat(cwd: &str, status: Status, unread: bool, tab_open: bool) -> ThreadSummary {
        ThreadSummary {
            status,
            unread,
            ..tests_summary("0001", cwd.as_ref(), "A chat", tab_open)
        }
    }

    #[test]
    fn a_checkout_says_all_its_chats_and_a_closed_ones_work_alone() {
        let chats = [
            chat("/work", Status::Working, false, true),
            chat("/work", Status::Failed, true, true),
            chat("/closed", Status::Idle, true, false),
            chat("/busy", Status::Working, true, false),
            ThreadSummary {
                beside: Beside {
                    subagents: 0,
                    tasks: 1,
                },
                ..chat("/watched", Status::Idle, true, false)
            },
        ];
        let checkouts = checkouts(chats.iter());
        let of = |checkout: &str| checkouts.get(Path::new(checkout)).copied();
        let turn = Work {
            turn: true,
            ..Work::default()
        };
        let work = of("/work").expect("a working checkout");
        assert_eq!(work.all.work, turn);
        assert_eq!(work.all.attention, Some(Attention::Failed));
        assert_eq!(work.tabless, CheckoutActivity::default().tabless);
        let busy = of("/busy").expect("a closed chat at work");
        assert_eq!((busy.all.work, busy.all.attention), (turn, None));
        assert_eq!(busy.tabless, busy.all);
        let watched = of("/watched").expect("a closed chat with a Monitor running");
        assert_eq!(watched.tabless.work.beside.tasks, 1);
        assert_eq!(of("/closed"), None);
    }
}
