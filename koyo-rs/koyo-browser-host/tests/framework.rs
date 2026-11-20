//! The engine's framework, from the sources `script/cef` builds it from to the engine
//! `script/browser-engine` packages, as `script/cef-framework` keeps it (`docs/browser.md` 11):
//! a release engine is packaged only from a framework built from this checkout's patches with
//! H.264 and AAC, and sources shared by checkouts never build a mix of patches none lists.
#![cfg(test)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CODECS: &str =
    "ffmpeg_branding=\"Chrome\"\nis_official_build=true\nproprietary_codecs=true\n";

fn script() -> PathBuf {
    PathBuf::from(env!("KOYO_CHECKOUT")).join("script/cef-framework")
}

fn run(args: &[&str]) -> Output {
    Command::new(script()).args(args).output().unwrap()
}

fn said(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A framework at `root/<name>` whose manifest is `manifest`, or that has none.
fn framework(root: &Path, name: &str, manifest: Option<&str>) -> String {
    let framework = root.join(name);
    std::fs::create_dir_all(framework.join("Resources")).unwrap();
    if let Some(manifest) = manifest {
        std::fs::write(framework.join("Resources/koyo-build.txt"), manifest).unwrap();
    }
    framework.to_string_lossy().into_owned()
}

/// What a framework `script/cef` built with `args` from this checkout's patches says of itself.
fn koyos_manifest(root: &Path, args: &str) -> String {
    let args_gn = root.join("args.gn");
    std::fs::write(&args_gn, args).unwrap();
    let patches = PathBuf::from(env!("KOYO_CHECKOUT")).join("script/cef-patches");
    let output = Command::new(script())
        .arg("manifest")
        .arg(&args_gn)
        .arg(&patches)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", said(&output));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn a_release_engine_is_packaged_only_from_koyos_framework() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let koyos = koyos_manifest(root, CODECS);
    let first = koyos
        .lines()
        .find(|line| line.starts_with("patch "))
        .unwrap();
    let (named, _hash) = first.rsplit_once(' ').unwrap();
    let name = named.trim_start_matches("patch ");
    let edited = koyos.replace(first, &format!("{named} {}", "0".repeat(64)));
    let without = koyos.replace(&format!("{first}\n"), "");
    let unlisted = format!("{koyos}patch an-old-experiment 1234\n");
    let no_codecs = koyos_manifest(root, "ffmpeg_branding=\"Chrome\"\n");

    let passes = run(&[
        "check",
        &framework(root, "koyos", Some(&koyos)),
        "--release",
    ]);
    assert!(passes.status.success(), "{}", said(&passes));

    for (framework, why) in [
        (
            framework(root, "cef-own", None),
            "it has no Resources/koyo-build.txt".to_owned(),
        ),
        (
            framework(root, "edited", Some(&edited)),
            format!("it has another version of {name}.patch"),
        ),
        (
            framework(root, "without", Some(&without)),
            format!("it hasn't {name}.patch"),
        ),
        (
            framework(root, "unlisted", Some(&unlisted)),
            "it has an-old-experiment.patch".to_owned(),
        ),
        (
            framework(root, "no-codecs", Some(&no_codecs)),
            "proprietary_codecs=true".to_owned(),
        ),
    ] {
        let refused = run(&["check", &framework, "--release"]);
        assert!(!refused.status.success(), "{framework} was packaged");
        assert!(
            said(&refused).contains(&why),
            "{framework}: {}",
            said(&refused)
        );
        assert!(said(&refused).contains("script/cef"), "{}", said(&refused));
    }
}

#[test]
fn a_debug_engine_is_packaged_from_cef_own_framework_saying_what_it_lacks() {
    let root = tempfile::tempdir().unwrap();
    let packaged = run(&["check", &framework(root.path(), "cef-own", None)]);
    assert!(packaged.status.success(), "{}", said(&packaged));
    assert!(said(&packaged).contains("warning"), "{}", said(&packaged));
    assert!(
        said(&packaged).contains("no H.264 or AAC"),
        "{}",
        said(&packaged)
    );
}

/// Chromium's sources in small: `a.txt`, `b.txt` and `c.txt`, with three sets of patches beside.
/// `listed` has `a.patch`, a line in `a.txt` and one in `b.txt`, and `b.patch`, a line in
/// `c.txt`; `edited` has `a.patch` cut down to its `a.txt` line; `dropped` has `a.patch` alone.
struct Sources {
    root: tempfile::TempDir,
}

impl Sources {
    fn new() -> Self {
        let sources = Self {
            root: tempfile::tempdir().unwrap(),
        };
        let src = sources.src();
        std::fs::create_dir_all(&src).unwrap();
        sources.git(&["init", "-q"]);
        for (file, text) in [
            ("a.txt", "one\ntwo\n"),
            ("b.txt", "three\nfour\n"),
            ("c.txt", "five\nsix\n"),
        ] {
            std::fs::write(src.join(file), text).unwrap();
        }
        sources.git(&["add", "."]);
        sources.git(&["commit", "-qm", "sources"]);
        sources.patch(
            "listed/a.patch",
            &[("a.txt", "one\nTWO\n"), ("b.txt", "three\nFOUR\n")],
            &[],
        );
        sources.patch(
            "edited/a.patch",
            &[("a.txt", "one\nTWO\n"), ("b.txt", "three\nFOUR\n")],
            &["a.txt"],
        );
        sources.patch("listed/b.patch", &[("c.txt", "five\nSIX\n")], &[]);
        for (from, to) in [
            ("listed/b.patch", "edited/b.patch"),
            ("listed/a.patch", "dropped/a.patch"),
        ] {
            std::fs::create_dir_all(sources.root.path().join(to).parent().unwrap()).unwrap();
            std::fs::copy(sources.root.path().join(from), sources.root.path().join(to)).unwrap();
        }
        sources
    }

    fn src(&self) -> PathBuf {
        self.root.path().join("src")
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.src())
            .args([
                "-c",
                "user.name=koyo",
                "-c",
                "user.email=koyo@example.invalid",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {}", said(&output));
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Writes the diff that makes `changes` to `to`, of the files `only` or all, and undoes it.
    fn patch(&self, to: &str, changes: &[(&str, &str)], only: &[&str]) {
        for (file, text) in changes {
            std::fs::write(self.src().join(file), text).unwrap();
        }
        let mut diff = vec!["diff", "--"];
        diff.extend(only);
        let to = self.root.path().join(to);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::write(to, self.git(&diff)).unwrap();
        self.git(&["checkout", "-q", "."]);
    }

    fn apply(&self, patches: &str) -> Output {
        let src = self.src().to_string_lossy().into_owned();
        let patches = self
            .root
            .path()
            .join(patches)
            .to_string_lossy()
            .into_owned();
        run(&["patch", &src, "--patches", &patches])
    }

    /// The files the sources carry a change to.
    fn changed(&self) -> String {
        self.git(&["diff", "--name-only"])
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[test]
fn sources_carrying_a_patch_the_checkout_lists_otherwise_build_nothing_until_it_is_taken_out() {
    let sources = Sources::new();
    let applied = sources.apply("listed");
    assert!(applied.status.success(), "{}", said(&applied));
    assert_eq!(sources.changed(), "a.txt b.txt c.txt");
    assert!(
        sources.apply("listed").status.success(),
        "a second run finds them in"
    );

    let edited = sources.apply("edited");
    assert!(
        !edited.status.success(),
        "a.patch edited down was taken for applied"
    );
    assert!(
        said(&edited).contains("a.patch, in another version"),
        "{}",
        said(&edited)
    );
    let dropped = sources.apply("dropped");
    assert!(
        !dropped.status.success(),
        "b.patch, no longer listed, stayed in"
    );
    assert!(
        said(&dropped).contains("b.patch, which this checkout hasn't"),
        "{}",
        said(&dropped)
    );
    assert_eq!(
        sources.changed(),
        "a.txt b.txt c.txt",
        "nothing applied or taken out meanwhile"
    );

    let take_out = said(&edited);
    let command = take_out
        .lines()
        .find(|line| line.trim_start().starts_with("git -C"))
        .unwrap();
    let ran = Command::new("sh")
        .arg("-c")
        .arg(command.trim())
        .output()
        .unwrap();
    assert!(ran.status.success(), "{command}: {}", said(&ran));
    let again = sources.apply("edited");
    assert!(again.status.success(), "{}", said(&again));
    assert_eq!(
        sources.changed(),
        "a.txt c.txt",
        "the sources carry the edited patch alone"
    );
}

#[test]
fn sources_patched_by_a_script_cef_that_kept_no_record_are_taken_as_they_are() {
    let sources = Sources::new();
    for patch in ["a.patch", "b.patch"] {
        let patch = sources
            .root
            .path()
            .join("listed")
            .join(patch)
            .to_string_lossy()
            .into_owned();
        sources.git(&["apply", &patch]);
    }
    let adopted = sources.apply("listed");
    assert!(adopted.status.success(), "{}", said(&adopted));
    let refused = sources.apply("dropped");
    assert!(
        said(&refused).contains("b.patch, which this checkout hasn't"),
        "{}",
        said(&refused)
    );
}
