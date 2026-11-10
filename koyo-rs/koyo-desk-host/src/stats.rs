//! What the host says of its frames when asked to (`--stats`): how many it sent the app, of
//! how many macOS delivered, every few seconds. For measuring, not for the person.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use koyo_desk_wire::link::LogLevel;

use crate::host::Host;

const EVERY: Duration = Duration::from_secs(5);

impl Host {
    pub fn say_frames(&mut self) {
        let (Some((since, sent_before, came_before)), Some(streams)) = (self.stats, &self.streams)
        else {
            return;
        };
        if since.elapsed() < EVERY {
            return;
        }
        let sent = streams.frames.load(Ordering::Relaxed);
        let came = streams.delivered.load(Ordering::Relaxed);
        self.stats = Some((Instant::now(), sent, came));
        if sent == sent_before {
            return;
        }
        let seconds = since.elapsed().as_secs_f64();
        let text = format!(
            "{} frames in {seconds:.1} s ({:.1} a second) of {} delivered ({:.1} a second) over {} windows",
            sent - sent_before,
            (sent - sent_before) as f64 / seconds,
            came - came_before,
            (came - came_before) as f64 / seconds,
            self.windows.len()
        );
        self.log(LogLevel::Info, text);
    }
}
