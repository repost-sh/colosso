//! The app's theme reaches the programs: a terminal opened after it's said starts with it in
//! `COLORFGBG`, and its engine answers a program asking whether the theme is light or dark.

use colosso_term_control::{ClientKind, RequestBody, Theme};

use super::{StopOnDrop, connect, test_dir, wait_until};

#[test]
fn a_program_is_told_the_apps_theme() {
    let dir = test_dir("theme");
    let _stop = StopOnDrop(dir.clone());
    let client = connect(&dir, ClientKind::App);
    let light = Theme {
        foreground: [20, 20, 22],
        background: [250, 250, 250],
        dark: false,
    };
    client
        .connection
        .request(RequestBody::Theme(light))
        .unwrap();
    // Nobody attaches, so the daemon's engine answers the query itself: nine bytes,
    // `ESC [ ? 9 9 7 ; 2 n` for light, printed without the escape.
    let script = [
        r#"echo "fgbg=$COLORFGBG""#,
        "stty -echo -icanon",
        r"printf '\033[?996n'",
        "read -r -k 9 reply",
        r#"print -r -- "scheme=${reply:1}""#,
    ]
    .join("; ");
    let cwd = dir.root().parent().unwrap().to_path_buf();
    let (terminal, _) = client.open("theme", &cwd, &["/bin/zsh", "-f", "-c", &script], false);
    let screen = || client.screen(&terminal.handle).lines.join("\n");
    wait_until("the program's answer", || screen().contains("scheme="));
    let shown = screen();
    assert!(shown.contains("fgbg=0;15"), "{shown}");
    assert!(shown.contains("scheme=[?997;2n"), "{shown}");
}
