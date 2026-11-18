use super::support::checkout;

/// The line `--stats` ends with for a crate that blocks nowhere.
const NO_BLOCKING: &str = "Blocking calls: 0 on a Blocking, 0 owed in 0 files (koyo-rs/blocking-debt.txt), 0 at_launch outside main.rs.\n";

#[test]
fn stats_count_the_graph_and_name_the_cheapest_cuts() {
    let scratch = checkout(
        "stats",
        &[
            (
                "main.rs",
                "mod a;\nmod b;\nmod c;\nmod d;\nfn init() {\n    d::init();\n}\n",
            ),
            ("a.rs", "use crate::b::B;\nuse crate::b::Other;\n"),
            ("b.rs", "use crate::{a, c};\n"),
            ("c.rs", "use crate::a::A;\n"),
            ("d.rs", ""),
        ],
    );
    let run = scratch.run(&["--stats"]);
    assert_eq!(
        (run.code, run.stdout.as_str()),
        (
            0,
            format!(
                "\
modules 5, edges 5, edges on a cycle 4, largest cycle 3 modules
b -> a: 1 site
b -> c: 1 site
c -> a: 1 site
a -> b: 2 sites
{NO_BLOCKING}"
            )
            .as_str()
        )
    );
}

#[test]
fn stats_name_at_most_fifteen_cuts() {
    let modules: Vec<String> = (0..17).map(|index| format!("m{index:02}")).collect();
    let declared: Vec<String> = modules
        .iter()
        .map(|module| format!("mod {module};"))
        .collect();
    let mut files = vec![("main.rs".to_owned(), declared.join("\n"))];
    for (index, module) in modules.iter().enumerate() {
        let next = &modules[(index + 1) % modules.len()];
        files.push((
            format!("{module}.rs"),
            format!("use crate::{next}::Next;\n"),
        ));
    }
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    let run = checkout("stats-ring", &files).run(&["--stats"]);
    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(
        lines[0],
        "modules 18, edges 17, edges on a cycle 17, largest cycle 17 modules"
    );
    assert_eq!(
        lines[1..16],
        (0..15)
            .map(|index| format!("m{index:02} -> m{:02}: 1 site", index + 1))
            .collect::<Vec<_>>()
    );
}

#[test]
fn stats_of_a_graph_with_no_cycle_say_so() {
    let scratch = checkout(
        "stats-acyclic",
        &[
            ("main.rs", "mod a;\nmod b;\n"),
            ("a.rs", "use crate::b::B;\n"),
            ("b.rs", ""),
        ],
    );
    let run = scratch.run(&["--stats"]);
    assert_eq!(
        (run.code, run.stdout.as_str()),
        (
            0,
            format!(
                "modules 3, edges 1, edges on a cycle 0, largest cycle 0 modules\n{NO_BLOCKING}"
            )
            .as_str()
        )
    );
}
