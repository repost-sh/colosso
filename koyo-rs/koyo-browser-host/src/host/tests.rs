use super::{Load, Pane, load, queue, with};
use crate::native;

/// A pane with a page the engine has been told is on screen or not.
fn page(pane: u64, on_screen: bool) {
    with(|host| host.panes.insert(pane, Pane::new("https://example.com")));
    native::tests::opened(pane);
    native::on_screen(pane, on_screen);
}

/// How far apart the engine's work may run follows what it works for: a second with nothing
/// open, a tenth while pages are open and none is on screen, and a thirtieth while one is, one
/// is being opened, or a request of the app's waits for the engine's turn.
#[test]
#[expect(clippy::float_cmp, reason = "each is the constant itself, not near it")]
fn the_engine_runs_as_often_as_what_it_works_for_needs() {
    let at_most = || load().at_most();
    assert_eq!(at_most(), 1. / 30., "not up yet");
    with(|host| host.ready = true);
    assert_eq!(at_most(), 1., "nothing open");
    queue::pretend_drain_pending(true);
    assert_eq!(load(), Load::Seen, "a request waiting, with nothing open");
    queue::pretend_drain_pending(false);

    page(1, false);
    page(2, false);
    assert_eq!(at_most(), 1. / 10., "pages, none on screen");
    queue::pretend_drain_pending(true);
    assert_eq!(at_most(), 1. / 30., "a request waiting, as one to show a page does");
    queue::pretend_drain_pending(false);
    native::on_screen(2, true);
    assert_eq!(at_most(), 1. / 30., "one on screen");
    native::on_screen(2, false);
    with(|host| host.panes.insert(3, Pane::new("https://example.com")));
    assert_eq!(at_most(), 1. / 30., "one being opened, with no page yet");
}
