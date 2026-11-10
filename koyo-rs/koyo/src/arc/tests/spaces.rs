//! A space that isn't shown says on its mark what its tiles would (`docs/arc-sidebar.md` 7): a
//! dot while something in it waits for the person, a spinner while something works.

use gpui_kit::{TestAppContext, VisualTestContext};
use koyo_agent::protocol::model::Status;
use koyo_agent::protocol::requests::RequestKind;

use super::{act, arc_shell, hear};
use crate::arc::actions::MoveProjectToNewSpace;
use crate::chat::Activity;
use crate::test_support::draw;

#[gpui_kit::test]
fn a_space_not_shown_says_what_its_tiles_would(cx: &mut TestAppContext) {
    let (window, shell, roots) = arc_shell(&["arc-pill-app", "arc-pill-site"], cx);
    let site = roots[1].clone();
    act(window, MoveProjectToNewSpace(site.clone()), cx);
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    draw(window, cx);

    // A chat in the other space's project, which has no tab here to mark it seen.
    let hear = |status, needs, unread, cx: &mut TestAppContext| {
        hear(&shell, ("0001", &site), (status, needs, unread), cx);
        draw(window, cx);
    };
    // What each space says, and whether each mark carries a pill, the first space shown.
    let told = |cx: &mut TestAppContext| {
        let says = cx.read(|cx| {
            let shell = shell.read(cx);
            let project = shell.workspace.project_of(&site).expect("the project");
            assert_eq!(
                shell.space_activity(1, cx),
                shell.project_activity(project, cx),
                "what the project's tile would say"
            );
            (shell.space_activity(0, cx), shell.space_activity(1, cx))
        });
        let mut vcx = VisualTestContext::from_window(window, cx);
        let pills = (
            vcx.debug_bounds("arc-space-pill-0").is_some(),
            vcx.debug_bounds("arc-space-pill-1").is_some(),
        );
        (says, pills)
    };
    let quiet = Activity::Quiet;
    assert_eq!(told(cx), ((quiet, quiet), (false, false)));
    hear(Status::Working, None, false, cx);
    assert_eq!(told(cx), ((quiet, Activity::Working), (false, true)));
    hear(Status::Idle, None, true, cx);
    assert_eq!(
        told(cx),
        ((quiet, Activity::Attention), (false, true)),
        "finished and not seen"
    );
    hear(Status::Working, Some(RequestKind::Approval), false, cx);
    assert_eq!(told(cx), ((quiet, Activity::Attention), (false, true)));

    // Its mark never shifts the others: the pill is laid over it.
    let marks = |cx: &mut TestAppContext| {
        let mut vcx = VisualTestContext::from_window(window, cx);
        (
            vcx.debug_bounds("arc-space-0").expect("the first mark"),
            vcx.debug_bounds("arc-space-1").expect("the second mark"),
        )
    };
    let with_pill = marks(cx);
    hear(Status::Idle, None, false, cx);
    assert_eq!(told(cx), ((quiet, quiet), (false, false)), "seen: it clears");
    assert_eq!(marks(cx), with_pill);

    // Shown, the space says it on its tiles, and the one left says its own.
    hear(Status::Idle, None, true, cx);
    shell.update(cx, |shell, cx| shell.show_space(1, cx));
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    assert!(vcx.debug_bounds("arc-space-pill-1").is_none());
    assert!(vcx.debug_bounds("arc-space-pill-0").is_none());
}
