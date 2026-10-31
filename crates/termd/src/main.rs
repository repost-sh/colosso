//! `colosso-termd`: the terminal daemon on its own, for tests and the command line. The app
//! runs the same daemon as `colosso termd`. See the library's documentation.

use std::process::ExitCode;

fn main() -> ExitCode {
    colosso_termd::main(std::env::args().skip(1))
}
