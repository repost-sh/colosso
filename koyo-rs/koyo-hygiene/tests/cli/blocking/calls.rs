use super::support::{CallCase, assert_call_cases, launches, with_debt};

const ROOT: &str = "mod a;\n";

#[test]
fn a_call_blocks_however_the_file_names_it() {
    assert_call_cases(&[
        CallCase {
            name: "written-out",
            files: &[
                ("main.rs", ROOT),
                ("a.rs", "fn f() {\n    let _ = std::fs::read(\"x\");\n}\n"),
            ],
            calls: &["a.rs:2:13: std::fs::read"],
        },
        CallCase {
            name: "through-use",
            files: &[
                ("main.rs", ROOT),
                (
                    "a.rs",
                    "\
use std::fs::{self, File};
use std::process::Command as Process;
use std::{fs as files, io};
fn f() -> io::Result<()> {
    fs::remove_file(\"x\")?;
    let _ = File::open(\"y\")?;
    files::write(\"z\", b\"\")?;
    Process::new(\"git\").status()?;
    Ok(())
}
fn g(_: &mut Process, _: File) {}
",
                ),
            ],
            calls: &[
                "a.rs:5:5: std::fs::remove_file",
                "a.rs:6:13: std::fs::File::open",
                "a.rs:7:5: std::fs::write",
                "a.rs:8:5: std::process::Command::new",
                "a.rs:11:14: std::process::Command",
                "a.rs:11:26: std::fs::File",
            ],
        },
        CallCase {
            name: "in-macros-and-from-the-root",
            files: &[
                ("main.rs", ROOT),
                (
                    "a.rs",
                    "\
fn f(path: &std::path::Path) {
    log::info!(\"{:?} {}\", ::std::fs::read_to_string(path), path.exists());
}
",
                ),
            ],
            calls: &["a.rs:2:29: std::fs::read_to_string", "a.rs:2:65: .exists()"],
        },
        CallCase {
            name: "a-glob",
            files: &[("main.rs", ROOT), ("a.rs", "use std::fs::*;\n")],
            calls: &["a.rs:1:14: std::fs::*"],
        },
    ]);
}

#[test]
fn asking_a_path_of_the_file_system_blocks() {
    assert_call_cases(&[CallCase {
        name: "probes",
        files: &[
            ("main.rs", ROOT),
            (
                "a.rs",
                "\
use std::path::{Path, PathBuf};
fn f(path: &Path, paths: &[PathBuf]) -> bool {
    path.is_file()
        || paths.iter().any(|path| path.join(\"x\").canonicalize().is_ok())
        || paths.iter().map(PathBuf::as_path).any(Path::is_dir)
        || path.metadata().is_ok()
}
",
            ),
        ],
        calls: &[
            "a.rs:3:10: .is_file()",
            "a.rs:4:51: .canonicalize()",
            "a.rs:5:51: std::path::Path::is_dir",
            "a.rs:6:17: .metadata()",
        ],
    }]);
}

#[test]
fn what_only_shares_a_name_with_a_blocking_call_is_free() {
    assert_call_cases(&[CallCase {
        name: "look-alikes",
        files: &[
            ("main.rs", ROOT),
            (
                "a.rs",
                "\
use std::fs;
use std::process::ExitCode;
use koyo_app_control::Command;
fn f(fs: u8, found: &Found) -> ExitCode {
    let _ = Command::new();
    let _ = (fs, found.exists, \"std::fs::read\", Vec::<u8>::new());
    let _ = found.is_file;
    ExitCode::SUCCESS
}
fn later() -> impl Sized + use<> {}
",
            ),
        ],
        calls: &[],
    }]);
}

#[test]
fn tests_and_features_are_left_out() {
    assert_call_cases(&[CallCase {
        name: "left-out",
        files: &[
            ("main.rs", "mod a;\n#[cfg(test)]\nmod tests;\n"),
            (
                "a.rs",
                "\
#[cfg(test)]
fn helper() {
    std::fs::read(\"x\");
}
#[cfg(feature = \"visual-check\")]
fn check() {
    std::fs::read(\"x\");
}
#[test]
fn reads() {
    std::fs::read(\"x\");
}
",
            ),
            ("tests.rs", "fn f() {\n    std::fs::read(\"x\");\n}\n"),
        ],
        calls: &[],
    }]);
}

#[test]
fn only_the_logs_sink_blocks_as_it_pleases() {
    assert_call_cases(&[CallCase {
        name: "free-files",
        files: &[
            ("main.rs", "mod blocking;\nmod logs;\n"),
            (
                "blocking.rs",
                "pub fn read() {\n    std::fs::read(\"x\");\n}\n",
            ),
            ("logs/mod.rs", "mod sink;\n"),
            (
                "logs/sink.rs",
                "fn open() {\n    std::fs::File::create(\"x\");\n}\n",
            ),
        ],
        calls: &["blocking.rs:2:5: std::fs::read"],
    }]);
}

#[test]
fn a_launch_read_outside_main_fails_whatever_the_list_allows() {
    let scratch = with_debt(
        "launch",
        &[
            (
                "main.rs",
                "mod a;\nmod b;\nmod blocking;\nfn main() {\n    blocking::at_launch(\"x\", |_| ());\n}\n",
            ),
            (
                "a.rs",
                "fn f() {\n    crate::blocking::at_launch(\"x\", |_| ());\n}\n",
            ),
            (
                "b.rs",
                "use crate::blocking::{Blocking, at_launch as early};\nfn f(_: Blocking<'_>) {\n    early(\"x\", |_| ());\n}\n",
            ),
            ("blocking.rs", ""),
        ],
        "koyo/src/a.rs 1\nkoyo/src/b.rs 1\n",
    );
    let run = scratch.run(&[]);
    assert_eq!(
        launches(&run),
        [
            "a.rs:2:5: crate::blocking::at_launch",
            "b.rs:3:5: crate::blocking::at_launch",
        ]
    );
    assert!(run.stdout.contains("no debt allows it"), "{}", run.stdout);
}

#[test]
fn waiting_on_a_child_an_executor_or_a_park_blocks() {
    assert_call_cases(&[CallCase {
        name: "waits",
        files: &[
            ("main.rs", ROOT),
            (
                "a.rs",
                "\
use std::process::Child;
use std::thread;
fn f(child: &mut Child, executor: &Executor, cx: &App) {
    let _ = child.wait();
    let _ = child.try_wait();
    executor.block(task);
    cx.background_executor().block_with_timeout(limit, task);
    smol::block_on(task);
    cx.foreground_executor().block_on(task);
    thread::park_timeout(limit);
    let _ = [child].into_iter().map(Child::wait);
}
",
            ),
        ],
        calls: &[
            "a.rs:4:19: .wait()",
            "a.rs:5:19: .try_wait()",
            "a.rs:6:14: .block()",
            "a.rs:7:30: .block_with_timeout()",
            "a.rs:8:5: smol::block_on",
            "a.rs:9:30: .block_on()",
            "a.rs:10:5: std::thread::park_timeout",
            "a.rs:11:37: std::process::Child::wait",
        ],
    }]);
}

#[test]
fn unix_sockets_and_file_modes_and_a_pub_use_of_the_file_system_block() {
    assert_call_cases(&[CallCase {
        name: "unix-and-exports",
        files: &[
            ("main.rs", ROOT),
            (
                "a.rs",
                "\
use std::os::unix::net::UnixStream;
pub use std::fs::read;
pub(crate) use std::fs::{self as files, write};
use std::fs::remove_file;
fn f() {
    let _ = UnixStream::connect(\"x\");
    std::os::unix::fs::symlink(\"x\", \"y\");
}
",
            ),
        ],
        calls: &[
            "a.rs:2:18: std::fs::read",
            "a.rs:3:41: std::fs::write",
            "a.rs:6:13: std::os::unix::net::UnixStream::connect",
            "a.rs:7:5: std::os::unix::fs::symlink",
        ],
    }]);
}

#[test]
fn a_method_of_the_same_name_on_what_evidently_isnt_a_path_child_or_executor_is_free() {
    assert_call_cases(&[CallCase {
        name: "typed-receivers",
        files: &[
            ("main.rs", ROOT),
            (
                "a.rs",
                "\
use std::path::Path;
fn log(record: &Record<'_>, gate: &Barrier, notice: Notice, path: &Path) {
    let _ = record.metadata();
    gate.wait();
    let _ = notice.block(p);
    let _ = path.metadata();
    let _ = entry.metadata();
    let _ = |record: Record| record.metadata();
}
",
            ),
        ],
        calls: &["a.rs:6:18: .metadata()", "a.rs:7:19: .metadata()"],
    }]);
}
