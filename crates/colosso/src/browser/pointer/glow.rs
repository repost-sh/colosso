//! The glow round a page an agent holds, as it comes and goes.

use std::time::{Duration, Instant};

use super::{progress, smooth};

/// How long the glow takes to come and go.
const GLOW_FADE: Duration = Duration::from_millis(450);

/// The glow round a page an agent holds: how much of it shows, as it comes and goes.
#[derive(Debug, Default)]
pub struct Glow {
    on: bool,
    /// When it last changed, and how much showed then.
    changed: Option<(Instant, f32)>,
}

impl Glow {
    /// How much of it shows at `now`, from 0 to 1.
    pub fn level(&self, now: Instant) -> f32 {
        let Some((since, from)) = self.changed else {
            return 0.;
        };
        let to = if self.on { 1. } else { 0. };
        from + (to - from) * smooth(progress(since, GLOW_FADE, now))
    }

    /// An agent holds the page from `now`, or no longer does.
    pub fn set(&mut self, on: bool, now: Instant) {
        if on != self.on {
            let from = self.level(now);
            (self.on, self.changed) = (on, Some((now, from)));
        }
    }

    /// Whether it's coming or going at `now`.
    pub fn changing(&self, now: Instant) -> bool {
        self.changed
            .is_some_and(|(since, _)| now < since + GLOW_FADE)
    }
}
