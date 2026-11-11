//! When the Mac's login's usage is read (`docs/claude-accounts.md` 4), Orca 1.4.256's cadence:
//! a second after Koyo starts, every 15 min while Koyo is in front, and on coming to the front
//! when the reading is 5 min old or more. Between readings, each chat's rate-limit headers keep
//! it current for free. A failed reading backs off as claude-swap's usage store does
//! (`usage_store::failure_backoff_s`), whatever the cadence asks.

/// The first reading after Koyo starts.
pub const FIRST: f64 = 1.;
/// While Koyo is in front.
pub const EVERY: f64 = 15. * 60.;
/// How old the reading is before coming to the front reads it again.
pub const ON_FOCUS_AFTER: f64 = 5. * 60.;

#[derive(Debug, Default)]
pub struct Cadence {
    /// When the login was last asked for.
    asked: Option<f64>,
    /// Whether Koyo was in front at the last look.
    focused: bool,
}

impl Cadence {
    /// Whether the login is due for reading at `now`, with Koyo `focused` or not and its reading
    /// `measured` then; due counts as asked.
    pub fn due(&mut self, now: f64, focused: bool, measured: Option<f64>) -> bool {
        let came_to_front = focused && !self.focused;
        self.focused = focused;
        let due = self.asked.is_none_or(|asked| {
            let stale = measured.is_none_or(|at| now - at >= ON_FOCUS_AFTER);
            (focused && now - asked >= EVERY) || (came_to_front && stale)
        });
        if due {
            self.asked = Some(now);
        }
        due
    }

    /// How long until the next look at `now`: coming to the front looks of itself.
    pub fn next_look(&self, now: f64) -> f64 {
        self.asked
            .map_or(FIRST, |asked| (asked + EVERY - now).max(FIRST))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: f64 = 1_800_000_000.;

    #[test]
    fn the_first_look_reads_and_comes_a_second_after_koyo_starts() {
        let mut cadence = Cadence::default();
        assert!((cadence.next_look(START) - FIRST).abs() < 1e-9);
        assert!(cadence.due(START + 1., false, None));
        assert!(!cadence.due(START + 2., false, None));
    }

    #[test]
    fn every_fifteen_minutes_while_koyo_is_in_front_only() {
        let mut cadence = Cadence::default();
        cadence.due(START, true, None);
        let read = Some(START);
        assert!((cadence.next_look(START + 60.) - (EVERY - 60.)).abs() < 1e-9);
        assert!(!cadence.due(START + EVERY - 1., true, read));
        assert!(cadence.due(START + EVERY, true, read));
        // Behind another app, the quarter hour passes unread.
        assert!(!cadence.due(START + 2. * EVERY, false, read));
    }

    #[test]
    fn coming_to_the_front_reads_a_reading_five_minutes_old() {
        let mut cadence = Cadence::default();
        cadence.due(START, false, None);
        let read = Some(START);
        // Four minutes on: fresh enough.
        assert!(!cadence.due(START + 240., true, read));
        cadence.due(START + 250., false, read);
        // Five minutes on, coming back: read again.
        assert!(cadence.due(START + 300., true, read));
        // Staying in front isn't coming to it.
        assert!(!cadence.due(START + 400., true, read));
    }
}
