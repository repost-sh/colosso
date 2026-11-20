use crate::support::{Run, Scratch};

pub const LIST: &str = "koyo-rs/module-edges.txt";
pub const DEBT: &str = "koyo-rs/blocking-debt.txt";
pub const HOST_DEBT: &str = "koyo-rs/blocking-debt-browser-host.txt";
pub const SOURCE: &str = "koyo-rs/koyo/src/";
pub const HOST: &str = "koyo-rs/koyo-browser-host/src/";

/// A git checkout with no commit yet, these files under `koyo-rs/koyo/src/`, a browser host
/// with an empty crate root, and an empty edge list and debt list.
pub fn checkout(name: &str, files: &[(&str, &str)]) -> Scratch {
    let scratch = Scratch::new(&format!("imports-{name}"));
    for (path, text) in files {
        scratch.write(&format!("{SOURCE}{path}"), text);
    }
    scratch.write(&format!("{HOST}lib.rs"), "");
    scratch.write(LIST, "");
    scratch.write(DEBT, "");
    scratch.write(HOST_DEBT, "");
    scratch.git(&["init", "-q"]);
    scratch
}

/// A crate, and the edges its source makes, each as `<file>:<line>: <from> -> <to>` with the
/// file under `koyo-rs/koyo/src/`, by place.
pub struct ScanCase {
    pub name: &'static str,
    pub files: &'static [(&'static str, &'static str)],
    pub edges: &'static [&'static str],
}

/// Checks each case with a module `hub` added that reaches every module the crate root
/// declares, and the root, and that the root and every file reach. So every edge the source
/// makes lies on a cycle, and the check reports it. The hub's own edges are left out.
pub fn assert_scan_cases(group: &str, cases: &[ScanCase]) {
    let mut failures = Vec::new();
    for case in cases {
        let files = with_hub(case.files);
        let files: Vec<(&str, &str)> = files
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect();
        let run = checkout(&format!("{group}-{}", case.name), &files).run(&[]);
        let edges = new_edges(&run);
        if edges != case.edges || run.code == 2 || !run.stderr.is_empty() {
            failures.push(format!(
                "{}: expected {:?}, got {:?} (exit {}, stderr {:?})",
                case.name, case.edges, edges, run.code, run.stderr
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn with_hub(files: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut modules = Vec::new();
    let mut hubbed = Vec::new();
    for (path, text) in files {
        let link = if *path == "main.rs" {
            modules.extend(text.lines().filter_map(|line| {
                let name = line.strip_prefix("mod ")?;
                name.split([';', ' ']).next()
            }));
            "mod hub;\nfn hub_link() {\n    crate::hub::link();\n}\n"
        } else {
            "use crate::hub::Hub;\n"
        };
        hubbed.push(((*path).to_owned(), format!("{text}\n{link}")));
    }
    hubbed.push((
        "hub.rs".to_owned(),
        format!(
            "use crate::{{{}}};\nfn link() {{\n    crate::hub_link();\n}}\n",
            modules.join(", ")
        ),
    ));
    hubbed
}

/// The new edges the run reports, as `<file>:<line>: <from> -> <to>`, but for the hub's.
fn new_edges(run: &Run) -> Vec<String> {
    run.findings()
        .iter()
        .filter_map(|line| {
            let (place, detail) = line.strip_prefix(SOURCE)?.split_once(": new edge: ")?;
            let (file_and_line, _column) = place.rsplit_once(':')?;
            let edge = detail.split(" closes ").next()?;
            let hub = edge.split(" -> ").any(|module| module == "hub");
            (!hub).then(|| format!("{file_and_line}: {edge}"))
        })
        .collect()
}
