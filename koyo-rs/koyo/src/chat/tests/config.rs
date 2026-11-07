//! What the host hears of Koyo's settings for the agents.

use gpui_kit::TestAppContext;
use koyo_agent::protocol::methods::Config;
use koyo_agent::protocol::model::Mode;

use super::chats_of;
use crate::chat::Connection;
use crate::settings;
use crate::test_support::{repo, shell_with};

#[gpui_kit::test]
fn the_host_hears_the_settings_once_connected_and_when_they_change(cx: &mut TestAppContext) {
    let root = repo("config", &[("README.md", "hi\n")]);
    let (_, shell) = shell_with(&[&root], cx);
    let chats = chats_of(&shell, cx);
    let told = |cx: &mut TestAppContext| chats.read_with(cx, |chats, _| chats.told_config.clone());

    assert_eq!(told(cx), None, "no host to tell yet");

    chats.update(cx, |chats, cx| chats.listed(Ok(Vec::new()), cx));
    // Koyo tools are on by default: chats' sessions start this executable as their server.
    let this = std::env::current_exe().unwrap();
    assert_eq!(
        told(cx),
        Some(Config {
            claude_path: None,
            default_mode: None,
            koyo_tools: Some(this.to_string_lossy().into_owned()),
        })
    );

    cx.update(|cx| settings::update(cx, |settings| settings.chat_mode = Some(Mode::Ask)));
    assert_eq!(
        told(cx).and_then(|config| config.default_mode),
        Some(Mode::Ask)
    );

    cx.update(|cx| settings::update(cx, |settings| settings.koyo_tools = false));
    assert_eq!(told(cx).and_then(|config| config.koyo_tools), None);

    cx.update(|cx| {
        settings::update(cx, |settings| {
            settings.chat_mode = None;
            settings.koyo_tools = true;
        });
    });
}

#[gpui_kit::test]
fn the_settings_go_to_a_host_being_reached_and_to_no_other(cx: &mut TestAppContext) {
    let root = repo("config-first", &[("README.md", "hi\n")]);
    let (_, shell) = shell_with(&[&root], cx);
    let chats = chats_of(&shell, cx);
    let told_when = |connection: Connection, cx: &mut TestAppContext| {
        chats.update(cx, |chats, cx| {
            chats.connection = connection;
            chats.told_config = None;
            chats.tell_config(cx);
            chats.told_config.clone()
        })
    };
    // No chat needs a host yet, or the host is another build's, or none can be reached.
    assert_eq!(told_when(Connection::Idle, cx), None);
    assert_eq!(told_when(Connection::OtherBuild, cx), None);
    assert_eq!(told_when(Connection::Failed("gone".into()), cx), None);
    // Being reached, as `reach` leaves it before a chat's first request: nothing listed yet.
    let told = told_when(Connection::Connecting, cx);
    assert!(told.is_some_and(|config| config.koyo_tools.is_some()));
}
