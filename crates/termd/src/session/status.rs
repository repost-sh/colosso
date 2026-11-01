//! A terminal's status (`docs/terminals.md` 5.7), from what its program does: the shell's
//! OSC 133 marks when it makes them, else how the output flows. A notification, or a bell
//! that quiet follows, asks for the person until they type or the program gets busy again.

use std::time::{Duration, Instant};

use colosso_term_control::Status;
use colosso_term_engine::Mark;

/// Output this recent, with no marks to go by, is the program working.
const BUSY: Duration = Duration::from_millis(500);
/// No output for this long, with no marks to go by, is the program waiting.
const STILL: Duration = Duration::from_millis(1500);
/// The quiet after a bell that makes it ask for the person, and the output that ends an ask.
const ATTENTION: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
pub struct Tracker {
    /// The shell marks its prompts, so its marks say what it's doing.
    marked: bool,
    /// Between a command's `133;C` and its `133;D`.
    command: bool,
    /// The exit code the last command's `133;D` gave.
    pub last_exit: Option<i32>,
    last_output: Option<Instant>,
    /// When the output flowing now began.
    burst: Option<Instant>,
    /// A bell rang then, and asks for the person once quiet follows.
    bell: Option<Instant>,
    /// Why the program asks for the person.
    asking: Option<String>,
}

impl Tracker {
    /// Output came, with these marks in it.
    pub fn output(&mut self, now: Instant, marks: &[Mark]) {
        let flowing = (self.last_output).is_some_and(|last| now.duration_since(last) < BUSY);
        if !flowing {
            self.burst = Some(now);
        }
        self.last_output = Some(now);
        // A program that rings and goes on printing rang at its own work.
        self.bell = None;
        if (self.burst).is_some_and(|began| now.duration_since(began) >= ATTENTION) {
            self.asking = None;
        }
        for mark in marks {
            self.marked = true;
            match mark {
                Mark::PromptStart | Mark::InputStart => self.command = false,
                Mark::OutputStart => {
                    self.command = true;
                    self.asking = None;
                }
                Mark::CommandDone { exit } => {
                    self.command = false;
                    self.last_exit = *exit;
                }
            }
        }
    }

    /// The program rang the bell. At a shell's prompt it's the shell answering the person's
    /// own keys, a failed completion say, and asks for no one.
    pub fn bell(&mut self, now: Instant) {
        if !self.marked || self.command {
            self.bell = Some(now);
        }
    }

    /// The program asked for attention (OSC 9 or 777); OSC 9 has a body and no title.
    pub fn notification(&mut self, title: &str, body: &str) {
        let said = if title.is_empty() { body } else { title };
        self.asking = Some(if said.is_empty() {
            "notification".to_owned()
        } else {
            format!("notification: {said}")
        });
    }

    /// The person typed, pasted or clicked.
    pub fn input(&mut self) {
        self.asking = None;
        self.bell = None;
    }

    /// The status now and why, or `None` to keep the one it has.
    pub fn status(&mut self, now: Instant) -> Option<(Status, String)> {
        if let Some(rang) = self.bell
            && now.duration_since(rang) >= ATTENTION
        {
            self.bell = None;
            self.asking = Some("bell".to_owned());
        }
        if let Some(reason) = &self.asking {
            return Some((Status::NeedsInput, reason.clone()));
        }
        if self.marked {
            return Some(if self.command {
                (Status::Running, "command running".to_owned())
            } else {
                (Status::Idle, "prompt".to_owned())
            });
        }
        let quiet = now.duration_since(self.last_output?);
        if quiet < BUSY {
            Some((Status::Running, "output".to_owned()))
        } else if quiet >= STILL {
            Some((Status::Idle, "quiet".to_owned()))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    fn status(tracker: &mut Tracker, now: Instant) -> Option<(Status, String)> {
        tracker.status(now)
    }

    #[test]
    fn a_shells_marks_say_prompt_or_command_whatever_the_output_does() {
        let (start, mut tracker) = (Instant::now(), Tracker::default());
        tracker.output(start, &[Mark::PromptStart, Mark::InputStart]);
        assert_eq!(
            status(&mut tracker, at(start, 10)),
            Some((Status::Idle, "prompt".into()))
        );
        tracker.output(at(start, 100), &[Mark::OutputStart]);
        assert_eq!(
            status(&mut tracker, at(start, 5000)),
            Some((Status::Running, "command running".into()))
        );
        tracker.output(
            at(start, 6000),
            &[Mark::CommandDone { exit: Some(2) }, Mark::PromptStart],
        );
        assert_eq!(
            status(&mut tracker, at(start, 6010)),
            Some((Status::Idle, "prompt".into()))
        );
        assert_eq!(tracker.last_exit, Some(2));
    }

    #[test]
    fn without_marks_flowing_output_is_running_and_quiet_is_idle() {
        let (start, mut tracker) = (Instant::now(), Tracker::default());
        assert_eq!(
            status(&mut tracker, start),
            None,
            "nothing yet: keep \"started\""
        );
        tracker.output(start, &[]);
        assert_eq!(
            status(&mut tracker, at(start, 100)),
            Some((Status::Running, "output".into()))
        );
        assert_eq!(
            status(&mut tracker, at(start, 1000)),
            None,
            "between busy and still"
        );
        assert_eq!(
            status(&mut tracker, at(start, 1600)),
            Some((Status::Idle, "quiet".into()))
        );
    }

    #[test]
    fn a_bell_asks_once_quiet_follows_it_and_typing_answers() {
        let (start, mut tracker) = (Instant::now(), Tracker::default());
        tracker.output(start, &[]);
        tracker.bell(at(start, 10));
        assert_ne!(
            status(&mut tracker, at(start, 1000)).map(|(s, _)| s),
            Some(Status::NeedsInput)
        );
        assert_eq!(
            status(&mut tracker, at(start, 2100)),
            Some((Status::NeedsInput, "bell".into()))
        );
        tracker.input();
        assert_eq!(
            status(&mut tracker, at(start, 2200)),
            Some((Status::Idle, "quiet".into()))
        );
    }

    #[test]
    fn a_bell_followed_by_output_or_rung_at_a_prompt_asks_for_no_one() {
        let (start, mut tracker) = (Instant::now(), Tracker::default());
        tracker.output(start, &[]);
        tracker.bell(at(start, 10));
        tracker.output(at(start, 500), &[]);
        assert_ne!(
            status(&mut tracker, at(start, 3000)).map(|(s, _)| s),
            Some(Status::NeedsInput)
        );

        let mut shell = Tracker::default();
        shell.output(start, &[Mark::PromptStart, Mark::InputStart]);
        shell.bell(at(start, 10));
        assert_eq!(
            status(&mut shell, at(start, 3000)),
            Some((Status::Idle, "prompt".into()))
        );
    }

    #[test]
    fn a_notification_asks_until_typing_or_long_output() {
        let (start, mut tracker) = (Instant::now(), Tracker::default());
        tracker.output(start, &[Mark::OutputStart]);
        tracker.notification("Claude needs your permission", "");
        assert_eq!(
            status(&mut tracker, at(start, 10)),
            Some((
                Status::NeedsInput,
                "notification: Claude needs your permission".into()
            ))
        );
        // Output keeps coming for over two seconds: the program got on without the person.
        for ms in (100..=2400).step_by(100) {
            tracker.output(at(start, ms), &[]);
        }
        assert_eq!(
            status(&mut tracker, at(start, 2410)),
            Some((Status::Running, "command running".into()))
        );
    }
}
