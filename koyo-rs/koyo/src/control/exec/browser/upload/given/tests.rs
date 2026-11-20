use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use koyo_app_control::ControlError;

use super::{Budget, MOST_BYTES, folder_for_upload, forget, forget_ended, given, read_into};
use crate::blocking;
use crate::test_support::scratch;

/// What giving `paths` from the checkout at `root` hands the page, copied into `into`.
fn give(root: &Path, into: &Path, paths: &[&str], folder: bool) -> Result<Vec<String>, ControlError> {
    let paths: Vec<String> = paths.iter().map(|path| (*path).to_owned()).collect();
    blocking::in_test(|may| given(root, &paths, (folder, Budget::whole()), into, may))
}

fn refused(root: &Path, into: &Path, paths: &[&str], folder: bool) -> String {
    give(root, into, paths, folder).unwrap_err().message
}

fn read(path: impl AsRef<Path>) -> String {
    String::from_utf8(std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn an_agent_gives_only_what_is_in_its_worktree() {
    let root = scratch("upload-paths");
    let outside = scratch("upload-paths-outside");
    let into = scratch("upload-paths-copies");
    std::fs::create_dir_all(root.join("shots")).unwrap();
    std::fs::write(root.join("shots/a.png"), b"png").unwrap();
    std::fs::write(outside.join("secret"), b"key").unwrap();
    std::os::unix::fs::symlink(outside.join("secret"), root.join("link")).unwrap();
    let absolute = root.join("shots/a.png").to_string_lossy().into_owned();
    let copies = give(&root, &into, &["shots/a.png", &absolute], false).unwrap();
    assert_eq!(copies.len(), 2);
    for copy in &copies {
        assert!(copy.starts_with(into.to_string_lossy().as_ref()), "{copy}");
        assert!(copy.ends_with("/a.png"), "under the name it had: {copy}");
        assert_eq!(read(copy), "png");
    }
    assert_ne!(copies[0], copies[1], "one folder each");
    assert!(refused(&root, &into, &["missing.png"], false).starts_with("Nothing is at"));
    let link = refused(&root, &into, &["link"], false);
    assert!(link.contains("is outside your worktree"), "a link out of it: {link}");
    assert!(refused(&root, &into, &["../upload-paths-outside/secret"], false).contains("outside"));
    let secret = outside.join("secret").to_string_lossy().into_owned();
    assert!(refused(&root, &into, &[&secret], false).contains("outside"));
    assert!(refused(&root, &into, &["shots"], false).ends_with("is a folder, not a file."));
    assert!(refused(&root, &into, &["shots/a.png"], true).ends_with("is no folder."));
}

#[test]
fn a_folder_is_given_as_a_copy_of_what_is_in_it() {
    let root = scratch("upload-folder");
    let into = scratch("upload-folder-copies");
    std::fs::create_dir_all(root.join("site/css")).unwrap();
    std::fs::write(root.join("site/index.html"), b"<p>").unwrap();
    std::fs::write(root.join("site/css/a.css"), b"p {}").unwrap();
    std::fs::write(root.join("logo.png"), b"png").unwrap();
    std::os::unix::fs::symlink(root.join("logo.png"), root.join("site/logo.png")).unwrap();
    std::os::unix::fs::symlink(root.join("site/css"), root.join("site/styles")).unwrap();
    std::os::unix::fs::symlink(root.join("gone"), root.join("site/nowhere")).unwrap();
    let copies = give(&root, &into, &["site"], true).unwrap();
    let copy = PathBuf::from(&copies[0]);
    assert!(copy.starts_with(&into) && copy.ends_with("site"), "{copy:?}");
    assert_eq!(read(copy.join("index.html")), "<p>");
    assert_eq!(read(copy.join("css/a.css")), "p {}");
    assert_eq!(read(copy.join("logo.png")), "png", "a link to a file of the worktree");
    assert!(!copy.join("styles").exists(), "a link to a folder is left out");
    assert!(!copy.join("nowhere").exists(), "and one that leads nowhere");
}

#[test]
fn a_folder_with_a_link_out_of_the_worktree_in_it_is_refused() {
    let root = scratch("upload-folder-link");
    let outside = scratch("upload-folder-link-outside");
    let into = scratch("upload-folder-link-copies");
    std::fs::create_dir_all(root.join("site/deep")).unwrap();
    std::fs::write(root.join("site/index.html"), b"<p>").unwrap();
    std::fs::create_dir_all(outside.join(".ssh")).unwrap();
    std::fs::write(outside.join(".ssh/id_ed25519"), b"key").unwrap();
    std::os::unix::fs::symlink(outside.join(".ssh"), root.join("site/deep/keys")).unwrap();
    let message = refused(&root, &into, &["site"], true);
    assert!(message.contains("outside your worktree"), "{message}");
    assert!(!into.exists(), "what was copied before goes");
    std::fs::remove_file(root.join("site/deep/keys")).unwrap();
    std::os::unix::fs::symlink(outside.join(".ssh/id_ed25519"), root.join("site/key")).unwrap();
    let message = refused(&root, &into, &["site"], true);
    assert!(message.contains("outside your worktree"), "a link to a file out of it: {message}");
}

#[test]
fn a_file_that_is_also_one_elsewhere_is_refused() {
    let root = scratch("upload-hard-link");
    let outside = scratch("upload-hard-link-outside");
    let into = scratch("upload-hard-link-copies");
    std::fs::write(outside.join("secret"), b"key").unwrap();
    std::fs::hard_link(outside.join("secret"), root.join("notes.txt")).unwrap();
    let message = refused(&root, &into, &["notes.txt"], false);
    assert!(message.contains("hard link"), "{message}");
}

#[test]
fn a_page_is_given_a_copy_of_what_was_checked_not_the_file_itself() {
    let root = scratch("upload-copied");
    let into = scratch("upload-copied-copies");
    std::fs::write(root.join("a.png"), b"checked").unwrap();
    let given = give(&root, &into, &["a.png"], false).unwrap();
    assert!(!given[0].starts_with(root.to_string_lossy().as_ref()), "{given:?}");
    std::fs::remove_file(root.join("a.png")).unwrap();
    std::os::unix::fs::symlink("/etc/hosts", root.join("a.png")).unwrap();
    assert_eq!(read(&given[0]), "checked");
}

/// A process that has ended: its id, which no process has until the system gives it again.
fn ended() -> u32 {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

#[test]
fn copies_are_this_runs_and_go_as_it_ends_and_an_ended_runs_go_too() {
    let data = scratch("upload-runs");
    let uploads = data.join("browser/uploads");
    let earlier = uploads.join(ended().to_string()).join("0");
    std::fs::create_dir_all(&earlier).unwrap();
    std::fs::write(earlier.join("a.png"), b"png").unwrap();
    let running = uploads.join(std::os::unix::process::parent_id().to_string()).join("0");
    std::fs::create_dir_all(&running).unwrap();
    let folder = blocking::in_test(|may| folder_for_upload(&data, may)).unwrap();
    let again = blocking::in_test(|may| folder_for_upload(&data, may)).unwrap();
    assert!(folder.is_dir() && again.is_dir() && folder != again);
    assert!(folder.starts_with(&uploads));
    blocking::in_test(|may| forget_ended(&uploads, may));
    assert!(!earlier.exists(), "an ended run's are gone, as this run's first upload has them go");
    assert!(folder.exists(), "this run's own stay");
    assert!(running.exists(), "a run still going keeps its own");
    blocking::in_test(|may| forget(&data, may));
    assert!(!folder.exists() && !again.exists(), "this run's go as it ends");
}

#[test]
fn an_uploads_folder_is_a_new_one_never_what_a_crashed_run_left() {
    let data = scratch("upload-fresh");
    let run = data.join("browser/uploads").join(std::process::id().to_string());
    for at in 0..64 {
        std::fs::create_dir_all(run.join(at.to_string())).unwrap();
        std::fs::write(run.join(at.to_string()).join("left.png"), b"old").unwrap();
    }
    let folder = blocking::in_test(|may| folder_for_upload(&data, may)).unwrap();
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0, "{folder:?} is new");
}

#[test]
fn a_pipe_is_refused_without_waiting_for_what_would_write_it() {
    let root = scratch("upload-fifo");
    let into = scratch("upload-fifo-copies");
    let made = std::process::Command::new("mkfifo").arg(root.join("pipe")).status().unwrap();
    assert!(made.success());
    let (tell, told) = std::sync::mpsc::channel();
    std::thread::spawn(move || tell.send(refused(&root, &into, &["pipe"], false)));
    let message = (told.recv_timeout(std::time::Duration::from_secs(10)))
        .expect("refused at once, not left waiting on the pipe");
    assert!(message.contains("is no regular file"), "{message}");
}

#[test]
fn a_file_on_the_copies_volume_is_cloned_whatever_it_weighs() {
    let root = scratch("upload-clone");
    let into = scratch("upload-clone-copies");
    std::fs::write(root.join("big.bin"), vec![7_u8; 4096]).unwrap();
    let paths = vec!["big.bin".to_owned()];
    let nothing_to_read = Budget { bytes: 0, entries: 1 };
    let copies = blocking::in_test(|may| given(&root, &paths, (false, nothing_to_read), &into, may))
        .expect("cloned, so nothing is read");
    assert_eq!(std::fs::read(&copies[0]).unwrap(), vec![7_u8; 4096]);
    let mode = std::fs::metadata(&copies[0]).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "the person's only");
}

#[test]
fn a_copy_that_cant_be_cloned_reads_no_more_than_an_upload_may() {
    let root = scratch("upload-read");
    std::fs::write(root.join("a.bin"), [1_u8; 20]).unwrap();
    let open = || std::fs::File::open(root.join("a.bin")).unwrap();
    let mut budget = Budget { bytes: 10, entries: 1 };
    let refused = blocking::in_test(|may| read_into(open(), &root.join("over"), "a.bin", &mut budget, may))
        .unwrap_err();
    assert!(refused.message.contains("past 2 GiB"), "{}", refused.message);
    let mut budget = Budget { bytes: 64, entries: 1 };
    blocking::in_test(|may| read_into(open(), &root.join("within"), "a.bin", &mut budget, may)).unwrap();
    assert_eq!((read(root.join("within")).len(), budget.bytes), (20, 44));
    assert_eq!(MOST_BYTES, 2 << 30);
}

#[test]
fn a_folder_of_more_files_than_an_upload_gives_is_refused() {
    let root = scratch("upload-many");
    let into = scratch("upload-many-copies");
    std::fs::create_dir_all(root.join("site")).unwrap();
    for at in 0..4 {
        std::fs::write(root.join(format!("site/{at}.txt")), b"x").unwrap();
    }
    let paths = vec!["site".to_owned()];
    let three = Budget { bytes: MOST_BYTES, entries: 3 };
    let refused = blocking::in_test(|may| given(&root, &paths, (true, three), &into, may)).unwrap_err();
    assert!(refused.message.contains("one file more than browser_upload gives"), "{}", refused.message);
    let four = Budget { bytes: MOST_BYTES, entries: 4 };
    blocking::in_test(|may| given(&root, &paths, (true, four), &into, may)).unwrap();
}
