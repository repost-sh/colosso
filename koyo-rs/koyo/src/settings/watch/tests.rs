//! The watch on a real file, on a thread of its own as the app runs it: an edit made in place,
//! a save that renames a new file over it, and the file written again after it was deleted,
//! each reach the app once they settle.

use std::sync::mpsc;
use std::time::Duration;

use super::watch;
use crate::blocking;
use crate::test_support::scratch;

fn delay(ms: u64) -> String {
    format!(r#"{{ "key_repeat": {{ "delay_ms": {ms} }} }}"#)
}

#[test]
fn an_edit_made_outside_reaches_the_app_whether_written_in_place_or_renamed_over() {
    let path = scratch("settings-watch").join("settings.json");
    std::fs::write(&path, delay(90)).unwrap();
    let seen = std::fs::metadata(&path).unwrap().modified().ok();
    let (tell, told) = mpsc::channel();
    let watched = path.clone();
    blocking::thread("settings watch test", move |may| {
        let _ = watch(&watched, seen, may, |looked| tell.send(looked).is_ok());
    })
    .unwrap();
    let next = || {
        let looked = told
            .recv_timeout(Duration::from_secs(5))
            .expect("the edit reached the app");
        looked.read.unwrap().key_repeat.delay_ms
    };

    std::fs::write(&path, delay(60)).unwrap();
    assert_eq!(next(), 60, "written in place");
    let renamed = path.with_extension("json.tmp");
    std::fs::write(&renamed, delay(45)).unwrap();
    std::fs::rename(&renamed, &path).unwrap();
    assert_eq!(next(), 45, "renamed over");
    std::fs::write(&path, delay(30)).unwrap();
    assert_eq!(next(), 30, "written in place after the rename");
    std::fs::remove_file(&path).unwrap();
    let gone = told.recv_timeout(Duration::from_secs(5)).expect("the file going reached the app");
    assert!(gone.read.is_err(), "the file gone reads as nothing");
    std::fs::write(&path, delay(20)).unwrap();
    assert_eq!(next(), 20, "written again after it was gone");

    drop(told);
    std::fs::write(&path, delay(15)).unwrap();
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
