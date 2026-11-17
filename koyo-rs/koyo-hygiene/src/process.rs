use std::fs;
use std::io;

use crate::check::{Finding, check};
use crate::cli::Mode;
use crate::failure::Failure;
use crate::files::{Presence, Source};
use crate::fix::fix;

pub struct FileReport {
    pub path: String,
    pub findings: Vec<Finding>,
    pub deleted: usize,
}

pub fn process(source: &Source, mode: Mode) -> Result<Option<FileReport>, Failure> {
    let text = match fs::read_to_string(&source.path) {
        Ok(text) => text,
        Err(failure)
            if failure.kind() == io::ErrorKind::NotFound
                && source.presence == Presence::MayBeGone =>
        {
            return Ok(None);
        }
        Err(failure) => return Err(Failure::new(&source.display, failure)),
    };
    let report = |findings, deleted| {
        Ok(Some(FileReport {
            path: source.display.clone(),
            findings,
            deleted,
        }))
    };
    match mode {
        Mode::Check => {
            let findings = check(source.language, &text)
                .map_err(|failure| Failure::new(&source.display, failure))?;
            report(findings, 0)
        }
        Mode::Fix => {
            let fixed = fix(source.language, &text).map_err(|failure| {
                Failure::new(&source.display, format!("not fixable: {failure}"))
            })?;
            if let Some(rewritten) = &fixed.rewritten {
                fs::write(&source.path, rewritten)
                    .map_err(|failure| Failure::new(&source.display, failure))?;
            }
            report(fixed.remaining, fixed.deleted)
        }
    }
}
