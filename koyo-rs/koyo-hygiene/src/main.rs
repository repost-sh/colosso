mod app;
mod blocking;
mod check;
mod cli;
mod failure;
mod files;
mod fix;
mod imports;
mod language;
mod lexical;
mod list_file;
mod pool;
mod process;
mod report;
mod rule;
mod scan;
mod shipped;
mod text;

use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut out = io::stdout().lock();
    let mut err = io::stderr().lock();
    app::run(std::env::args_os().skip(1), &mut out, &mut err)
}
