//! What the host hears of Koyo's settings for the agents.

use gpui_kit::TestAppContext;
use koyo_agent::protocol::methods::Config;
use koyo_agent::protocol::model::Mode;

use super::chats_of;
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
    assert_eq!(told(cx), Some(Config { claude_path: None, default_mode: None }));

    cx.update(|cx| settings::update(cx, |settings| settings.chat_mode = Some(Mode::Ask)));
    assert_eq!(told(cx).and_then(|config| config.default_mode), Some(Mode::Ask));

    cx.update(|cx| settings::update(cx, |settings| settings.chat_mode = None));
}
