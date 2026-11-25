//! What goes on in a chat, as a tab, a checkout, a project or a space says it of the chats in it
//! (`docs/activity.md`): the work, and what it has for the person, two things that are each said
//! whatever the other is. Both are put together one way, so nothing read off a row is worked out
//! anywhere but here.

use std::iter::Sum;
use std::ops::Add;

use gpui_kit::Hsla;
use koyo_agent::protocol::model::{Status, ThreadSummary};
use koyo_agent::work::Work;

use crate::theme::Palette;

/// What a chat has for the person, least urgent first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Attention {
    /// It finished, and they haven't looked since.
    Unread,
    /// It failed, and they haven't looked since.
    Failed,
    /// It waits for them.
    NeedsYou,
}

impl Attention {
    fn of(summary: &ThreadSummary) -> Option<Self> {
        if summary.needs.is_some() {
            return Some(Attention::NeedsYou);
        }
        let ended = if summary.status == Status::Failed {
            Attention::Failed
        } else {
            Attention::Unread
        };
        summary.unread.then_some(ended)
    }

    /// Its dot's colour on a rail tab, which says which it is.
    pub fn colour(self, p: Palette) -> Hsla {
        match self {
            Attention::NeedsYou => p.text,
            Attention::Failed => p.danger,
            Attention::Unread => p.muted,
        }
    }
}

/// What goes on in a chat, or in several put together: all their work, and the most urgent of what
/// they have for the person.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    pub work: Work,
    pub attention: Option<Attention>,
}

impl Activity {
    pub fn of(summary: &ThreadSummary) -> Self {
        Activity {
            work: summary.work(),
            attention: Attention::of(summary),
        }
    }

    /// This, with something that waits for the person too if `waits`, as a terminal does.
    pub fn with_waiting(self, waits: bool) -> Self {
        let waiting = waits.then_some(Attention::NeedsYou);
        Activity {
            attention: self.attention.max(waiting),
            ..self
        }
    }

    /// What's left to say of a chat a person closed: it has nothing new to tell until it's opened,
    /// but its work goes on, and it may wait for them.
    pub(super) fn of_a_closed_chat(self) -> Self {
        Activity {
            attention: (self.attention).filter(|attention| *attention == Attention::NeedsYou),
            ..self
        }
    }
}

impl Add for Activity {
    type Output = Activity;

    fn add(self, other: Activity) -> Activity {
        Activity {
            work: self.work + other.work,
            attention: self.attention.max(other.attention),
        }
    }
}

impl Sum for Activity {
    fn sum<I: Iterator<Item = Activity>>(activities: I) -> Activity {
        activities.fold(Activity::default(), Add::add)
    }
}

#[cfg(test)]
mod tests {
    use koyo_agent::protocol::model::{Beside, Status, ThreadSummary};
    use koyo_agent::protocol::requests::RequestKind;
    use koyo_agent::work::Work;

    use super::{Activity, Attention};
    use crate::chat::tests::summary as tests_summary;

    fn chat(status: Status, needs: Option<RequestKind>, unread: bool) -> ThreadSummary {
        ThreadSummary {
            status,
            needs,
            unread,
            ..tests_summary("0001", "/work".as_ref(), "A chat", true)
        }
    }

    fn says(chat: &ThreadSummary) -> (bool, Option<Attention>) {
        let activity = Activity::of(chat);
        (activity.work.turn, activity.attention)
    }

    #[test]
    fn a_chat_says_its_turn_and_what_it_has_for_the_person() {
        use Attention::{Failed, NeedsYou, Unread};
        use Status::{Idle, Working};

        assert_eq!(says(&chat(Idle, None, false)), (false, None));
        assert_eq!(says(&chat(Working, None, false)), (true, None));
        assert_eq!(says(&chat(Idle, None, true)), (false, Some(Unread)));
        assert_eq!(
            says(&chat(Working, None, true)),
            (true, Some(Unread)),
            "neither hides the other"
        );
        assert_eq!(says(&chat(Status::Failed, None, true)), (false, Some(Failed)));
        assert_eq!(says(&chat(Status::Failed, None, false)), (false, None), "seen");
        assert_eq!(
            says(&chat(Working, Some(RequestKind::Approval), true)),
            (false, Some(NeedsYou)),
            "a turn that waits for the person isn't at work"
        );
    }

    #[test]
    fn what_runs_beside_a_turn_is_work_whatever_the_chat_asks() {
        let beside = Beside {
            subagents: 3,
            tasks: 1,
        };
        let asking = ThreadSummary {
            beside,
            ..chat(Status::Working, Some(RequestKind::Question), false)
        };
        let activity = Activity::of(&asking);
        assert_eq!(activity.work, Work { turn: false, beside });
        assert_eq!(activity.attention, Some(Attention::NeedsYou));
    }

    #[test]
    fn together_chats_say_all_their_work_and_the_most_urgent_they_have_for_the_person() {
        let working = Activity::of(&chat(Status::Working, None, false));
        let failed = Activity::of(&chat(Status::Failed, None, true));
        let unread = Activity::of(&chat(Status::Idle, None, true));
        let together: Activity = [unread, working, failed].into_iter().sum();
        assert!(together.work.turn);
        assert_eq!(together.attention, Some(Attention::Failed));
        assert_eq!(
            together.with_waiting(true).attention,
            Some(Attention::NeedsYou)
        );
        assert_eq!(together.with_waiting(false), together);
    }
}
