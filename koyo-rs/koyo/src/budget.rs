//! Render budgets: what a named change makes the window build, counted in a headless window
//! and held by a test. GPUI keeps a view from one frame to the next and builds it again only
//! when it was notified or something it reached into while rendering changed, so the count of
//! views built for a change is what that change costs, whatever the machine. A budget fails
//! when a notify, a read or a timer added to the shell makes a change build more than it did.
//!
//! The shell is the window's root and draws the sidebar, the tab strip and the rails itself, so
//! the budget that matters most is that a pane's own frames leave it alone: a terminal's output
//! (`terminals/budget.rs`), a chat's answer arriving and its spinner turning
//! (`chat/tests/budget.rs`). What breaks it is the shell reaching into the pane's view while it
//! renders, by `read` or by `update`: keep what the shell needs of a view beside the view, told
//! by the view's events, so the window doesn't read the view as it draws, and update a view the
//! shell tells what to show only when that changed: one updated and not notified builds again
//! all the same when the shell it's in renders.
//!
//! The frames are the ones the window asks for, delivered a display frame apart on the test
//! clock as the app's display link delivers them, so an animation is seen and comes to rest as
//! it does in the app. A window that keeps asking fails the budget, unless the test means it to
//! and counts what it builds over a set time (`built_for`).

use std::cell::Cell;
use std::time::Duration;

use gpui_kit::{AnyWindowHandle, App, AppContext as _, Entity, TestAppContext, Window};

use crate::settings::Layout;

mod deferred;
use crate::test_support::{in_front, shell_on};

thread_local! {
    static SHELL_RENDERS: Cell<usize> = const { Cell::new(0) };
}

/// Counts a render of the shell, which `Shell::render` tells of in test builds.
pub fn shell_rendered() {
    SHELL_RENDERS.set(SHELL_RENDERS.get() + 1);
}

/// What a change made the window build.
/// What a change that costs nothing builds is the default: no frame is drawn for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Built {
    /// How many times the shell rendered.
    pub shell: usize,
    /// How many views were built, the shell among them when it rendered.
    pub views: u64,
    /// How many frames were drawn.
    pub frames: u64,
}

/// The time between two frames of a 60 Hz display.
const FRAME: Duration = Duration::from_micros(16_667);

/// The most frames a window may ask for in a row before it's taken never to come to rest: two
/// seconds of them, longer than any of the app's eases.
const RESTLESS: usize = 120;

/// What `change` makes `window` build, from rest to rest: the window is brought to rest,
/// `change` runs, and every frame after it is counted until the window asks for no more.
pub fn built_after(
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
    change: impl FnOnce(&mut TestAppContext),
) -> Built {
    come_to_rest(window, cx, "before the change");
    count(window, cx, |cx| {
        change(cx);
        come_to_rest(window, cx, "after the change");
    })
}

/// Delivers the frames `window` asks for until it asks for none, which it must within
/// [`RESTLESS`] of them.
fn come_to_rest(window: AnyWindowHandle, cx: &mut TestAppContext, when: &str) {
    let asked = frames(window, cx, RESTLESS);
    assert!(
        asked < RESTLESS,
        "{when}, the window never comes to rest: it asks for a frame at every frame"
    );
}

/// What `window` builds while the clock passes by `time`, a display frame at a time, with each
/// frame it asks for delivered: for what keeps going for as long as it's shown, whether it asks
/// for every frame or draws itself again on a timer of its own. A timer that runs out draws the
/// window on its own here, where the app draws all that changed by the next display frame in one,
/// so what moves on timers of its own counts more frames here than the app draws.
pub fn built_for(window: AnyWindowHandle, cx: &mut TestAppContext, time: Duration) -> Built {
    count(window, cx, |cx| {
        cx.run_until_parked();
        let mut passed = Duration::ZERO;
        while passed < time {
            cx.executor().advance_clock(FRAME);
            on(window, cx, Window::simulate_next_frame);
            cx.run_until_parked();
            passed += FRAME;
        }
    })
}

/// What `time` of `window` builds behind another app's window, in front, and behind again, each
/// after a second to settle, held at `behind` and `front` ([`ratchet`]). Behind is where a person
/// watches from another app, the window still in sight. It's measured once the window has been in
/// front a while, as one in use has: a test's window starts behind without ever having lost the
/// keyboard, so a caret in it blinks there as none does behind in use.
pub fn held_behind_and_in_front(
    window: AnyWindowHandle,
    time: Duration,
    behind: Built,
    front: Built,
    cx: &mut TestAppContext,
) {
    let second = Duration::from_secs(1);
    in_front(window, true, cx);
    built_for(window, cx, second);
    let time_of = |front: bool, cx: &mut TestAppContext| {
        in_front(window, front, cx);
        built_for(window, cx, second);
        built_for(window, cx, time)
    };
    ratchet(time_of(false, cx), behind, "behind another app's");
    ratchet(time_of(true, cx), front, "in front");
    ratchet(time_of(false, cx), behind, "put behind again");
}

/// What the clock passing by `time` makes `window` build, from rest to rest: the timers that run
/// out in it, and what they start.
pub fn built_during(window: AnyWindowHandle, cx: &mut TestAppContext, time: Duration) -> Built {
    built_after(window, cx, |cx| cx.executor().advance_clock(time))
}

/// Lets every timer the setup started run out, so a change measured over the clock passing
/// counts only what it starts.
pub fn settle(cx: &mut TestAppContext) {
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
}

/// How many times the app wakes for a timer in a minute of the clock, once what was going on has
/// come to rest ([`settle`]): each moment one or more timers run out is one wake, as one wake of
/// the thread runs all that came due then, a timer whose task was let go among them. The clock is
/// left at the minute's end, or at the first timer after it, which is left to run.
pub fn wakes_a_minute(cx: &mut TestAppContext) -> usize {
    settle(cx);
    let end = cx.executor().now() + Duration::from_secs(60);
    let mut wakes = 0;
    while cx.dispatcher.advance_clock_to_next_timer() && cx.executor().now() <= end {
        wakes += 1;
        cx.run_until_parked();
    }
    let now = cx.executor().now();
    if now < end {
        cx.executor().advance_clock(end - now);
    }
    wakes
}

/// What `change` builds the second time it's made: the first time after a tab is shown also
/// builds what the change makes for the first time, and the shell, which hands the tab's view
/// the keyboard by reading it then.
pub fn built_again(
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
    change: impl Fn(&mut TestAppContext),
) -> Built {
    built_after(window, cx, &change);
    built_after(window, cx, change)
}

/// A frame of `view`'s own, as its frame of output is a terminal's: it changes and says so.
pub fn frame_of<V: 'static>(view: &Entity<V>) -> impl Fn(&mut TestAppContext) + '_ {
    |cx| view.update(cx, |_, cx| cx.notify())
}

/// Asserts that `built` keeps to `budget`: the shell rendered and the frames drawn as many
/// times, and no more views built. `what` names the change.
pub fn within(built: Built, budget: Built, what: &str) {
    assert_eq!(
        (built.shell, built.frames),
        (budget.shell, budget.frames),
        "{what}: the shell rendered {} times in {} frames, where its budget is {} in {}",
        built.shell,
        built.frames,
        budget.shell,
        budget.frames,
    );
    assert!(
        built.views <= budget.views,
        "{what}: {} views were built, where the budget is {}",
        built.views,
        budget.views,
    );
}

/// Asserts that `built` is `held`, a budget held at what a change costs today that a fix should
/// lower: the shell rendered, the views built and the frames drawn as many times. It fails when
/// the cost rises, a regression, and when it falls, which takes the lower number.
pub fn ratchet(built: Built, held: Built, what: &str) {
    let worse = built.shell > held.shell || built.views > held.views || built.frames > held.frames;
    assert!(
        !worse,
        "{what}: a regression, {built:?} built where {held:?} is held"
    );
    assert!(
        built == held,
        "{what}: better: lower the held number from {held:?} to {built:?}"
    );
}

/// What `window` built while `run` ran.
fn count(
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
    run: impl FnOnce(&mut TestAppContext),
) -> Built {
    on(window, cx, |window, _| window.reset_layout_stats());
    SHELL_RENDERS.set(0);
    run(cx);
    let stats = on(window, cx, |window, _| window.layout_stats());
    Built {
        shell: SHELL_RENDERS.get(),
        views: stats.views_built,
        frames: stats.frames,
    }
}

/// Delivers the frames `window` asks for, a display frame apart, up to `most` of them: how many
/// it asked for. The test platform draws a window as soon as an update leaves it dirty, as the
/// app does at the next frame, so no frame is forced on a window that isn't.
fn frames(window: AnyWindowHandle, cx: &mut TestAppContext, most: usize) -> usize {
    cx.run_until_parked();
    let mut asked = 0;
    while asked < most {
        cx.executor().advance_clock(FRAME);
        let callbacks = on(window, cx, Window::simulate_next_frame);
        cx.run_until_parked();
        if callbacks == 0 {
            break;
        }
        asked += 1;
    }
    asked
}

fn on<R>(
    window: AnyWindowHandle,
    cx: &mut TestAppContext,
    read: impl FnOnce(&mut Window, &mut App) -> R,
) -> R {
    cx.update_window(window, |_, window, cx| read(window, cx))
        .unwrap()
}

#[gpui_kit::test]
fn a_window_nothing_changes_in_draws_no_frame_however_long_it_waits(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-rest", Layout::Orca, cx);
    let minute = Duration::from_secs(60);

    assert_eq!(built_after(window, cx, |_| {}), Built::default());
    assert_eq!(
        built_during(window, cx, minute),
        Built::default(),
        "a timer draws the window while nothing changes in it"
    );

    shell.update(cx, |_, cx| {
        cx.spawn(async move |shell, cx| {
            let half = Duration::from_secs(30);
            cx.background_executor().timer(half).await;
            shell.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    });
    let shell_once = Built {
        shell: 1,
        views: 2,
        frames: 1,
    };
    within(
        built_during(window, cx, minute),
        shell_once,
        "a timer that draws the shell, which the count sees, so the minute above had none",
    );
}

#[gpui_kit::test]
fn a_change_to_the_shell_renders_it_once_in_one_frame(cx: &mut TestAppContext) {
    let (window, shell, _) = shell_on("budget-shell", Layout::Orca, cx);

    let built = built_after(window, cx, |cx| shell.update(cx, |_, cx| cx.notify()));
    let budget = Built {
        shell: 1,
        views: 1,
        frames: 1,
    };
    within(built, budget, "the shell notified");
}
