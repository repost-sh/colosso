//! What an agent is at for a chat (`docs/activity.md`), read in one way off what lists know of the
//! chat and off the chat itself, and put together for chats shown as one: a tab's, a checkout's, a
//! project's, a space's.

use std::iter::Sum;
use std::ops::Add;

use crate::protocol::model::{Beside, Status, ThreadSummary};
use crate::replica::Replica;

/// What's at work for a chat, or for several put together.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// A turn runs, and isn't waiting on a person.
    pub turn: bool,
    pub beside: Beside,
}

/// How far from done work is, the least first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pace {
    /// No agent's at it, only what one left running in the background and goes on from.
    Background,
    /// An agent's at it: a turn runs, or a subagent.
    Agent,
}

impl Work {
    /// Whether anything's at work.
    pub fn any(self) -> bool {
        self.pace().is_some()
    }

    /// How it goes, or none for nothing at work.
    pub fn pace(self) -> Option<Pace> {
        if self.turn || self.beside.subagents > 0 {
            Some(Pace::Agent)
        } else {
            (self.beside.tasks > 0).then_some(Pace::Background)
        }
    }
}

impl Add for Work {
    type Output = Work;

    fn add(self, other: Work) -> Work {
        Work {
            turn: self.turn || other.turn,
            beside: Beside {
                subagents: self.beside.subagents + other.beside.subagents,
                tasks: self.beside.tasks + other.beside.tasks,
            },
        }
    }
}

impl Sum for Work {
    fn sum<I: Iterator<Item = Work>>(work: I) -> Work {
        work.fold(Work::default(), Add::add)
    }
}

impl ThreadSummary {
    /// What's at work for the chat.
    pub fn work(&self) -> Work {
        Work {
            turn: self.status == Status::Working && self.needs.is_none(),
            beside: self.beside,
        }
    }
}

impl Replica {
    /// What's at work for the chat.
    pub fn work(&self) -> Work {
        Work {
            turn: self.live().is_some() && self.asking().is_none(),
            beside: self.beside,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Beside, Pace, Work};

    fn work(turn: bool, subagents: u64, tasks: u64) -> Work {
        Work {
            turn,
            beside: Beside { subagents, tasks },
        }
    }

    #[test]
    fn an_agent_at_it_comes_before_what_runs_in_the_background() {
        assert_eq!(work(false, 0, 0).pace(), None);
        assert_eq!(work(false, 0, 2).pace(), Some(Pace::Background));
        assert_eq!(work(false, 1, 2).pace(), Some(Pace::Agent));
        assert_eq!(work(true, 0, 2).pace(), Some(Pace::Agent));
        assert!(Pace::Agent > Pace::Background);
    }

    #[test]
    fn put_together_nothing_of_either_is_lost() {
        let together: Work = [work(true, 0, 1), work(false, 2, 0), work(false, 1, 0)]
            .into_iter()
            .sum();
        assert_eq!(together, work(true, 3, 1));
    }
}
