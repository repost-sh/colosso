//! One log as the activity panel holds it: its newest rows, numbered for good so the rows
//! Clear hid and the rows opened stay the same as old rows go, and one read of it out at a
//! time.

use colosso_term_control::LogRead;
use serde::de::DeserializeOwned;

/// The rows a tab keeps, the newest.
const KEPT_ROWS: usize = 2000;

/// Where a read of a log starts: its newest lines the first time, then where the last ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadFrom {
    Tail,
    Offset(u64),
}

pub struct Log<T> {
    rows: Vec<T>,
    /// How many rows went before the first kept: a row's number is this and its index.
    dropped: usize,
    /// Where reading goes on from; `None` until the first read.
    next: Option<u64>,
    reading: bool,
    /// The log grew while a read was out: read again once it's back.
    again: bool,
}

impl<T> Default for Log<T> {
    fn default() -> Self {
        Log {
            rows: Vec::new(),
            dropped: 0,
            next: None,
            reading: false,
            again: false,
        }
    }
}

impl<T: DeserializeOwned> Log<T> {
    /// Where a read starts, unless one is out already, which then reads again once back.
    pub fn start(&mut self) -> Option<ReadFrom> {
        if self.reading {
            self.again = true;
            return None;
        }
        self.reading = true;
        Some(self.next.map_or(ReadFrom::Tail, ReadFrom::Offset))
    }

    /// Takes in what a read found, `None` when it failed: whether the rows changed, and
    /// whether to read again.
    pub fn finish(&mut self, read: Option<LogRead>) -> (bool, bool) {
        self.reading = false;
        let again = std::mem::take(&mut self.again);
        let Some(read) = read else {
            return (false, again);
        };
        let rotated = read.rotated && !self.rows.is_empty();
        if rotated {
            self.dropped += self.rows.len();
            self.rows.clear();
        }
        self.next = Some(read.next);
        let before = self.rows.len();
        let rows = (read.lines.into_iter()).filter_map(|line| serde_json::from_str(&line).ok());
        self.rows.extend(rows);
        let came = self.rows.len() > before;
        let over = self.rows.len().saturating_sub(KEPT_ROWS);
        self.rows.drain(..over);
        self.dropped += over;
        (rotated || came, again)
    }
}

impl<T> Log<T> {
    pub fn rows(&self) -> &[T] {
        &self.rows
    }

    /// The rows numbered `from` on, with their numbers.
    pub fn numbered(&self, from: usize) -> impl Iterator<Item = (usize, &T)> {
        let dropped = self.dropped;
        (self.rows.iter().enumerate())
            .skip(from.saturating_sub(dropped))
            .map(move |(index, row)| (dropped + index, row))
    }

    /// The number the next row will have.
    pub fn end(&self) -> usize {
        self.dropped + self.rows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(lines: std::ops::Range<u32>, next: u64, rotated: bool) -> LogRead {
        LogRead {
            lines: lines.map(|n| n.to_string()).collect(),
            next,
            rotated,
        }
    }

    #[test]
    fn one_read_at_a_time_and_another_when_the_log_grew_meanwhile() {
        let mut log = Log::<u32>::default();
        assert_eq!(log.start(), Some(ReadFrom::Tail));
        assert_eq!(log.start(), None, "one read out at a time");
        assert_eq!(log.finish(Some(read(0..3, 6, false))), (true, true));
        assert_eq!(log.start(), Some(ReadFrom::Offset(6)));
        assert_eq!(log.finish(Some(read(0..0, 6, false))), (false, false));
        assert_eq!(log.start(), Some(ReadFrom::Offset(6)));
        assert_eq!(
            log.finish(None),
            (false, false),
            "a failed read changes nothing"
        );
    }

    #[test]
    fn rows_keep_their_numbers_as_old_ones_go() {
        let mut log = Log::<u32>::default();
        log.start();
        log.finish(Some(read(0..2000, 1, false)));
        log.start();
        log.finish(Some(read(2000..2010, 2, false)));
        assert_eq!(log.rows().len(), KEPT_ROWS);
        assert_eq!(log.end(), 2010);
        let first = log.numbered(2005).next();
        assert_eq!(first, Some((2005, &2005)));
        log.start();
        log.finish(Some(read(7..8, 3, true)));
        assert_eq!(
            log.numbered(0).collect::<Vec<_>>(),
            [(2010, &7)],
            "rotated: a fresh start"
        );
    }
}
