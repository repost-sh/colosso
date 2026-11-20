use gpui_kit::{Entity, TestAppContext};
use koyo_term::Source;
use koyo_term_control::{Control, Handle, Status, TerminalRecord};

use super::busy;
use crate::settings::Layout;
use crate::shell::Shell;
use crate::terminals::budget::terminal_shown;
use crate::terminals::states::State;
use crate::test_support::{
    close_shown, draw, repo, select, shell_on, shell_with, terminal_record as record,
};

/// A terminal tab showing `record`, the daemon having said so; its id.
pub(in crate::terminals) fn tab(shell: &Entity<Shell>, record: &TerminalRecord, cx: &mut TestAppContext) -> u64 {
    shell.update(cx, |shell, cx| {
        shell.terminals.states.upsert(record);
        let id = shell.terminals.add(Source::Existing {
            handle: record.handle.clone(),
            name: record.name.clone(),
        });
        let worktree = record.worktree.as_ref().unwrap().path.clone();
        shell
            .workspace
            .add_terminal_tab(&worktree, id, record.name.clone().into(), true);
        cx.notify();
        id
    })
}

fn open(shell: &Entity<Shell>, id: u64, cx: &mut TestAppContext) -> bool {
    shell.read_with(cx, |shell, _| {
        shell.workspace.terminal_tabs().any(|(_, tab)| tab == id)
    })
}

#[test]
fn only_a_shell_at_its_prompt_or_a_program_that_ended_is_safe_to_end() {
    let state = |status, reason: &str| State {
        name: "zsh".into(),
        worktree: None,
        status,
        reason: reason.into(),
        control: Control::default(),
        pid: None,
    };
    assert!(!busy(&state(Status::Idle, "prompt")));
    assert!(!busy(&state(Status::Exited, "exited 0")));
    assert!(busy(&state(Status::Running, "command running")));
    assert!(busy(&state(Status::NeedsInput, "bell")));
    assert!(busy(&state(Status::Unknown, "started")));
    assert!(busy(&state(Status::Idle, "quiet")));
}

#[gpui_kit::test]
fn closing_an_idle_terminal_asks_nothing_and_it_opens_again_as_a_new_shell(
    cx: &mut TestAppContext,
) {
    let root = repo("close-idle-terminal", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let worktree = root.to_str().unwrap();
    let id = tab(&shell, &record("zsh", worktree, Status::Idle, "prompt"), cx);

    close_shown(window, &shell, cx);
    assert!(!cx.has_pending_prompt());
    assert!(!open(&shell, id, cx));
    shell.read_with(cx, |shell, _| {
        assert!(
            matches!(shell.terminals.sources.get(&id), Some(Source::Open(_))),
            "⌘⇧T has no terminal to attach to, so it starts a shell"
        );
        assert_eq!(shell.terminals.tab_of(&Handle("zsh".into())), None);
    });
}

#[gpui_kit::test]
fn closing_a_busy_terminal_asks_and_cancelling_keeps_it(cx: &mut TestAppContext) {
    let root = repo("close-busy-terminal", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let worktree = root.to_str().unwrap();
    let record = record("server", worktree, Status::Running, "command running");
    let id = tab(&shell, &record, cx);

    close_shown(window, &shell, cx);
    let (question, detail) = cx.pending_prompt().expect("the person is asked");
    assert_eq!(question, "Close server?");
    assert!(detail.contains("server: "), "{detail}");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(open(&shell, id, cx), "cancelling keeps the tab");
    shell.read_with(cx, |shell, _| {
        assert_eq!(shell.terminals.tab_of(&record.handle), Some(id));
    });

    close_shown(window, &shell, cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Close");
    cx.run_until_parked();
    assert!(!open(&shell, id, cx), "agreeing closes it");
    shell.read_with(cx, |shell, _| {
        assert_eq!(shell.terminals.tab_of(&record.handle), None);
    });
}

#[gpui_kit::test]
fn closing_a_terminals_tab_lets_go_of_its_view_at_once(cx: &mut TestAppContext) {
    let (window, shell, root) = shell_on("close-terminal-view", Layout::Orca, cx);
    terminal_shown(window, &shell, &root, cx);
    let id = shell.read_with(cx, |shell, _| {
        shell.workspace.terminal_tabs().next().unwrap().1
    });

    close_shown(window, &shell, cx);
    assert!(!cx.has_pending_prompt());
    draw(window, cx);
    shell.read_with(cx, |shell, _| {
        assert!(shell.terminals.view(id).is_none(), "its view is let go");
        assert!(
            shell.terminals.sources.contains_key(&id),
            "its terminal stays known, for ⌘⇧T"
        );
    });
}
