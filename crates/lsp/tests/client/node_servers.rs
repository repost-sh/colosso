//! The client against the real vtsls: installed by npm from the Node on PATH into the data
//! folder, started, given a file, and asked what a name is.
//!
//! It installs the package the first time, so it runs only when asked:
//! `COLOSSO_LSP_IT=1 COLOSSO_DATA_DIR=$PWD/.superpowers/data cargo test -p colosso-lsp -- --ignored node_servers`

use std::time::Instant;

use colosso_lsp::servers::{NpmCli, VTSLS, node};

use crate::common::{GREET_TS, data_dir, fixture, probe};

#[test]
#[ignore = "installs and runs vtsls; see this file's header"]
fn vtsls_says_what_a_call_is() {
    let Some(data) = data_dir("vtsls") else { return };
    let node = node().expect("Node on PATH");
    let npm = NpmCli { node: node.clone() };
    let took = Instant::now();
    let script = VTSLS.script(&data, &npm).unwrap();
    eprintln!("{}: installed, {:.1} s", VTSLS.name, took.elapsed().as_secs_f64());
    let root = fixture(VTSLS.name, &[(GREET_TS.file, GREET_TS.text)]);
    probe(&VTSLS.launch(node, &script, &root), &root, &GREET_TS);
}
