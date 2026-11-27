//! What the updates' tests share: bundles and disk images made in a test's own folder and
//! signed ad hoc by the system's tools, as a release's are but for the identity, and a bucket
//! that is a folder. No network, no keychain: a requirement that names the identifier alone
//! stands in for the one a signing identity designates.

use std::cell::Cell;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::{Entity, TestAppContext};

use super::attempt::Attempt;
use super::feed::{Feed, Release};
use super::fetch::{AtMost, Fetch};
use super::installed::Installed;
use super::signature::Requirement;
use super::staged::Staged;
use super::{EVERY, RestartToUpdate, Shown, TheUpdates, Updates, act, released, shown};
use crate::blocking::{Blocking, in_test};
use crate::test_support::{scratch, write};

/// The identifier Koyo is signed under.
pub(super) const IDENTIFIER: &str = "codes.koyo.desktop";

/// Where the tests' feed is said to be: `Folder` serves its file's name.
pub(super) const FEED: &str = "https://updates.test/aarch64/feed.json";

/// What a test trusts: anything signed under Koyo's identifier.
pub(super) fn trust() -> Requirement {
    Requirement::text(&format!("identifier \"{IDENTIFIER}\""))
}

pub(super) fn release(version: &str, bytes: u64) -> Release {
    Release {
        version: version.parse().unwrap(),
        bytes,
        minimum_macos: "11.0".parse().unwrap(),
    }
}

/// How a bundle is made: what it says of itself, and the identifier it's signed under, if
/// it's signed.
#[derive(Clone, Copy)]
pub(super) struct Made {
    pub version: &'static str,
    pub minimum_macos: &'static str,
    pub identifier: Option<&'static str>,
}

impl Made {
    /// A release of Koyo, as the pipeline makes one.
    pub(super) fn release(version: &'static str) -> Made {
        Made {
            version,
            minimum_macos: "11.0",
            identifier: Some(IDENTIFIER),
        }
    }
}

pub(super) fn info_plist(made: &Made) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>koyo</string>
  <key>CFBundleIdentifier</key>
  <string>{IDENTIFIER}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>{}</string>
  <key>LSMinimumSystemVersion</key>
  <string>{}</string>
</dict>
</plist>
"#,
        made.version, made.minimum_macos
    )
}

fn run(tool: &str, args: &[&std::ffi::OsStr]) {
    let output = Command::new(tool).args(args).output().unwrap();
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{tool} {args:?}: {said}");
}

/// Signs what's at `path`, a bundle or an image, ad hoc under `identifier`.
pub(super) fn sign(path: &Path, identifier: &str) {
    let args = ["--force", "--sign", "-", "-i", identifier].map(std::ffi::OsStr::new);
    run("/usr/bin/codesign", &[&args[..], &[path.as_os_str()]].concat());
}

/// Makes a bundle at `at` as `made` says: what it says of itself, and an executable.
pub(super) fn bundle(at: &Path, made: &Made) {
    write(at, "Contents/Info.plist", info_plist(made).as_bytes());
    std::fs::create_dir_all(at.join("Contents/MacOS")).unwrap();
    std::fs::copy("/usr/bin/true", at.join("Contents/MacOS/koyo")).unwrap();
    if let Some(identifier) = made.identifier {
        sign(at, identifier);
    }
}

/// Makes a bundle at `at` as `made` says with a helper app nested in it, as the engine's
/// helpers are in a release: the helper is signed first, and the bundle's seal then holds it
/// by what it's signed as. The helper, which holds one resource, `words.txt`.
pub(super) fn bundle_with_helper(at: &Path, made: &Made) -> PathBuf {
    let helper = at.join("Contents/Frameworks/Koyo Helper.app");
    write(&helper, "Contents/Resources/words.txt", b"as signed");
    bundle(&helper, &Made { identifier: Some("codes.koyo.desktop.helper"), ..*made });
    bundle(at, made);
    helper
}

/// Makes a release's image at `image`: a disk image with `Koyo.app`, made as `made` says, at
/// its root, signed under `signed` if it's signed. `work` is a folder to build it in.
pub(super) fn image(work: &Path, image: &Path, made: &Made, signed: Option<&str>) {
    let root = work.join(format!("image-{}", made.version));
    bundle(&root.join("Koyo.app"), made);
    let args = ["create", "-quiet", "-fs", "HFS+", "-format", "UDZO", "-volname", "Koyo"];
    let args = args.map(std::ffi::OsStr::new);
    let named = [std::ffi::OsStr::new("-srcfolder"), root.as_os_str(), image.as_os_str()];
    run("/usr/bin/hdiutil", &[&args[..], &named[..]].concat());
    if let Some(identifier) = signed {
        sign(image, identifier);
    }
}

/// Publishes a release into `bucket` as the pipeline does: `Koyo-<version>.dmg`, signed under
/// Koyo's identifier, and a `feed.json` that names it. The release, as the feed says it.
pub(super) fn publish(bucket: &Path, made: &Made) -> Release {
    std::fs::create_dir_all(bucket).unwrap();
    let file = bucket.join(format!("Koyo-{}.dmg", made.version));
    image(bucket, &file, made, Some(IDENTIFIER));
    let bytes = std::fs::metadata(&file).unwrap().len();
    let feed = format!(
        r#"{{ "version": "{}", "minimum_macos": "{}", "bytes": {bytes} }}"#,
        made.version, made.minimum_macos
    );
    std::fs::write(bucket.join("feed.json"), feed).unwrap();
    Release {
        version: made.version.parse().unwrap(),
        bytes,
        minimum_macos: made.minimum_macos.parse().unwrap(),
    }
}

/// This copy of Koyo, for a test: a bundle at `<beside>/Koyo.app` that reads [`FEED`], trusts
/// what's signed under Koyo's identifier, and whose installer is the program at `exe`.
pub(super) fn installed(beside: &Path, exe: &Path) -> Installed {
    let bundle = beside.join("Koyo.app");
    self::bundle(&bundle, &Made::release(env!("CARGO_PKG_VERSION")));
    Installed {
        feed: Feed::at(FEED).unwrap(),
        bundle,
        exe: exe.to_owned(),
        trust: trust(),
        macos: "14.0".parse().unwrap(),
    }
}

/// A program at `<folder>/installer` that stands in for `koyo install-update`: it writes the
/// arguments it was started with, a line each, to `<folder>/told`, and ends. The path of the
/// program, and of what it writes.
pub(super) fn recorder(folder: &Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt as _;
    let (program, told) = (folder.join("installer"), folder.join("told"));
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{0}.part' && mv '{0}.part' '{0}'\n",
        told.display()
    );
    std::fs::write(&program, script).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    (program, told)
}

/// A bucket that is a folder: an address's last part names a file in it.
pub(super) struct Folder {
    pub at: PathBuf,
    /// Run once, when half of a stream has gone out: a test's way in mid-download.
    pub midway: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    /// What a stream waits for before anything of it goes out, until it's sent to or let go:
    /// a test's way to look while a download is under way.
    pub held: Mutex<Option<Receiver<()>>>,
}

impl Folder {
    pub(super) fn at(at: &Path) -> Folder {
        Folder {
            at: at.to_owned(),
            midway: Mutex::new(None),
            held: Mutex::new(None),
        }
    }

    fn file(&self, url: &str) -> Result<PathBuf, String> {
        let (_, name) = url.rsplit_once('/').ok_or("an address with no file")?;
        Ok(self.at.join(name))
    }
}

impl Fetch for Folder {
    fn read(&self, url: &str, most: u64, _: Blocking<'_>) -> Result<Vec<u8>, String> {
        let held = std::fs::read(self.file(url)?).map_err(|error| format!("404: {error}"))?;
        if held.len() as u64 > most {
            return Err("more came than was asked for".into());
        }
        Ok(held)
    }

    fn stream(
        &self,
        url: &str,
        most: u64,
        into: &mut dyn Write,
        _: Blocking<'_>,
    ) -> Result<(), String> {
        if let Some(held) = self.held.lock().unwrap().take() {
            let _ = held.recv();
        }
        let held = std::fs::read(self.file(url)?).map_err(|error| format!("404: {error}"))?;
        let (first, second) = held.split_at(held.len() / 2);
        let mut into = AtMost { into, left: most };
        let said = |error: io::Error| error.to_string();
        into.write_all(first).map_err(said)?;
        if let Some(midway) = self.midway.lock().unwrap().take() {
            midway();
        }
        into.write_all(second).map_err(said)
    }
}

/// Attaches `image` at `at`, as an install does while it copies the app out.
pub(super) fn attach(image: &Path, at: &Path) {
    std::fs::create_dir_all(at).unwrap();
    let args = ["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint"];
    let args = args.map(std::ffi::OsStr::new);
    run("/usr/bin/hdiutil", &[&args[..], &[at.as_os_str(), image.as_os_str()]].concat());
}

/// The attempt folders beside `bundle`.
pub(super) fn attempts(bundle: &Path) -> Vec<PathBuf> {
    let prefix = format!(".{}.update-", bundle.file_name().unwrap().to_string_lossy());
    let mut found: Vec<PathBuf> = std::fs::read_dir(bundle.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .map(|entry| entry.path())
        .collect();
    found.sort();
    found
}

/// Release `version`, staged in an attempt of its own to replace `installed`: a copy made as
/// a release's is, and checked.
pub(super) fn staged(installed: &Path, version: &'static str) -> Staged {
    in_test(|may| {
        let attempt = Attempt::begin(installed, may).unwrap();
        bundle(&attempt.staged(), &Made::release(version));
        let macos = "14.0".parse().unwrap();
        Staged::check(attempt, &release(version, 1), &trust(), macos, may).unwrap()
    })
}

/// Waits until `done`, for what a process or a thread of its own does outside the test.
pub(super) fn until(what: &str, mut done: impl FnMut() -> bool) {
    let given_up = Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(Instant::now() < given_up, "gave up waiting for {what}");
        #[expect(
            clippy::disallowed_methods,
            reason = "what's waited for isn't the test's to be woken by"
        )]
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Lets go of `attempt` as a Koyo that ended has, and gives back its folder once nobody
/// holds it. Measured: a process another test is starting holds a copy of every open file of
/// this one, an attempt's lock among them, until it becomes its program, up to 200 µs, and
/// the kernel keeps a lock while any copy of it is open. Among the other tests, about one
/// sweep in ten found a folder just let go still held; in 200 runs alone, none did. Taking
/// the lock waits for the last copy to close, and unlocking gives it back whatever copies
/// were made of this one meanwhile.
pub(super) fn let_go(attempt: Attempt) -> PathBuf {
    let folder = attempt.folder().to_owned();
    drop(attempt);
    let lock = File::open(&folder).unwrap();
    lock.lock().unwrap();
    lock.unlock().unwrap();
    folder
}

/// A released Koyo in a folder of its own, reading a bucket that is a folder.
pub struct Koyo {
    pub updates: Entity<Updates>,
    pub bundle: PathBuf,
    pub bucket: PathBuf,
    /// What the installer's stand-in writes of how it was started.
    pub told: PathBuf,
    /// How often a restart into the update was asked for.
    pub restarts: Rc<Cell<usize>>,
    fetch: Arc<Folder>,
}

/// Starts a released Koyo whose installer starts, or doesn't.
pub fn koyo(name: &str, installer_starts: bool, cx: &mut TestAppContext) -> Koyo {
    cx.executor().allow_parking();
    let root = scratch(name);
    let (beside, bucket) = (root.join("Applications"), root.join("bucket"));
    std::fs::create_dir_all(&beside).unwrap();
    std::fs::create_dir_all(&bucket).unwrap();
    let (program, told) = recorder(&root);
    let exe = if installer_starts { program } else { root.join("no-such-koyo") };
    let installed = installed(&beside, &exe);
    let bundle = installed.bundle.clone();
    let restarts = Rc::new(Cell::new(0));
    let counted = restarts.clone();
    let fetch = Arc::new(Folder::at(&bucket));
    let reading = fetch.clone();
    let updates = cx.update(|cx| {
        cx.on_action(move |_: &RestartToUpdate, _| counted.set(counted.get() + 1));
        let updates = released(installed, reading, cx);
        cx.set_global(TheUpdates::Released(updates.clone()));
        updates
    });
    Koyo { updates, bundle, bucket, told, restarts, fetch }
}

impl Koyo {
    /// Says in the feed that release `version` is out, with no image of it.
    pub fn announce(&self, version: &str) {
        let feed = format!(r#"{{ "version": "{version}", "minimum_macos": "11.0", "bytes": 1 }}"#);
        std::fs::write(self.bucket.join("feed.json"), feed).unwrap();
    }

    /// Holds the next download before anything of it comes, until what's returned is let go.
    pub fn hold_download(&self) -> Sender<()> {
        let (let_go, held) = channel();
        *self.fetch.held.lock().unwrap() = Some(held);
        let_go
    }

    /// An hour passes, and the feed is read again.
    pub fn an_hour_on(cx: &mut TestAppContext) {
        cx.executor().advance_clock(EVERY);
        cx.run_until_parked();
    }

    /// Publishes 9.0.0, as the pipeline does, and lets the feed be read.
    pub fn publish(&self, cx: &mut TestAppContext) {
        publish(&self.bucket, &Made::release("9.0.0"));
        cx.run_until_parked();
    }

    /// Publishes 9.0.0, clicks, and waits until it's staged.
    pub fn stage(&self, cx: &mut TestAppContext) {
        self.publish(cx);
        cx.update(act);
        until_it_says("Restart to Update", cx);
    }

    /// Koyo quits, as its quit hook has it.
    pub fn quit(&self, cx: &mut TestAppContext) {
        self.updates.update(cx, |updates, _| in_test(|may| updates.quitting(may)));
    }

    pub fn told(&self) -> String {
        until("the installer to start", || self.told.exists());
        std::fs::read_to_string(&self.told).unwrap()
    }

    /// Lets the app run until it has handed a release over: what the installer was told.
    pub fn handed_over(&self, cx: &mut TestAppContext) -> String {
        until("the release to be handed over", || {
            cx.run_until_parked();
            self.told.exists()
        });
        self.told()
    }
}

/// What's shown: the label of its click, or "quiet" or "working", and all it says.
pub fn says(cx: &mut TestAppContext) -> (&'static str, String) {
    cx.read(|cx| {
        let shown = shown(cx);
        let label = match &shown {
            Shown::Quiet(_) => "quiet",
            Shown::Offer { act, .. } => act,
            Shown::Working { .. } => "working",
        };
        (label, shown.said().whole().to_string())
    })
}

/// Waits for the install's thread to bring the update to where it says `label`.
pub fn until_it_says(label: &str, cx: &mut TestAppContext) {
    until(label, || {
        cx.run_until_parked();
        says(cx).0 == label
    });
    cx.run_until_parked();
}
