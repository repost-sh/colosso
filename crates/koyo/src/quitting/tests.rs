use gpui_kit::TestAppContext;

use super::{HEIGHT, Next, OnQuit, QuitDialog, WIDTH, next};
use crate::test_support::{repo, shell_with};
use crate::{modal, settings};

#[test]
fn nothing_running_quits_and_a_choice_made_is_kept_to() {
    assert_eq!(next(OnQuit::Ask, 0), Next::Quit, "nothing to ask about");
    assert_eq!(next(OnQuit::Ask, 1), Next::Ask);
    assert_eq!(next(OnQuit::Keep, 2), Next::Quit);
    assert_eq!(next(OnQuit::End, 2), Next::End);
    // An ended terminal it keeps could come back, and the daemon would stay.
    assert_eq!(next(OnQuit::End, 0), Next::End);
}

fn opened(remember: bool, cx: &mut TestAppContext) -> gpui_kit::AnyWindowHandle {
    let root = repo(&format!("quit-{remember}"), &[("README.md", "hi\n")]);
    let (window, _shell) = shell_with(&[&root], cx);
    window
        .update(cx, |_, window, cx| {
            let dialog = QuitDialog {
                running: vec!["server".into(), "zsh".into()],
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
