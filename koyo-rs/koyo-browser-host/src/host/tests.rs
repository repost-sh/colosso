use super::{Load, Pane, load, queue, with};
use crate::native;

/// A pane with a page no pane draws, which an agent keeps on screen for the engine or not.
fn page(pane: u64, kept: bool) {
    with(|host| host.panes.insert(pane, Pane::new("https://example.com")));
    native::tests::opened(pane);
    native::sight(pane, kept, true);
}

/// What the engine works for follows what's open: waited on while it isn't up or a request of
/// the app's waits for its turn, idle with nothing open, unseen while pages are open and none is
/// on screen, and shown while one is or one is being opened.
#[test]
fn what_the_engine_works_for_follows_what_is_open() {
    assert_eq!(load(), Load::Waited, "not up yet");
    with(|host| host.ready = true);
    assert_eq!(load(), Load::Idle, "nothing open");
    queue::pretend_drain_pending(true);
    assert_eq!(load(), Load::Waited, "a request waiting, with nothing open");
    queue::pretend_drain_pending(false);

    page(1, false);
    page(2, false);
    assert_eq!(load(), Load::Unseen, "pages, none on screen");
    queue::pretend_drain_pending(true);
    assert_eq!(load(), Load::Waited, "a request waiting, as one to show a page does");
    queue::pretend_drain_pending(false);
    native::sight(2, true, true);
    assert_eq!(load(), Load::Shown, "one on screen");
    native::sight(2, false, true);
    with(|host| host.panes.insert(3, Pane::new("https://example.com")));
    assert_eq!(load(), Load::Shown, "one being opened, with no page yet");
}

/// A load's progress reaches the app as it first says, then a step on at a time, and whole.
#[test]
fn loading_is_told_a_step_at_a_time() {
    use super::panes::moved_on;
    assert!(moved_on(None, 0), "the first a load says");
    assert!(!moved_on(Some(10), 12), "two points on");
    assert!(moved_on(Some(10), 15), "a step on");
    assert!(moved_on(Some(98), 100), "all of it");
    assert!(!moved_on(Some(100), 100), "the same");
    let told: Vec<u8> = (0..=100).scan(None, |was, now| {
        let tell = moved_on(*was, now);
        if tell {
            *was = Some(now);
        }
        Some(tell.then_some(now))
    })
    .flatten()
    .collect();
    assert_eq!(told.len(), 21, "a load from 0 to 100 told {told:?}");
}
