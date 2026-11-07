//! What comes and goes over time from wherever it had got to, eased so it neither pops in nor
//! stops short: the glow round a page an agent holds, the line round a working chat's composer.
//! Everything here is told the time, so tests tell it too.

use std::time::{Duration, Instant};

/// 0 to 1 over `span` from `since`, at `now`.
pub fn progress(since: Instant, span: Duration, now: Instant) -> f32 {
    if span.is_zero() {
        return 1.;
    }
    (now.saturating_duration_since(since).as_secs_f32() / span.as_secs_f32()).clamp(0., 1.)
}

/// Eases in and out, its speed nothing at either end.
pub fn smooth(t: f32) -> f32 {
    t * t * (3. - 2. * t)
}

/// How much of something shows, from 0 to 1, as it comes over `fade_in` and goes over
/// `fade_out`. Turned midway, it goes from where it had got to rather than jumping.
#[derive(Clone, Debug)]
pub struct Fade {
    /// How long it takes to come, and to go.
    coming: Duration,
    going: Duration,
    on: bool,
    /// When it last turned, and how much showed then.
    changed: Option<(Instant, f32)>,
}

impl Fade {
    /// One that isn't showing, and comes and goes as long as these say.
    pub const fn new(fade_in: Duration, fade_out: Duration) -> Fade {
        Fade {
            coming: fade_in,
            going: fade_out,
            on: false,
            changed: None,
        }
    }

    /// Whether it's showing, or coming to.
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// How long it takes to get where it's going.
    fn span(&self) -> Duration {
        if self.on { self.coming } else { self.going }
    }

    /// How much of it shows at `now`, from 0 to 1.
    pub fn level(&self, now: Instant) -> f32 {
        let Some((since, from)) = self.changed else {
            return 0.;
        };
        let to = if self.on { 1. } else { 0. };
        from + (to - from) * smooth(progress(since, self.span(), now))
    }

    /// It shows from `now`, or no longer does. Answers whether that's a change.
    pub fn set(&mut self, on: bool, now: Instant) -> bool {
        let changes = on != self.on;
        if changes {
            let from = self.level(now);
            (self.on, self.changed) = (on, Some((now, from)));
        }
        changes
    }

    /// Whether it's coming or going at `now`.
    pub fn changing(&self, now: Instant) -> bool {
        (self.changed).is_some_and(|(since, _)| now < since + self.span())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_comes_and_goes_as_long_as_it_says() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut fade = Fade::new(Duration::from_millis(700), Duration::from_millis(1100));
        assert!(!fade.changing(start), "nothing to fade yet");
        fade.set(true, start);
        assert!((fade.level(at(350)) - 0.5).abs() < 0.01);
        assert!(!fade.changing(at(700)));
        fade.set(false, at(1000));
        assert!(fade.changing(at(2000)), "it takes longer to go");
        assert!(!fade.changing(at(2100)));
        assert!(fade.level(at(2100)).abs() < 1e-6);
    }
}
