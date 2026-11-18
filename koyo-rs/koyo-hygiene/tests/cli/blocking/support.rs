use crate::imports::support::{SOURCE, checkout};
use crate::support::{Run, Scratch};

pub use crate::imports::support::DEBT;

/// A git checkout with no commit yet, these files under `koyo-rs/koyo/src/`, an empty edge
/// list and `debt` as the debt list.
pub fn with_debt(name: &str, files: &[(&str, &str)], debt: &str) -> Scratch {
    let scratch = checkout(&format!("blocking-{name}"), files);
    scratch.write(DEBT, debt);
    scratch
}

/// The blocking calls the run reports, as `<file>:<line>:<column>: <what>` with the file
/// under `koyo-rs/koyo/src/`.
pub fn calls(run: &Run) -> Vec<String> {
    of_rule(run, "blocking call")
}

/// The `blocking::at_launch` outside the crate root the run reports, as [`calls`] are.
pub fn launches(run: &Run) -> Vec<String> {
    of_rule(run, "launch read")
}

fn of_rule(run: &Run, rule: &str) -> Vec<String> {
    let rule = format!(": {rule}: ");
    run.findings()
        .iter()
        .filter_map(|line| {
            let (place, what) = line.strip_prefix(SOURCE)?.split_once(&rule)?;
            Some(format!("{place}: {what}"))
        })
        .collect()
}

/// Files, and the blocking calls their source makes, as `<file>:<line>:<column>: <what>`.
pub struct CallCase {
    pub name: &'static str,
    pub files: &'static [(&'static str, &'static str)],
    pub calls: &'static [&'static str],
}

pub fn assert_call_cases(cases: &[CallCase]) {
    let mut failures = Vec::new();
    for case in cases {
        let run = with_debt(case.name, case.files, "").run(&[]);
        let calls = calls(&run);
        let expected_code = i32::from(!case.calls.is_empty());
        if calls != case.calls || run.code != expected_code || !run.stderr.is_empty() {
            failures.push(format!(
                "{}: expected {:?}, got {:?} (exit {}, stdout {:?}, stderr {:?})",
                case.name, case.calls, calls, run.code, run.stdout, run.stderr
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
