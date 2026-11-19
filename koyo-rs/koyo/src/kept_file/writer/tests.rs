//! The writes waiting as the app quits, with one file's write stuck on a disk that stalled: the
//! others land, and the wait for the stuck one ends.

use std::path::PathBuf;
use std::time::Instant;

use super::WAITS;
use super::stuck::Stuck;
use crate::blocking::in_test;
use crate::test_support::scratch;

#[test]
fn a_write_stuck_past_the_wait_holds_back_its_own_file_alone() {
    let folder = scratch("kept-file-stuck");
    let (stuck, other) = (folder.join("stuck.json"), folder.join("other.json"));
    let disk = Stuck::writing(&stuck, "1");
    disk.hand(&stuck, "2");
    disk.hand(&other, "3");

    let began = Instant::now();
    in_test(|may| disk.writers.settle(may));
    let waited = began.elapsed();

    let landed = |path: &PathBuf, json: &str| (path.clone(), json.to_owned());
    assert_eq!(disk.landed(), [landed(&other, "3")], "the other file's write landed");
    assert!(
        (WAITS..WAITS * 2).contains(&waited),
        "waited {waited:?} on the stuck write"
    );
    let after = disk.release();
    assert_eq!(
        after,
        [landed(&other, "3"), landed(&stuck, "1"), landed(&stuck, "2")],
        "the stuck file's writes land in order once it's free"
    );
    assert_eq!(std::fs::read_to_string(&stuck).unwrap(), "2");
}
