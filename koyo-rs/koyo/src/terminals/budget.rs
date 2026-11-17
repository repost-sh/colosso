//! What a terminal's output costs the window around it (`crate::budget`).

use std::path::Path;

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};
use koyo_term::{TermClient, TerminalView};
use koyo_term_control::{Status, TermDir};

use crate::budget::{Built, built_again, frame_of, within};
use crate::shell::Shell;
use crate::test_support::{draw, repo, scratch, select, shell_with, terminal_record};

/// A terminal tab shown on the checkout at `root`, the daemon having listed its terminal: its
/// view. The client is of a daemon that never starts, so the view is made, drawn under the
/// terminal's bar, and never attached: its frames are the view updated and notified, as
/// `on_update` does for each frame of output, and draw its notice, not a grid.
fn terminal_shown(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    root: &Path,
    cx: &mut TestAppContext,
) -> Entity<TerminalView> {
    let client = TermClient::new(
        TermDir::at(scratch("budget-no-daemon")),
        super::daemon_command(),
    );
    client.shutdown().unwrap();
    shell.update(cx, |shell, _| shell.terminals.client = Some(client));
    let record = terminal_record("zsh", root.to_str().unwrap(), Status::Idle, "prompt");
    let id = super::stop::tests::tab(shell, &record, cx);
    draw(window, cx);
    shell.read_with(cx, |shell, _| shell.terminals.view(id).unwrap())
}

#[gpui_kit::test]
fn a_frame_of_a_terminals_output_leaves_the_shell_alone(cx: &mut TestAppContext) {
    let root = repo("budget-terminal", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let view = terminal_shown(window, &shell, &root, cx);

    let budget = Built {
        shell: 0,
        views: 2,
        frames: 1,
    };
    let built = built_again(window, cx, frame_of(&view));
    within(built, budget, "a frame of a terminal's output");
}
