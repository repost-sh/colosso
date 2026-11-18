//! The activity panel's Output tab reads its terminal's output on only while a person can see the
//! terminal, and reads what came meanwhile as soon as they can again: not with its tab hidden,
//! Settings over the workspace or its window unseen, unless the harness counts that one seen.

use std::time::Duration;

use gpui_kit::{AnyWindowHandle, Entity, TestAppContext};

use super::{ActivityPanel, PanelTab};
use crate::navigation::Route;
use crate::settings::Layout;
use crate::settings_view::SettingsPage;
use crate::shell::Shell;
use crate::terminals::budget::terminal_shown;
use crate::test_support::{draw, on_screen, open_page, shell_on};

/// The terminal shown with its activity panel open on the Output tab, a mark set: the panel and
/// the terminal's pane.
fn reading_output(
    window: AnyWindowHandle,
    shell: &Entity<Shell>,
    root: &std::path::Path,
    cx: &mut TestAppContext,
) -> (Entity<ActivityPanel>, u64) {
    terminal_shown(window, shell, root, cx);
    let (pane, id) = shell.read_with(cx, |shell, _| {
        let pane = shell.workspace.tab_shown().unwrap().first();
        (pane.id, pane.terminal().unwrap())
    });
    shell.update(cx, |shell, cx| shell.toggle_activity(id, cx));
    let panel = shell.read_with(cx, |shell, _| shell.terminals.panels[&id].clone());
    panel.update(cx, |panel, cx| {
        panel.output.mark = Some(0);
        panel.show(PanelTab::Output, cx);
    });
    draw(window, cx);
    (panel, pane)
}

/// How many times the panel has asked the daemon for output after `time` more has passed.
fn reads_after(time: Duration, panel: &Entity<ActivityPanel>, cx: &mut TestAppContext) -> usize {
    cx.executor().advance_clock(time);
    cx.run_until_parked();
    panel.read_with(cx, |panel, _| panel.reads.get())
}

#[gpui_kit::test]
fn the_output_tab_reads_only_while_its_terminal_is_seen_and_at_once_when_it_is_again(
    cx: &mut TestAppContext,
) {
    let (window, shell, root) = shell_on("panel-sight", Layout::Orca, cx);
    let (panel, pane) = reading_output(window, &shell, &root, cx);
    let second = Duration::from_secs(1);
    let shown = reads_after(Duration::ZERO, &panel, cx);
    assert_eq!(reads_after(3 * second, &panel, cx), shown + 3, "a read each second");

    open_page(window, &shell, Some("https://example.com/"), cx);
    draw(window, cx);
    let hidden = reads_after(Duration::ZERO, &panel, cx);
    assert_eq!(reads_after(3 * second, &panel, cx), hidden, "reads with its tab not shown");
    shell.update(cx, |shell, cx| assert!(shell.show_tab_holding(pane, cx)));
    draw(window, cx);
    assert_eq!(reads_after(Duration::ZERO, &panel, cx), hidden + 1, "shown, it reads at once");

    on_screen(window, false, cx);
    let unseen = reads_after(Duration::ZERO, &panel, cx);
    assert_eq!(reads_after(3 * second, &panel, cx), unseen, "reads in a window nobody sees");
    on_screen(window, true, cx);
    assert_eq!(reads_after(Duration::ZERO, &panel, cx), unseen + 1, "seen, it reads at once");
}

#[gpui_kit::test]
fn the_output_tab_reads_nothing_behind_settings_and_at_once_back_on_the_workspace(
    cx: &mut TestAppContext,
) {
    let (window, shell, root) = shell_on("panel-settings", Layout::Orca, cx);
    let (panel, _) = reading_output(window, &shell, &root, cx);
    shell.update(cx, |shell, cx| shell.navigate(Route::Settings(SettingsPage::General), cx));
    draw(window, cx);
    let behind = reads_after(Duration::ZERO, &panel, cx);
    assert_eq!(reads_after(Duration::from_secs(3), &panel, cx), behind, "reads behind Settings");

    shell.update(cx, |shell, cx| shell.navigate(Route::Workspace, cx));
    draw(window, cx);
    assert_eq!(reads_after(Duration::ZERO, &panel, cx), behind + 1, "back, it reads at once");
}

/// The harness drives a Koyo on a desktop nobody looks at (`tick`, `crate::sight`): a window
/// there counts as seen, so the panel reads on as if a person watched.
#[gpui_kit::test]
fn the_output_tab_reads_on_in_a_window_the_harness_counts_as_seen(cx: &mut TestAppContext) {
    let (window, shell, root) = shell_on("panel-unseen-counts", Layout::Orca, cx);
    let (panel, _) = reading_output(window, &shell, &root, cx);
    crate::sight::count_unseen(true);
    on_screen(window, false, cx);
    let unseen = reads_after(Duration::ZERO, &panel, cx);
    let read = reads_after(Duration::from_secs(3), &panel, cx);
    crate::sight::count_unseen(false);
    assert_eq!(read, unseen + 3, "a read each second, counted seen");
}
