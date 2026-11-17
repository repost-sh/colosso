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
//! by the view's events, and compare what the shell tells a view before telling it.
//!
//! The frames are the ones the window asks for, delivered a display frame apart on the test
//! clock as the app's display link delivers them, so an animation is seen and comes to rest as
//! it does in the app. A window that keeps asking fails the budget, unless the test means it to
//! and counts a set number of frames (`built_over`).

use std::cell::Cell;
use std::time::Duration;

use gpui_kit::{AnyWindowHandle, App, AppContext as _, Entity, TestAppContext, Window};

use crate::test_support::{draw, repo, select, shell_with};

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

/// What the next `n` frames build, for a window that's meant to keep asking for them, as while
/// something animates for as long as it's shown.
pub fn built_over(window: AnyWindowHandle, cx: &mut TestAppContext, n: usize) -> Built {
    count(window, cx, |cx| {
        let asked = frames(window, cx, n);
        assert_eq!(
            asked, n,
            "the window came to rest after {asked} of {n} frames"
        );
    })
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
    let root = repo("budget-rest", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    draw(window, cx);
    let minute = |cx: &mut TestAppContext| cx.executor().advance_clock(Duration::from_secs(60));

    assert_eq!(built_after(window, cx, |_| {}), Built::default());
    assert_eq!(
        built_after(window, cx, minute),
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
        views: 3,
        frames: 1,
    };
    within(
        built_after(window, cx, minute),
        shell_once,
        "a timer that draws the shell, which the count sees, so the minute above had none",
    );
}

#[gpui_kit::test]
fn a_change_to_the_shell_renders_it_once_in_one_frame(cx: &mut TestAppContext) {
    let root = repo("budget-shell", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    draw(window, cx);

    let built = built_after(window, cx, |cx| shell.update(cx, |_, cx| cx.notify()));
    let budget = Built {
        shell: 1,
        views: 2,
        frames: 1,
    };
    within(built, budget, "the shell notified");
}
