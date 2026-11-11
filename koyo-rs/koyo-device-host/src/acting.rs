//! An act under way, a step at a time on the host's loop, and the look after it
//! (`docs/devices.md` 6): the picture sampled before, the act sent, then watched until it has
//! stood still for 300 ms (2 s at most), and the answer says how much changed. These input paths
//! fail silently, so an act isn't done until the picture has been looked at. Waits are the same
//! watch with the caller's times.

use std::time::{Duration, Instant};

use koyo_device_wire::{Act, Answer, Point, RefusalCode, RequestId};

use crate::host::{Host, refused};
use crate::look::{self, Sample};
use crate::plan::{self, Timed, Touch};
use crate::start::Phase;

const QUIET: Duration = Duration::from_millis(300);
const SETTLES_WITHIN: Duration = Duration::from_secs(2);
/// A simulator's pixels are exact: any point that differs is movement.
const TOLERANCE: u8 = 0;

/// Watching the picture until it stands still.
pub struct Watch {
    since: Instant,
    last_moved: Instant,
    /// The picture when the watch began, and whether any frame since has differed from it.
    before: Option<Sample>,
    changed: bool,
    last: Option<Sample>,
    seen: u64,
}

impl Watch {
    fn new(host: &Host, last: Option<Sample>) -> Watch {
        let now = Instant::now();
        Watch {
            since: now,
            last_moved: now,
            before: last.clone(),
            changed: false,
            last,
            seen: host.ring.copied_count(),
        }
    }

    /// Looks at the newest frame, if a new one came; whether it has stood still for `quiet`.
    fn still_for(&mut self, host: &Host, quiet: Duration) -> bool {
        let copied = host.ring.copied_count();
        if copied != self.seen
            && let Some(held) = host.ring.latest()
        {
            self.seen = copied;
            if let Some(now) = look::sample(&held.surface, host.sim.scale) {
                if self.last.as_ref().is_none_or(|last| moved(last, &now)) {
                    self.last_moved = Instant::now();
                }
                self.changed =
                    self.changed || self.before.as_ref().is_none_or(|before| moved(before, &now));
                self.last = Some(now);
            }
        }
        self.last_moved.elapsed() >= quiet
    }
}

/// Whether the screen moved from `before` to `now`: a text cursor blinking doesn't count, or a
/// wait would never end and an act would answer at the first blink.
fn moved(before: &Sample, now: &Sample) -> bool {
    let diff = look::compare(before, now, TOLERANCE);
    diff.differing > 0 && !diff.region.is_some_and(look::is_caret)
}

pub struct Running {
    pub request: RequestId,
    steps: Vec<Timed>,
    next: usize,
    started: Instant,
    before: Option<Sample>,
    watch: Option<Watch>,
    /// Keys and fingers down, to be let go if it stops early.
    keys_down: Vec<u16>,
    /// For typing: how many characters there are, and how many keys were pressed.
    typing: Option<(usize, usize)>,
}

pub struct Waiting {
    pub request: RequestId,
    quiet: Duration,
    until: Instant,
    watch: Watch,
}

impl Waiting {
    pub fn new(request: RequestId, quiet: Duration, timeout: Duration, host: &Host) -> Waiting {
        let last = host
            .ring
            .latest()
            .and_then(|held| look::sample(&held.surface, host.sim.scale));
        Waiting {
            request,
            quiet,
            until: Instant::now() + timeout,
            watch: Watch::new(host, last),
        }
    }
}

impl Host {
    /// What is under way moves on: the next act starts when none runs, the running one's due
    /// steps are sent, its look goes on, waits are checked. Whether the loop should come back
    /// soon.
    pub fn act_on(&mut self) -> bool {
        if self.running.is_none()
            && matches!(self.phase, Phase::Ready)
            && let Some((request, act)) = self.acts.pop_front()
        {
            self.begin(request, &act);
        }
        if let Some(mut running) = self.running.take()
            && self.step(&mut running)
        {
            self.running = Some(running);
        }
        self.wait_on();
        self.running.is_some() || !self.waits.is_empty() || !self.acts.is_empty()
    }

    fn begin(&mut self, request: RequestId, act: &Act) {
        let size = self.size();
        let (at, to) = match spots(self, act) {
            Ok(places) => places,
            Err(refusal) => return self.answer(request, Err(refusal)),
        };
        let steps = match plan::plan(act, at, to, size) {
            Ok(steps) => steps,
            Err(refusal) => return self.answer(request, Err(refusal)),
        };
        if let Act::Turn { to } = act {
            if let Err(error) = self.sim.turn(*to) {
                return self.answer(request, refused(RefusalCode::Failed, error));
            }
            self.follow_soon();
        }
        let before = self
            .ring
            .latest()
            .and_then(|held| look::sample(&held.surface, self.sim.scale));
        let typing = match act {
            Act::Type { text } => Some((text.chars().count(), 0)),
            _ => None,
        };
        self.running = Some(Running {
            request,
            steps,
            next: 0,
            started: Instant::now(),
            before,
            watch: None,
            keys_down: Vec::new(),
            typing,
        });
    }

    /// Sends what is due; then watches. Whether it is still under way.
    fn step(&mut self, running: &mut Running) -> bool {
        let elapsed = u32::try_from(running.started.elapsed().as_millis()).unwrap_or(u32::MAX);
        let size = self.size();
        while let Some(timed) = running.steps.get(running.next)
            && timed.at_ms <= elapsed
        {
            let sent = match &timed.touch {
                Touch::Fingers { at, touching, edge } => self.sim.touch(at, *touching, *edge, size),
                Touch::Key { usage, down } => {
                    self.sim.key(*usage, *down);
                    if *down {
                        running.keys_down.push(*usage);
                        if let Some((_, pressed)) = &mut running.typing {
                            *pressed += usize::from(*usage != 0xE1);
                        }
                    } else {
                        running.keys_down.retain(|key| key != usage);
                    }
                    Ok(())
                }
                Touch::Button { page, usage, down } => {
                    self.sim.button(*page, *usage, *down);
                    Ok(())
                }
            };
            running.next += 1;
            if let Err(error) = sent {
                self.sim.lift();
                self.answer(running.request, refused(RefusalCode::Failed, error));
                return false;
            }
        }
        if running.next < running.steps.len() {
            return true;
        }
        let watch = running
            .watch
            .get_or_insert_with(|| Watch::new(self, running.before.clone()));
        // Until the picture has changed and then stood still, or the time is up: a press whose
        // effect is a moment coming isn't taken for one that did nothing.
        let still = watch.still_for(self, QUIET);
        if !(still && watch.changed) && watch.since.elapsed() < SETTLES_WITHIN {
            return true;
        }
        let diff = match (&running.before, &watch.last) {
            (Some(before), Some(after)) => look::compare(before, after, TOLERANCE),
            _ => look::Diff {
                differing: 0,
                total: 0,
                region: None,
            },
        };
        let millis = u32::try_from(running.started.elapsed().as_millis()).unwrap_or(u32::MAX);
        self.answer(
            running.request,
            Ok(Answer::Acted {
                change: diff.change(),
                region: diff.region,
                still,
                millis,
            }),
        );
        false
    }

    /// Stops the running act where it is: its fingers lifted, its keys let go.
    pub fn interrupt(&mut self, code: RefusalCode, why: &str) {
        let Some(running) = self.running.take() else {
            return;
        };
        self.sim.lift();
        for usage in &running.keys_down {
            self.sim.key(*usage, false);
        }
        let message = match running.typing {
            Some((all, pressed)) => format!("{why}; {pressed} of {all} characters were typed"),
            None => why.to_owned(),
        };
        self.answer(running.request, refused(code, message));
    }

    fn wait_on(&mut self) {
        let mut waits = std::mem::take(&mut self.waits);
        waits.retain_mut(|waiting| {
            let still = waiting.watch.still_for(self, waiting.quiet);
            if still || Instant::now() >= waiting.until {
                let millis =
                    u32::try_from(waiting.watch.since.elapsed().as_millis()).unwrap_or(u32::MAX);
                self.answer(waiting.request, Ok(Answer::Waited { still, millis }));
                return false;
            }
            true
        });
        self.waits.extend(waits);
    }
}

/// An act's places as points: where it lands, and where a swipe ends.
fn spots(host: &Host, act: &Act) -> Result<(Point, Point), koyo_device_wire::Refusal> {
    let middle = {
        let size = host.size();
        Point {
            x: size.width / 2.,
            y: size.height / 2.,
        }
    };
    Ok(match act {
        Act::Tap { spot, .. } | Act::Scroll { spot, .. } | Act::Pinch { spot, .. } => {
            let at = host.place(spot)?;
            (at, at)
        }
        Act::Swipe { from, to, .. } => (host.place(from)?, host.place(to)?),
        _ => (middle, middle),
    })
}
