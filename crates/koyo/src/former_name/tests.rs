use std::cell::Cell;
use std::fs;
use std::io;

use super::{move_data_folder, move_log_folder};
use crate::test_support::scratch;

#[test]
fn colossos_data_moves_once_its_daemon_has_stopped() {
    let root = scratch("former-data");
    let former = root.join("dev.colosso.desktop");
    let data = root.join("dev.koyo.desktop");
    fs::create_dir_all(former.join("term")).unwrap();
    fs::create_dir_all(former.join("control")).unwrap();
    fs::write(former.join("settings.json"), "{}").unwrap();
    fs::write(former.join("control/colosso-control.sock"), "").unwrap();
    let stopped = Cell::new(None);
    let moved = move_data_folder(&former, &data, |dir| {
        // Stopped while its folder is still where it runs.
        stopped.set(Some(dir.root().to_path_buf()));
        assert!(dir.root().is_dir());
        Ok(())
    });
    assert!(moved.unwrap());
    assert_eq!(stopped.take(), Some(former.join("term")));
    assert!(!former.exists());
    assert_eq!(fs::read_to_string(data.join("settings.json")).unwrap(), "{}");
    assert!(data.join("term").is_dir());
    assert!(!data.join("control/colosso-control.sock").exists());
}

#[test]
fn nothing_moves_over_koyos_own_folder_or_from_a_daemon_that_wont_stop() {
    let root = scratch("former-data-kept");
    let former = root.join("dev.colosso.desktop");
    let data = root.join("dev.koyo.desktop");
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
    assert_eq!(fs::read_to_string(logs.join("Koyo.log.old")).unwrap(), "before");
    assert!(logs.join("language-servers").is_dir());
}
