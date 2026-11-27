use gpui_kit::{AnyWindowHandle, TestAppContext};
use koyo_agent::protocol::ids::{ThreadId, Time};
use koyo_agent::protocol::model::{Beside, Status, ThreadSummary};
use koyo_agent::protocol::requests::RequestKind;

use super::{HEIGHT, Next, OnQuit, QuitDialog, WIDTH, next, works};
use crate::files::At;
use crate::files::tests::{checkout, editing, shell_showing, typed};
use crate::test_support::{press, repo, shell_with};
use crate::updates::testing::koyo;
use crate::updates::{self, Shown};
use crate::{files, modal, settings};

#[test]
fn nothing_running_quits_and_a_choice_made_is_kept_to() {
    assert_eq!(next(OnQuit::Ask, 0), Next::Quit, "nothing to ask about");
    assert_eq!(next(OnQuit::Ask, 1), Next::Ask);
    assert_eq!(next(OnQuit::Keep, 2), Next::Quit);
    assert_eq!(next(OnQuit::End, 2), Next::End);
    assert_eq!(next(OnQuit::End, 0), Next::End);
}

/// The dialog for terminals `running` and chats `chats`, as it says what runs.
fn saying(running: &[&str], chats: &[&str]) -> String {
    let named = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect();
    let dialog = QuitDialog {
        running: named(running),
        chats: named(chats),
        remember: false,
        end: None,
    };
    dialog.what_runs().to_string()
}

#[test]
fn the_dialog_names_the_terminals_and_the_chats_still_running() {
    assert_eq!(saying(&["zsh"], &[]), "A terminal is still running: zsh.");
    assert_eq!(
        saying(&[], &["Fix the build"]),
        "A chat is still running: Fix the build."
    );
    assert_eq!(
        saying(&[], &["Docs", "Fix"]),
        "2 chats are still running: Docs, Fix."
    );
    assert_eq!(
        saying(&["server", "zsh"], &["Fix the build"]),
        "2 terminals and 1 chat are still running: server, zsh, Fix the build."
    );
    assert_eq!(
        saying(&["zsh"], &["Docs", "Fix"]),
        "1 terminal and 2 chats are still running: zsh, Docs, Fix."
    );
    assert_eq!(
        saying(&[], &["Watch the build."]),
        "A chat is still running: Watch the build."
    );
}

fn chat(status: Status, needs: Option<RequestKind>, tasks: u64) -> ThreadSummary {
    ThreadSummary {
        id: ThreadId("0001".into()),
        cwd: "/work".into(),
        title: "A chat".into(),
        provider: "claude".into(),
        model: None,
        effort: None,
        fast: false,
        permission: None,
        plan: false,
        status,
        needs,
        beside: Beside {
            subagents: 0,
            tasks,
        },
        queued: 0,
        unread: false,
        tab_open: true,
        updated_at: Time("2026-11-07T09:00:00.000Z".into()),
        said: None,
    }
}

#[test]
fn a_chat_at_work_runs_a_turn_waits_for_the_person_or_runs_tasks_in_the_background() {
    assert!(works(&chat(Status::Working, None, 0)), "a turn running");
    assert!(
        works(&chat(Status::Idle, Some(RequestKind::Approval), 0)),
        "waiting on you"
    );
    assert!(
        works(&chat(Status::Idle, None, 1)),
        "a task in the background"
    );
    assert!(!works(&chat(Status::Idle, None, 0)), "idle");
    assert!(
        !works(&chat(Status::Failed, None, 0)),
        "failed, with nothing running"
    );
}

fn opened(remember: bool, cx: &mut TestAppContext) -> AnyWindowHandle {
    let root = repo(&format!("quit-{remember}"), &[("README.md", "hi\n")]);
    let (window, _shell) = shell_with(&[&root], cx);
    window
        .update(cx, |_, window, cx| {
            let dialog = QuitDialog {
                running: vec!["server".into(), "zsh".into()],
                chats: Vec::new(),
                remember,
                end: Some(Box::new(|_| {})),
            };
            modal::open(WIDTH, HEIGHT, window, cx, |_, _| dialog);
        })
        .unwrap();
    cx.run_until_parked();
    window
}

#[gpui_kit::test]
fn a_choice_is_kept_for_good_only_when_asked_to(cx: &mut TestAppContext) {
    opened(false, cx);
    cx.update(|cx| {
        let dialog = modal::current::<QuitDialog>(cx).expect("the dialog is open");
        assert_eq!(
            dialog.read(cx).what_runs().as_ref(),
            "2 terminals are still running: server, zsh."
        );
        dialog.update(cx, QuitDialog::end);
    });
    cx.update(|cx| assert_eq!(settings::get(cx).on_quit, OnQuit::Ask));

    opened(true, cx);
    cx.update(|cx| {
        let dialog = modal::current::<QuitDialog>(cx).expect("the dialog is open");
        dialog.update(cx, QuitDialog::end);
    });
    cx.update(|cx| assert_eq!(settings::get(cx).on_quit, OnQuit::End));
}

#[gpui_kit::test]
fn the_settings_menu_changes_the_choice(cx: &mut TestAppContext) {
    let root = repo("quit-settings", &[("README.md", "hi\n")]);
    let (window, _shell) = shell_with(&[&root], cx);
    cx.update(|cx| settings::update(cx, |settings| settings.on_quit = OnQuit::End));
    cx.dispatch_action(window, super::SetOnQuit(OnQuit::Ask));
    cx.update(|cx| assert_eq!(settings::get(cx).on_quit, OnQuit::Ask));
}

/// What the update's one click is labelled, or none where it offers no click.
fn offers(cx: &mut TestAppContext) -> Option<&'static str> {
    cx.read(|cx| match updates::shown(cx) {
        Shown::Offer { act, .. } => Some(act),
        Shown::Quiet(_) | Shown::Working { .. } => None,
    })
}

#[gpui_kit::test]
fn a_restart_into_an_update_asks_about_unsaved_edits_and_called_off_hands_nothing_over(cx: &mut TestAppContext) {
    let root = checkout("quit-restart-unsaved");
    let (window, shell, _) = shell_showing(&root, cx);
    let (_, editor, _) = editing(window, &shell, "src/lib.rs", At::Bytes(0..0), cx);
    typed(window, &editor, "// lib\n", cx);
    let koyo = koyo("quit-restart-unsaved-koyo", true, cx);

    koyo.stage(cx);

    assert!(cx.read(files::asking), "a restart with an unsaved edit didn't ask about it");
    assert_eq!(koyo.restarts.get(), 1);
    press(cx.read(modal::window).expect("the question's window"), "escape", cx);

    assert!(!cx.read(files::asking));
    assert_eq!(offers(cx), Some("Restart to Update"), "a restart called off isn't staged any more");
    assert!(!koyo.told.exists(), "a restart called off handed the release over");

    cx.update(updates::act);
    cx.run_until_parked();
    assert!(cx.read(files::asking), "a second restart didn't ask again");
    assert_eq!(offers(cx), Some("Restart to Update"));
}

#[gpui_kit::test]
fn a_restart_into_an_update_with_nothing_unsaved_hands_the_release_over_to_reopen(cx: &mut TestAppContext) {
    let root = checkout("quit-restart-saved");
    let _shown = shell_showing(&root, cx);
    let koyo = koyo("quit-restart-saved-koyo", true, cx);
    koyo.publish(cx);

    cx.update(updates::act);
    let told = koyo.handed_over(cx);

    assert!(!cx.read(files::asking));
    assert!(told.starts_with("install-update\n--attempt\n"), "{told}");
    assert!(told.ends_with("\n--reopen\n"), "the installer wasn't told to open Koyo again: {told}");
    assert_eq!(offers(cx), None, "a release handed over still offers a click");
    assert_eq!(cx.read(updates::shown).said().line.as_ref(), "Restarting into Koyo 9.0.0.");
}
