//! When the accounts' usage is read (`docs/claude-accounts.md` 4), Orca 1.4.256's cadence:
//!
//! - the active account a second after Koyo starts, every 15 min while Koyo is in front, on
//!   coming to the front when its reading is 5 min old or more, and on every switch (which the
//!   switch asks for itself);
//! - the other accounts one at a time, at most one request a minute, while the panel is open.
//!
//! Between readings, each chat's rate-limit headers keep the active account current for free.
//! Whether a request is sent in the end is the usage store's call: its freshness floor, backoff
//! and leases hold for every reader (claude-swap's `reserve`).

/// The first reading after Koyo starts.
pub const FIRST: f64 = 1.;
/// While Koyo is in front.
pub const EVERY: f64 = 15. * 60.;
/// How old the reading is before coming to the front reads it again.
pub const ON_FOCUS_AFTER: f64 = 5. * 60.;
/// The least time between two readings of other accounts while the panel is open.
pub const OTHERS_EVERY: f64 = 60.;

/// What a look found due.
#[derive(Clone, Copy, Debug)]
pub struct Due {
    pub active: bool,
    /// One other account, the one read least recently.
    pub other: bool,
}

#[derive(Debug, Default)]
pub struct Cadence {
    /// When the active account was last asked for, by this cadence or a switch.
    asked: Option<f64>,
    /// When another account was last asked for.
    other_asked: Option<f64>,
    /// Whether Koyo was in front at the last look.
    focused: bool,
}

impl Cadence {
    /// The active account was just read for another reason: a switch.
    pub fn active_asked(&mut self, now: f64) {
        self.asked = Some(now);
    }

    /// What's due at `now`, with Koyo `focused` or not, the active account's reading
    /// `measured` then, and the panel open or not; what's due counts as asked.
    pub fn due(&mut self, now: f64, focused: bool, measured: Option<f64>, panel_open: bool) -> Due {
        let came_to_front = focused && !self.focused;
        self.focused = focused;
        let active = self.asked.is_none_or(|asked| {
            let stale = measured.is_none_or(|at| now - at >= ON_FOCUS_AFTER);
            (focused && now - asked >= EVERY) || (came_to_front && stale)
        });
        if active {
            self.asked = Some(now);
        }
        let other = panel_open
            && self
                .other_asked
                .is_none_or(|asked| now - asked >= OTHERS_EVERY);
        if other {
            self.other_asked = Some(now);
        }
        Due { active, other }
    }

    /// How long until the next look at `now`: coming to the front looks of itself, and an open
    /// panel looks every minute.
    pub fn next_look(&self, now: f64, panel_open: bool) -> f64 {
        let active = self.asked.map_or(FIRST, |asked| asked + EVERY - now);
        let other = panel_open.then(|| {
            self.other_asked
                .map_or(FIRST, |asked| asked + OTHERS_EVERY - now)
        });
        other.map_or(active, |other| active.min(other)).max(FIRST)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: f64 = 1_800_000_000.;

    #[test]
    fn the_first_look_reads_and_comes_a_second_after_koyo_starts() {
        let mut cadence = Cadence::default();
        assert!((cadence.next_look(START, false) - FIRST).abs() < 1e-9);
        assert!(cadence.due(START + 1., false, None, false).active);
        assert!(!cadence.due(START + 2., false, None, false).active);
    }

    #[test]
    fn every_fifteen_minutes_while_koyo_is_in_front_only() {
        let mut cadence = Cadence::default();
        cadence.due(START, true, None, false);
        let read = Some(START);
        assert!((cadence.next_look(START + 60., false) - (EVERY - 60.)).abs() < 1e-9);
        assert!(!cadence.due(START + EVERY - 1., true, read, false).active);
        assert!(cadence.due(START + EVERY, true, read, false).active);
        // Behind another app, the quarter hour passes unread.
        assert!(!cadence.due(START + 2. * EVERY, false, read, false).active);
    }

    #[test]
    fn coming_to_the_front_reads_a_reading_five_minutes_old() {
        let mut cadence = Cadence::default();
        cadence.due(START, false, None, false);
        let read = Some(START);
        // Four minutes on: fresh enough.
        assert!(!cadence.due(START + 240., true, read, false).active);
        cadence.due(START + 250., false, read, false);
        // Five minutes on, coming back: read again.
        assert!(cadence.due(START + 300., true, read, false).active);
        // Staying in front isn't coming to it.
        assert!(!cadence.due(START + 400., true, read, false).active);
    }

    #[test]
    fn other_accounts_one_a_minute_while_the_panel_is_open() {
        let mut cadence = Cadence::default();
        cadence.due(START, true, None, false);
        let read = Some(START);
        assert!(cadence.due(START + 10., true, read, true).other);
        assert!(!cadence.due(START + 69., true, read, true).other);
        assert!((cadence.next_look(START + 69., true) - 1.).abs() < 1e-9);
        assert!(cadence.due(START + 70., true, read, true).other);
        assert!(!cadence.due(START + 200., true, read, false).other);
    }

    #[test]
    fn a_switch_counts_as_reading_the_active_account() {
        let mut cadence = Cadence::default();
        cadence.due(START, true, None, false);
        cadence.active_asked(START + 600.);
        let read = Some(START + 600.);
        assert!(!cadence.due(START + EVERY, true, read, false).active);
        assert!(cadence.due(START + 600. + EVERY, true, read, false).active);
    }
}
