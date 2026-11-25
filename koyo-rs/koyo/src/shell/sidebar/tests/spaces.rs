//! A space that isn't shown says on its mark what its projects would (`docs/sidebar.md` 4): a
//! dot while something in it waits for the person, a spinner while something works.

use gpui_kit::{TestAppContext, VisualTestContext};
use koyo_agent::protocol::model::Status;
use koyo_agent::protocol::requests::RequestKind;

use super::{act, going_on, hear, sidebar_shell};
use crate::chat::{Activity, Attention};
use crate::projects::MoveProjectToNewSpace;
use crate::test_support::draw;

#[gpui_kit::test]
fn a_space_not_shown_says_what_its_projects_would(cx: &mut TestAppContext) {
    let (window, shell, roots) = sidebar_shell(&["sidebar-pill-app", "sidebar-pill-site"], cx);
    let site = roots[1].clone();
    act(window, MoveProjectToNewSpace(site.clone()), cx);
    shell.update(cx, |shell, cx| shell.show_space(0, cx));
    draw(window, cx);

    let hear = |status, needs, unread, cx: &mut TestAppContext| {
        hear(&shell, ("0001", &site), (status, needs, unread), cx);
        draw(window, cx);
    };
    let told = |cx: &mut TestAppContext| {
        let says = cx.read(|cx| {
            let shell = shell.read(cx);
            let project = shell.workspace.project_of(&site).expect("the project");
            assert_eq!(
                shell.space_activity(1, cx),
                shell.project_activity(project, cx),
                "what the project would say"
            );
            (shell.space_activity(0, cx), shell.space_activity(1, cx))
        });
        let mut vcx = VisualTestContext::from_window(window, cx);
        let pills = (
            ["space-mark-spinner-0", "space-mark-dot-0"].map(|name| vcx.debug_bounds(name).is_some()),
            ["space-mark-spinner-1", "space-mark-dot-1"].map(|name| vcx.debug_bounds(name).is_some()),
        );
        (says, pills)
    };
    let marks = |cx: &mut TestAppContext| {
        let mut vcx = VisualTestContext::from_window(window, cx);
        (
            vcx.debug_bounds("space-mark-0").expect("the first mark"),
            vcx.debug_bounds("space-mark-1").expect("the second mark"),
        )
    };
    let quiet = Activity::default();
    let nothing = [false, false];
    assert_eq!(told(cx), ((quiet, quiet), (nothing, nothing)));
    hear(Status::Working, None, false, cx);
    assert_eq!(told(cx), ((quiet, going_on(true, None)), (nothing, [true, false])));
    let pill = |name: &'static str, cx: &mut TestAppContext| {
        let mut vcx = VisualTestContext::from_window(window, cx);
        vcx.debug_bounds(name).expect("what the mark says")
    };
    let spinner = pill("space-mark-spinner-1", cx);
    hear(Status::Idle, None, true, cx);
    assert_eq!(
        told(cx),
        ((quiet, going_on(false, Some(Attention::Unread))), (nothing, [false, true])),
        "finished and not seen"
    );
    let (dot, mark) = (pill("space-mark-dot-1", cx), marks(cx).1);
    let apart = spinner.center() - dot.center();
    assert!(apart.x.abs() < gpui_kit::px(0.5) && apart.y.abs() < gpui_kit::px(0.5));
    assert_eq!(spinner.size.width, dot.size.width * 1.5);
    assert!(dot.center().x > mark.center().x && dot.center().y < mark.center().y);
    hear(Status::Working, Some(RequestKind::Approval), false, cx);
    let asks = going_on(false, Some(Attention::NeedsYou));
    assert_eq!(told(cx), ((quiet, asks), (nothing, [false, true])));
    hear(Status::Working, None, true, cx);
    let both = going_on(true, Some(Attention::Unread));
    assert_eq!(told(cx), ((quiet, both), (nothing, [true, true])));
    assert_eq!(pill("space-mark-spinner-1", cx), spinner, "in its place");
    let beside = pill("space-mark-dot-1", cx);
    assert!(beside.right() < spinner.left(), "{beside:?} beside {spinner:?}");

    let with_pill = marks(cx);
    hear(Status::Idle, None, false, cx);
    assert_eq!(
        told(cx),
        ((quiet, quiet), (nothing, nothing)),
        "seen: it clears"
    );
    assert_eq!(marks(cx), with_pill);

    hear(Status::Idle, None, true, cx);
    shell.update(cx, |shell, cx| shell.show_space(1, cx));
    draw(window, cx);
    let mut vcx = VisualTestContext::from_window(window, cx);
    assert!(vcx.debug_bounds("space-mark-dot-1").is_none());
    assert!(vcx.debug_bounds("space-mark-dot-0").is_none());
}
