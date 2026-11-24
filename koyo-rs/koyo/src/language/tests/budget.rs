//! What a server's item in the status bar costs the window while the server is busy
//! (`crate::budget`): its spinner turns at the beat (`crate::beat`), each beat building the
//! servers' view alone, where gpui-kit's spinner built it at every frame the display showed, 60
//! a second here. In front, the file shown has the keyboard, and each blink of its caret is a
//! frame of its own too.

use std::time::Duration;

use gpui_kit::TestAppContext;
use serde_json::json;

use super::support::*;
use crate::budget::{Built, held_behind_and_in_front};
use crate::language::*;
use crate::test_support::repo;

#[gpui_kit::test]
fn a_busy_servers_spinner_builds_the_servers_view_at_each_beat(cx: &mut TestAppContext) {
    let root = repo("language-busy", &[("src/lib.rs", LIB)]);
    let (launcher, _) = launcher(|_, _| json!([{"recv": "initialize"}, {"hang": true}]));
    let shell = shell(&[&root], launcher, cx);
    let _lib = open(&shell, &root, "src/lib.rs", cx);
    let window = shell.0;
    let server = server(&root, cx).unwrap();
    assert_eq!(
        cx.read(|cx| server.read(cx).state().clone()),
        State::Starting
    );
    let held_behind = Built {
        shell: 0,
        views: 10,
        frames: 10,
    };
    let held_in_front = Built {
        shell: 0,
        views: 34,
        frames: 32,
    };
    held_behind_and_in_front(
        window,
        Duration::from_secs(1),
        held_behind,
        held_in_front,
        cx,
    );
    let _ = std::fs::remove_dir_all(&root);
}
