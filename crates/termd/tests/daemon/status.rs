//! Shell integration end to end: a real zsh with Ghostty's scripts, its marks read as status
//! (`docs/terminals.md` 5.7).

use std::collections::{BTreeMap, BTreeSet};

use colosso_term_control::{
    ClientKind, IfExists, Input, Message, OpenRequest, RequestBody, ResponseBody, Size, Status,
    TermDir, TerminalRecord,
};

use super::{Client, StopOnDrop, connect, test_dir, wait_until};

/// zsh with Ghostty's integration marks its prompts and commands, so the status reads the
/// prompt, then the command running, then the prompt again with the command's exit code.
#[test]
fn an_integrated_zsh_reports_prompts_commands_and_exit_codes() {
    let dir = test_dir("status");
    let _stop = StopOnDrop(dir.clone());
    let client = connect(&dir, ClientKind::App);
    let terminal = open_zsh(&client, &dir);
    let handle = terminal.handle;
    let (index, _) = client.attach(&handle, None);
    let record = |client: &Client| {
        (client.list().into_iter())
            .find(|summary| summary.record.handle == handle)
            .unwrap()
            .record
    };
    let at_prompt =
        |record: &TerminalRecord| record.status == Status::Idle && record.status_reason == "prompt";
    wait_until("the prompt", || at_prompt(&record(&client)));

    let typed = Input {
        index,
        bytes: "sleep 1; false\r".into(),
    };
    client.connection.send(&Message::Input(typed)).unwrap();
    wait_until("the command to run", || {
        let record = record(&client);
        record.status == Status::Running && record.status_reason == "command running"
    });
    wait_until("the prompt after it", || {
        let record = record(&client);
        at_prompt(&record) && record.last_exit == Some(1)
    });
    client.connection.request(RequestBody::Shutdown).unwrap();
    wait_until("the socket to go", || !dir.socket().exists());
}

/// Opens an interactive zsh with the integration, in a home of its own so the person's own
/// zsh files stay out of the test, its prompt `% `.
pub(super) fn open_zsh(client: &Client, dir: &TermDir) -> TerminalRecord {
    let home = dir.root().parent().unwrap().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join(".zshrc"), "PS1='%# '\n").unwrap();
    let home_text = home.to_string_lossy().into_owned();
    let request = OpenRequest {
        worktree: None,
        name: "zsh".into(),
        command: vec!["/bin/zsh".into(), "-i".into()],
        cwd: Some(home),
        env: BTreeMap::from([
            ("HOME".to_owned(), home_text.clone()),
            ("ZDOTDIR".to_owned(), home_text),
        ]),
        size: Some(Size::cells(60, 12)),
        labels: BTreeSet::new(),
        if_exists: IfExists::Attach,
        respawn: false,
    };
    let ResponseBody::Opened { terminal, .. } = client
        .connection
        .request(RequestBody::Open(request))
        .unwrap()
    else {
        panic!("not opened");
    };
    *terminal
}
