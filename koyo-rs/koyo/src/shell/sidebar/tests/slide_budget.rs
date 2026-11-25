//! What a space's slide costs the window (`crate::budget`), held at what it costs today: each
//! frame of it renders the shell once, which lays the space out at its offset, and builds no
//! view of the tabs it doesn't show. A slide that built the shown tab's view at every frame, or
//! asked for frames once it came to rest, fails here as a regression.

use gpui_kit::TestAppContext;

use super::{act, sidebar_shell};
use crate::budget::{Built, built_after, ratchet};
use crate::projects::MoveProjectToNewSpace;
use crate::test_support::open_page;

#[gpui_kit::test]
fn each_frame_of_a_space_slide_renders_the_shell_once(cx: &mut TestAppContext) {
    let (window, shell, roots) = sidebar_shell(&["sidebar-budget-app", "sidebar-budget-site"], cx);
    open_page(window, &shell, Some("https://one.example/"), cx);
    act(window, MoveProjectToNewSpace(roots[1].clone()), cx);
    open_page(window, &shell, Some("https://two.example/"), cx);
    cx.update(|cx| cx.set_reduce_motion(false));

    let built = built_after(window, cx, |cx| {
        shell.update(cx, |shell, cx| shell.slide_to_space(0, cx));
    });
    assert_eq!(
        built.shell as u64, built.frames,
        "a slide renders the shell once a frame: {built:?}"
    );
    let held = Built {
        shell: 52,
        views: 63,
        frames: 52,
    };
    ratchet(built, held, "a space's slide");
}
