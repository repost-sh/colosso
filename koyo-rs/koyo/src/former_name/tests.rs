use std::cell::Cell;
use std::fs;
use std::io;

use super::{answers, move_data_folder, move_log_folder, move_newest};
use crate::test_support::scratch;

#[test]
fn colossos_data_moves_once_its_daemon_has_stopped() {
    let root = scratch("former-data");
    let former = root.join("dev.colosso.desktop");
    let data = root.join("codes.koyo.desktop");
    fs::create_dir_all(former.join("term")).unwrap();
    fs::create_dir_all(former.join("control")).unwrap();
    fs::write(former.join("settings.json"), "{}").unwrap();
    fs::write(former.join("control/colosso-control.sock"), "").unwrap();
    let stopped = Cell::new(None);
    let moved = move_data_folder(&former, &data, |dir| {
        stopped.set(Some(dir.to_path_buf()));
        assert!(dir.join("term").is_dir());
        Ok(())
    });
    assert!(moved.unwrap());
    assert_eq!(stopped.take(), Some(former.clone()));
    assert!(!former.exists());
    assert_eq!(
        fs::read_to_string(data.join("settings.json")).unwrap(),
        "{}"
    );
    assert!(data.join("term").is_dir());
    assert!(!data.join("control/colosso-control.sock").exists());
}

#[test]
fn nothing_moves_over_koyos_own_folder_or_from_a_daemon_that_wont_stop() {
    let root = scratch("former-data-kept");
    let former = root.join("dev.colosso.desktop");
    let data = root.join("codes.koyo.desktop");
    fs::create_dir_all(&former).unwrap();
    let refuse = |_: &_| Err(io::Error::other("still running"));
    assert!(move_data_folder(&former, &data, refuse).is_err());
    assert!(former.is_dir() && !data.exists());

    fs::create_dir_all(&data).unwrap();
    let moved = move_data_folder(&former, &data, |_| panic!("nothing to stop"));
    assert!(!moved.unwrap());
    assert!(former.is_dir());

    let missing = root.join("missing");
    assert!(!move_data_folder(&missing, &root.join("other"), |_| Ok(())).unwrap());
}

#[test]
fn the_koyo_dev_folder_moves_and_colossos_beside_it_stays() {
    let base = scratch("former-koyo-dev");
    let koyo = base.join("dev.koyo.desktop");
    let colosso = base.join("dev.colosso.desktop");
    fs::create_dir_all(koyo.join("agents")).unwrap();
    fs::write(koyo.join("settings.json"), "koyo").unwrap();
    fs::create_dir_all(&colosso).unwrap();
    let moved = move_newest(
        &base,
        |_| false,
        |dir| {
            assert_eq!(dir, koyo, "what runs in the koyo.dev folder is stopped");
            Ok(())
        },
    );
    assert_eq!(moved.unwrap(), Some(koyo.clone()));
    let data = base.join("codes.koyo.desktop");
    assert_eq!(
        fs::read_to_string(data.join("settings.json")).unwrap(),
        "koyo"
    );
    assert!(data.join("agents").is_dir());
    assert!(!koyo.exists());
    assert!(
        colosso.is_dir(),
        "an older folder than Koyo's is left as it was"
    );
    let again = move_newest(&base, |_| panic!("Koyo has its folder"), |_| Ok(()));
    assert_eq!(again.unwrap(), None);
}

#[test]
fn colossos_folder_moves_to_the_koyo_codes_one_when_its_the_only_one() {
    let base = scratch("former-colosso-only");
    fs::create_dir_all(base.join("dev.colosso.desktop")).unwrap();
    let moved = move_newest(&base, |_| panic!("no Koyo folder to ask of"), |_| Ok(()));
    assert_eq!(moved.unwrap(), Some(base.join("dev.colosso.desktop")));
    assert!(base.join("codes.koyo.desktop").is_dir());
}

#[test]
fn an_older_koyo_running_on_its_folder_keeps_it_there() {
    let base = scratch("former-koyo-running");
    let koyo = base.join("dev.koyo.desktop");
    fs::create_dir_all(&koyo).unwrap();
    let moved = move_newest(&base, |dir| dir == koyo, |_| panic!("nothing is stopped"));
    assert_eq!(moved.unwrap(), None);
    assert!(koyo.is_dir());
    assert!(!base.join("codes.koyo.desktop").exists());
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "what holds the socket a moment longer is another process, outside any executor"
)]
fn a_koyo_is_running_while_its_socket_answers() {
    let root = scratch("former-answers");
    let socket = root.join("control.sock");
    assert!(!answers(&socket), "no socket");
    let listener = koyo_daemon::bind(&socket).unwrap();
    assert!(answers(&socket));
    drop(listener);
    let let_go = (0..200).any(|_| {
        let quiet = !answers(&socket);
        if !quiet {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        quiet
    });
    assert!(let_go, "a socket nobody listens on");
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn colossos_logs_move_under_koyos_names() {
    let root = scratch("former-logs");
    let former = root.join("Colosso");
    let logs = root.join("Logs/Koyo");
    fs::create_dir_all(former.join("language-servers")).unwrap();
    fs::write(former.join("Colosso.log"), "now").unwrap();
    fs::write(former.join("Colosso.log.old"), "before").unwrap();
    move_log_folder(&former, &logs).unwrap();
    assert!(!former.exists());
    assert_eq!(fs::read_to_string(logs.join("Koyo.log")).unwrap(), "now");
    assert_eq!(
        fs::read_to_string(logs.join("Koyo.log.old")).unwrap(),
        "before"
    );
    assert!(logs.join("language-servers").is_dir());
}
