//! The watch on a real file, on a thread of its own as the app runs it: an edit made in place,
//! a save that renames a new file over it, and the file written again after it was deleted,
//! each reach the app once they settle. A file that can't be opened has it wait, a save renamed
//! over the file as its watch began is watched in its place, and a watch that failed starts
//! again.

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use super::{watch, watch_opening};
use crate::blocking;
use crate::settings::file::Looked;
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
        watch(&watched, seen, may, |looked| tell.send(looked).is_ok());
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

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).unwrap().modified().ok()
}

fn set_mode(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// The delay the next edit to reach the app sets.
fn next_delay(told: &mpsc::Receiver<Looked>, what: &str) -> u64 {
    let looked = told.recv_timeout(Duration::from_secs(5));
    let looked = looked.unwrap_or_else(|error| panic!("{what} didn't reach the app: {error}"));
    looked.read.unwrap().key_repeat.delay_ms
}

/// The CPU time `thread` has used so far, as the kernel counts it.
fn cpu_time(thread: &JoinHandle<()>) -> Duration {
    use std::os::unix::thread::JoinHandleExt as _;
    // SAFETY: a thread_basic_info is integers alone, for which zero is a value.
    let mut info: libc::thread_basic_info = unsafe { std::mem::zeroed() };
    let mut count = libc::THREAD_BASIC_INFO_COUNT;
    let flavor = u32::try_from(libc::THREAD_BASIC_INFO).unwrap();
    // SAFETY: the port of a thread that hasn't been joined, and an out buffer of the size its
    // flavor has, which `count` says.
    let kr = unsafe {
        let port = libc::pthread_mach_thread_np(thread.as_pthread_t());
        libc::thread_info(port, flavor, (&raw mut info).cast(), &raw mut count)
    };
    assert_eq!(kr, libc::KERN_SUCCESS, "thread_info");
    let time = |time: libc::time_value_t| {
        let seconds = u64::try_from(time.seconds).unwrap();
        let micros = u64::try_from(time.microseconds).unwrap();
        Duration::from_secs(seconds) + Duration::from_micros(micros)
    };
    time(info.user_time) + time(info.system_time)
}

#[test]
fn a_file_that_cant_be_opened_has_the_watch_wait_rather_than_spin() {
    let path = scratch("settings-watch-unreadable").join("settings.json");
    std::fs::write(&path, delay(90)).unwrap();
    set_mode(&path, 0o000);
    let seen = modified(&path);
    let (tell, told) = mpsc::channel();
    let watched = path.clone();
    let watching = std::thread::spawn(move || {
        blocking::in_test(|may| {
            watch(&watched, seen, may, |looked| tell.send(looked).is_ok());
        });
    });

    let before = cpu_time(&watching);
    let looked = told.recv_timeout(Duration::from_millis(500));
    let used = cpu_time(&watching).saturating_sub(before);

    set_mode(&path, 0o600);
    drop(told);
    std::fs::remove_file(&path).unwrap();
    assert!(looked.is_err(), "nothing changed, yet the app was told");
    assert!(used < Duration::from_millis(100), "the watch ran {used:?} of the 500 ms");
}

#[test]
fn a_save_renamed_over_the_file_as_its_watch_began_is_watched_in_its_place() {
    let path = scratch("settings-watch-race").join("settings.json");
    std::fs::write(&path, delay(90)).unwrap();
    let seen = modified(&path);
    let mut first = true;
    let open = move |opening: &Path| {
        let file = std::fs::File::open(opening);
        if std::mem::take(&mut first) {
            let renamed = opening.with_extension("json.tmp");
            std::fs::write(&renamed, delay(45)).unwrap();
            std::fs::rename(&renamed, opening).unwrap();
        }
        file
    };
    let (tell, told) = mpsc::channel();
    let watched = path.clone();
    blocking::thread("settings watch test", move |may| {
        let tell = |looked| tell.send(looked).is_ok();
        watch_opening(&watched, seen, may, tell, open);
    })
    .unwrap();

    assert_eq!(next_delay(&told, "the save renamed over"), 45);
    std::fs::write(&path, delay(30)).unwrap();
    assert_eq!(next_delay(&told, "the edit after it"), 30);

    drop(told);
    std::fs::write(&path, delay(15)).unwrap();
}

#[test]
fn a_watch_that_failed_starts_again_and_takes_up_the_next_edit() {
    let folder = scratch("settings-watch-again");
    let path = folder.join("settings.json");
    set_mode(&folder, 0o000);
    let (tell, told) = mpsc::channel();
    let watched = path.clone();
    blocking::thread("settings watch test", move |may| {
        watch(&watched, None, may, |looked| tell.send(looked).is_ok());
    })
    .unwrap();
    let early = told.recv_timeout(Duration::from_millis(300));
    set_mode(&folder, 0o755);
    assert!(early.is_err(), "the folder couldn't be read, yet the app was told");

    std::fs::write(&path, delay(25)).unwrap();
    assert_eq!(next_delay(&told, "the edit once the folder could be read"), 25);

    drop(told);
    std::fs::remove_file(&path).unwrap();
}
