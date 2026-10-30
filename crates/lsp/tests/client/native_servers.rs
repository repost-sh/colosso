//! The client against the real TypeScript server and Pyrefly: each downloaded into the data
//! folder, started, given a file, and asked what a name is.
//!
//! It downloads the servers the first time, so it runs only when asked:
//! `COLOSSO_LSP_IT=1 COLOSSO_DATA_DIR=$PWD/.superpowers/data cargo test -p colosso-lsp -- --ignored native_servers`

use std::time::Instant;

use colosso_lsp::servers::{Curl, PYREFLY, TYPESCRIPT};

use crate::common::{GREET_PY, GREET_TS, data_dir, fixture, probe};

#[test]
#[ignore = "downloads and runs TypeScript's server and Pyrefly; see this file's header"]
fn typescript_and_pyrefly_say_what_a_call_is() {
    let Some(data) = data_dir("the native servers") else { return };
    for (server, call) in [(&TYPESCRIPT, &GREET_TS), (&PYREFLY, &GREET_PY)] {
        let took = Instant::now();
        let binary = server.install(&data, &Curl).unwrap();
        eprintln!("{}: installed, {:.1} s", server.name, took.elapsed().as_secs_f64());
        let root = fixture(server.name, &[(call.file, call.text)]);
        probe(&server.launch(binary, &root), &root, call);
    }
}
