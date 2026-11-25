use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Run {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

impl Run {
    pub fn findings(&self) -> Vec<&str> {
        self.stdout
            .lines()
            .take_while(|line| !line.is_empty())
            .collect()
    }
}

/// A folder of a test's own. It sits in the workspace's `target/tmp`, not
/// `CARGO_TARGET_TMPDIR`, which a shared build folder may put outside the checkout.
pub struct Scratch {
    root: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        let root = PathBuf::from(env!("KOYO_TARGET_DIR"))
            .join("tmp")
            .join("hygiene")
            .join(name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("scratch folder");
        Self { root }
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub fn write(&self, relative: &str, text: &str) {
        self.write_bytes(relative, text.as_bytes());
    }

    pub fn write_bytes(&self, relative: &str, bytes: &[u8]) {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().expect("a parent folder")).expect("fixture folder");
        fs::write(path, bytes).expect("fixture file");
    }

    /// Moves a file on disk, and tells git nothing.
    pub fn rename(&self, from: &str, to: &str) {
        let to = self.path(to);
        fs::create_dir_all(to.parent().expect("a parent folder")).expect("fixture folder");
        fs::rename(self.path(from), to).expect("fixture file");
    }

    pub fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path(relative)).expect("fixture file")
    }

    pub fn run(&self, arguments: &[&str]) -> Run {
        self.run_in(Path::new(""), arguments)
    }

    pub fn run_in(&self, folder: &Path, arguments: &[&str]) -> Run {
        let output = Command::new(env!("CARGO_BIN_EXE_koyo-hygiene"))
            .args(arguments)
            .current_dir(self.root.join(folder))
            .env("GIT_CEILING_DIRECTORIES", self.outside())
            .output()
            .expect("the binary runs");
        Run {
            stdout: String::from_utf8(output.stdout).expect("utf-8 stdout"),
            stderr: String::from_utf8(output.stderr).expect("utf-8 stderr"),
            code: output.status.code().expect("an exit code"),
        }
    }

    pub fn git(&self, arguments: &[&str]) {
        let status = Command::new("git")
            .args(arguments)
            .current_dir(&self.root)
            .env("GIT_CEILING_DIRECTORIES", self.outside())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .status()
            .expect("git runs");
        assert!(status.success(), "git {arguments:?}");
    }

    pub fn commit(&self) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", "fixture"]);
    }

    /// The folder above the scratch, where git stops looking for a checkout, since the
    /// scratch sits inside Koyo's own.
    fn outside(&self) -> &Path {
        self.root.parent().expect("a parent folder")
    }
}

pub struct CheckCase {
    pub name: &'static str,
    pub file: &'static str,
    pub source: &'static str,
    pub findings: &'static [&'static str],
}

pub fn assert_check_cases(group: &str, cases: &[CheckCase]) {
    let mut failures = Vec::new();
    for case in cases {
        let scratch = Scratch::new(&format!("{group}-{}", case.name));
        scratch.write(case.file, case.source);
        let run = scratch.run(&[case.file]);
        let expected_code = i32::from(!case.findings.is_empty());
        if run.findings() != case.findings || run.code != expected_code || !run.stderr.is_empty() {
            failures.push(format!(
                "{}: expected {:?} (exit {expected_code}), got {:?} (exit {}, stderr {:?})",
                case.name,
                case.findings,
                run.findings(),
                run.code,
                run.stderr
            ));
        }
        if case.findings.is_empty() && !run.stdout.is_empty() {
            failures.push(format!(
                "{}: stdout must be empty, got {:?}",
                case.name, run.stdout
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

pub struct FixCase {
    pub name: &'static str,
    pub file: &'static str,
    pub before: &'static str,
    pub after: &'static str,
    pub deleted: usize,
}

pub fn deleted_summary(deleted: usize, files: usize) -> String {
    let comments = if deleted == 1 { "comment" } else { "comments" };
    let files_noun = if files == 1 { "file" } else { "files" };
    format!("Deleted {deleted} {comments} from {files} {files_noun}.\n")
}

pub fn assert_fix_cases(group: &str, cases: &[FixCase]) {
    let mut failures = Vec::new();
    for case in cases {
        let scratch = Scratch::new(&format!("{group}-{}", case.name));
        scratch.write(case.file, case.before);
        let first = scratch.run(&["--fix", case.file]);
        let fixed = scratch.read(case.file);
        let files = usize::from(case.deleted > 0);
        if fixed != case.after
            || first.code != 0
            || first.stdout != deleted_summary(case.deleted, files)
        {
            failures.push(format!(
                "{}: expected {:?} deleting {}, got {:?} (exit {}, stdout {:?}, stderr {:?})",
                case.name, case.after, case.deleted, fixed, first.code, first.stdout, first.stderr
            ));
            continue;
        }
        let second = scratch.run(&["--fix", case.file]);
        if scratch.read(case.file) != case.after
            || second.stdout != deleted_summary(0, 0)
            || second.code != 0
        {
            failures.push(format!(
                "{}: a second --fix changed something: {:?}",
                case.name, second.stdout
            ));
        }
        let check = scratch.run(&[case.file]);
        if check.code != 0 || !check.stdout.is_empty() {
            failures.push(format!(
                "{}: check fails after --fix: {:?}",
                case.name, check.stdout
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
