//! What the host hears of Koyo's settings for the agents.

use gpui_kit::TestAppContext;
use koyo_agent::protocol::methods::Config;
use koyo_agent::protocol::model::Mode;

use super::chats_of;
use crate::settings;
use crate::test_support::{repo, shell_with};

#[gpui_kit::test]
fn the_client_keeps_the_settings_from_the_start_and_as_they_change(cx: &mut TestAppContext) {
    let root = repo("config", &[("README.md", "hi\n")]);
    let (_, shell) = shell_with(&[&root], cx);
    let chats = chats_of(&shell, cx);
    let told = |cx: &mut TestAppContext| chats.read_with(cx, |chats, _| chats.told_config.clone());

    // Kept before any host is reached, for every connection to tell it first. Koyo tools are on
    // by default: chats' sessions start this executable as their server.
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
}
