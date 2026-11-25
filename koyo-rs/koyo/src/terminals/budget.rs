//! What a terminal's output costs the window around it (`crate::budget`).

use std::path::Path;

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};
use koyo_term::{TermClient, TerminalView};
use koyo_term_control::{Status, TermDir};

use crate::budget::{Built, built_after, built_again, frame_of, ratchet, within};
use crate::shell::Shell;
use crate::test_support::{
    draw, open_changes, repo, scratch, select, shell_on, shell_with, status_event, terminal_record,
};

/// A terminal tab shown on the checkout at `root`, the daemon having listed its terminal: its
/// view. The client is of a daemon that never starts, so the view is made, drawn under the
/// terminal's bar, and never attached: its frames are the view updated and notified, as
/// `on_update` does for each frame of output, and draw its notice, not a grid.
pub(crate) fn terminal_shown(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    root: &Path,
    cx: &mut TestAppContext,
) -> Entity<TerminalView> {
    no_daemon(shell, cx);
    let record = terminal_record("zsh", root.to_str().unwrap(), Status::Idle, "prompt");
    let id = super::stop::tests::tab(shell, &record, cx);
    draw(window, cx);
    shell.read_with(cx, |shell, _| shell.terminals.view(id).unwrap())
}

/// The shell's terminals have a client of a daemon that never starts, so their views are made
/// and never attached.
pub(crate) fn no_daemon(shell: &Entity<Shell>, cx: &mut TestAppContext) {
    let client = TermClient::new(
        TermDir::at(scratch("budget-no-daemon")),
        super::daemon_command(),
    );
    client.shutdown().unwrap();
    shell.update(cx, |shell, _| shell.terminals.client = Some(client));
}

/// A frame of a terminal's output leaves the shell alone, beside the open right panel too: the
/// panel's edge is drawn in the panel, after the terminal (`theme/card.rs`), rather than among
/// the window's overlays, which GPUI could reach only by drawing the shell again around the
/// terminal (gpui-fast's `splice_layout`).
#[gpui_kit::test]
fn a_frame_of_a_terminals_output_leaves_the_shell_alone_beside_the_right_panel_too(
    cx: &mut TestAppContext,
) {
    let (window, shell, root) = shell_on("budget-terminal", cx);
    let view = terminal_shown(window, &shell, &root, cx);

    let budget = Built {
        shell: 0,
        views: 2,
        frames: 1,
    };
    let built = built_again(window, cx, frame_of(&view));
    within(built, budget, "a frame of a terminal's output");

    open_changes(window, &shell, cx);
    let built = built_again(window, cx, frame_of(&view));
    within(
        built,
        budget,
        "a frame of a terminal's output beside the right panel",
    );
}

/// What the daemon says of a terminal's status renders the shell while the shell shows it: on
/// its tab's dot in the selected checkout's strip and in the bar over its pane. A terminal of
/// another checkout shows only in that checkout's row, as a count of those waiting for the
/// person, so it renders the shell only when it starts or stops waiting.
#[gpui_kit::test]
fn a_terminals_status_renders_the_shell_only_while_the_shell_shows_it(cx: &mut TestAppContext) {
    let (root, other) = (
        repo("budget-status", &[("README.md", "hi\n")]),
        repo("budget-status-other", &[("README.md", "hi\n")]),
    );
    let (window, shell) = shell_with(&[&root, &other], cx);
    select(&shell, &root, cx);
    draw(window, cx);
    terminal_shown(window, &shell, &root, cx);
    let told = |status: Status| {
        let shell = shell.clone();
        move |cx: &mut TestAppContext| {
            let event = status_event("zsh", status, &format!("{status:?}"));
            shell.update(cx, |shell, cx| shell.terminal_notice(&event, cx));
        }
    };

    built_after(window, cx, told(Status::Running));
    let shown = Built {
        shell: 1,
        views: 1,
        frames: 1,
    };
    let built = built_after(window, cx, told(Status::Idle));
    ratchet(built, shown, "a shown terminal's status");

    select(&shell, &other, cx);
    draw(window, cx);
    let built = built_after(window, cx, told(Status::Running));
    ratchet(built, Built::default(), "the status of a terminal of another checkout");
    let built = built_after(window, cx, told(Status::NeedsInput));
    ratchet(built, shown, "a terminal of another checkout waiting for the person");
}
