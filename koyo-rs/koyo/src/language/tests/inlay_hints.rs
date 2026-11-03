//! Inlay hints: asked for the rows in view once a frame shows them, put at their bytes,
//! moved with the text as it's typed, asked again after the pause, taken out and put back
//! as the setting turns them off and on, and asked again when the server says they changed.

use gpui_kit::component::input::{Bias, Inlay};
use gpui_kit::{App, TestAppContext};
use serde_json::{Value, json};

use super::support::*;
use crate::language::State;
use crate::settings;
use crate::test_support::repo;

const MAIN: &str = "fn main() {\n    let x = greet(1);\n}\n";

#[gpui_kit::test]
async fn inlay_hints_show_what_the_server_infers_and_follow_the_text(cx: &mut TestAppContext) {
    let root = repo("language-inlay-hints", &[("src/main.rs", MAIN)]);
    let (x, x_end) = find(MAIN, "x");
    let (one, one_start) = find(MAIN, "1");
    let (launcher, _) = launcher(move |root, _| {
        let uri = uri(&root.join("src/main.rs"));
        let mut script = started_with(root, "src/main.rs", json!({"inlayHintProvider": true}));
        script.extend([
            // Asked for the rows in view, from the first.
            json!({"expect": {"method": "textDocument/inlayHint", "params": {
                "textDocument": {"uri": uri}, "range": {"start": at(0, 0)}}}}),
            json!({"reply": 1, "result": [
                {"position": x_end["end"], "label": ": u32", "kind": 1},
                {"position": one_start["start"], "label": [{"value": "n"}, {"value": ":"}],
                 "kind": 2, "paddingRight": true}]}),
            // A character typed before them.
            change(1, &json!({"start": at(1, 4), "end": at(1, 4)}), "y", &uri),
            json!({"expect": {"method": "textDocument/inlayHint"}}),
            json!({"reply": 2, "result": [{"position": at(1, 10), "label": ": u32", "kind": 1}]}),
            // The setting off and on again: asked once more.
            json!({"expect": {"method": "textDocument/inlayHint"}}),
            json!({"reply": 3, "result": [{"position": at(1, 10), "label": ": u32", "kind": 1}]}),
            // Typed on again; then it says they changed: answered, and asked again at the
            // next frame. Its answer takes an index.
            change(2, &json!({"start": at(1, 5), "end": at(1, 5)}), "y", &uri),
            json!({"send": {"id": "r1", "method": "workspace/inlayHint/refresh"}}),
            json!({"expect": {"id": "r1", "result": null}}),
            json!({"expect": {"method": "textDocument/inlayHint"}}),
            json!({"reply": 5, "result": []}),
        ]);
        Value::Array(script)
    });
    let shell = shell(&[&root], launcher, cx);
    let main = open(&shell, &root, "src/main.rs", cx);
    let server = server(&root, cx).unwrap();
    assert_eq!(settled(&server, cx).await, State::Ready);
    // Moved when it started and when it was first ready; the server's request moves it on.
    let refreshed = cx.read(|cx| server.read(cx).hints_refreshed);
    let editor = cx.read(|cx| editor(&main, cx)).unwrap();
    let inlays = |cx: &App| editor.read(cx).lsp().inlays().to_vec();
    let provided = |cx: &App| editor.read(cx).lsp().inlay_hint_provider.is_some();
    let inlay = |offset, label: &str, bias| Inlay {
        offset,
        label: label.into(),
        bias,
    };

    // A frame shows the rows; once typing and scrolling have paused, the server is asked.
    after_the_pause(shell.0, &server, &editor, cx, |cx| inlays(cx).len() == 2).await;
    assert_eq!(
        cx.read(inlays),
        [inlay(x.end, ": u32", Bias::Right), inlay(one.start, "n: ", Bias::Left)]
    );

    // A character typed before them moves both at once, before the server is asked again;
    // after the pause it is, and its answer for the new text replaces them.
    place_caret(shell.0, &editor, find(MAIN, "let").0.start, cx);
    cx.simulate_keystrokes(shell.0, "y->y");
    assert_eq!(
        cx.read(inlays),
        [inlay(x.end + 1, ": u32", Bias::Right), inlay(one.start + 1, "n: ", Bias::Left)]
    );
    after_the_pause(shell.0, &server, &editor, cx, |cx| inlays(cx).len() == 1).await;
    assert_eq!(cx.read(inlays), [inlay(x.end + 1, ": u32", Bias::Right)]);

    // Turned off in the settings, the editor's provider goes and the hints with it; turned
    // on again, it's back and the server is asked once more.
    cx.update(|cx| settings::update(cx, |settings| settings.inlay_hints = false));
    cx.run_until_parked();
    assert!(!cx.read(provided));
    assert!(cx.read(inlays).is_empty());
    cx.update(|cx| settings::update(cx, |settings| settings.inlay_hints = true));
    cx.run_until_parked();
    assert!(cx.read(provided));
    after_the_pause(shell.0, &server, &editor, cx, |cx| inlays(cx).len() == 1).await;

    // Typed on, the hint moves at once; the server says they changed, which drops a fetch
    // under way: asked again at the next frame, and its answer is empty.
    cx.simulate_keystrokes(shell.0, "y->y");
    assert_eq!(cx.read(inlays), [inlay(x.end + 2, ": u32", Bias::Right)]);
    let watched = vec![changes(&server, cx)];
    until(cx, watched, |cx| server.read(cx).hints_refreshed == refreshed + 1).await;
    after_the_pause(shell.0, &server, &editor, cx, |cx| inlays(cx).is_empty()).await;
    let _ = std::fs::remove_dir_all(&root);
}
