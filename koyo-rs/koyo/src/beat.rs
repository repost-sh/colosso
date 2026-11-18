//! The beat what goes round in Koyo while it waits moves to, in place of every frame the display
//! shows: a chat's spinners and its composer's edge, and the spinner a busy button, server or
//! loading view shows. Each asks, as it's drawn, to be drawn again at the next beat, and all that
//! asked are drawn again at once, in one frame however many there are.
//!
//! It's kept beside GPUI rather than in a global of its own: what a view reads of a global as it's
//! drawn, GPUI draws again when the global changes, and each asking changes it.

use std::cell::RefCell;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use gpui_kit::{App, EntityId, Task, Window};

/// A beat's time while Koyo is in front.
pub(crate) const IN_FRONT: Duration = Duration::from_micros(33_333);
/// A beat's time while another app is, often enough for what goes round to be seen going round:
/// a chat works while its person watches from elsewhere.
const BEHIND: Duration = Duration::from_millis(100);

/// Whether Koyo is the app in front, one of its windows having the keyboard: a window under one
/// of Koyo's own floating windows, the launcher or a menu, is in front too.
pub fn is_frontmost(cx: &App) -> bool {
    cx.active_window().is_some()
}

thread_local! {
    static BEAT: RefCell<Beat> = RefCell::default();
}

#[derive(Default)]
struct Beat {
    /// The views to draw again at the next beat.
    asked: HashSet<EntityId>,
    /// The next beat, and when it comes.
    next: Option<(Instant, Task<()>)>,
    /// When the first turn was asked of it, which every turn goes by.
    since: Option<Instant>,
}

/// Has the view being drawn drawn again at the next beat, at most 30 a second while Koyo is in
/// front and 10 while another app is. A window nobody can see (minimized, covered, on another
/// desktop) asks for none; the shell's window is drawn whole once it's seen again (`Shell::new`),
/// and what moves in it asks again then.
pub fn again(window: &Window, cx: &mut App) {
    if !crate::sight::window_seen(window) {
        return;
    }
    let pace = if is_frontmost(cx) { IN_FRONT } else { BEHIND };
    let due = cx.background_executor().now() + pace;
    BEAT.with_borrow_mut(|beat| {
        beat.asked.insert(window.current_view());
        if beat.next.as_ref().is_none_or(|(at, _)| due < *at) {
            beat.next = Some((due, beat_after(pace, cx)));
        }
    });
}

/// How far round what goes round once a `lap` is, as a share of the way, everything that goes
/// round off one clock; and the next beat asked for, unless motion is reduced, which holds it at
/// the start.
pub fn turn(lap: Duration, window: &Window, cx: &mut App) -> f32 {
    if cx.reduce_motion() {
        return 0.;
    }
    again(window, cx);
    let now = cx.background_executor().now();
    let since = BEAT.with_borrow_mut(|beat| *beat.since.get_or_insert(now));
    let gone = now.saturating_duration_since(since);
    (gone.as_secs_f64() / lap.as_secs_f64()).fract() as f32
}

/// The beat `pace` from now: it draws again everything that asked for it.
fn beat_after(pace: Duration, cx: &App) -> Task<()> {
    cx.spawn(async move |cx| {
        cx.background_executor().timer(pace).await;
        let asked = BEAT.with_borrow_mut(|beat| {
            beat.next = None;
            std::mem::take(&mut beat.asked)
        });
        cx.update(|cx| asked.into_iter().for_each(|id| cx.notify(id)));
    })
}
