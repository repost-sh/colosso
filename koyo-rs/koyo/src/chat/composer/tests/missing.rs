//! What a chat says when Claude Code isn't there.

use gpui_kit::TestAppContext;

use super::chat;

#[gpui_kit::test]
fn a_chat_says_so_only_while_claude_code_is_missing(cx: &mut TestAppContext) {
    let chat = chat("chat-claude-missing", cx);
    let says = |cx: &TestAppContext| {
        chat.composer.read_with(cx, |composer, cx| {
            *composer.chats.read(cx).claude_missing.read(cx)
        })
    };
    assert!(!says(cx));

    let held = chat.composer.read_with(cx, |composer, cx| composer.chats.read(cx).claude_missing.clone());
    held.update(cx, |held, cx| {
        *held = true;
        cx.notify();
    });
    assert!(says(cx));
    cx.run_until_parked();
}
