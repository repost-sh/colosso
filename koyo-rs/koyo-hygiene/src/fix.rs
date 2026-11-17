mod blank;
mod edits;
mod proof;

use std::fmt;

use crate::check::{Finding, findings_of};
use crate::language::{Language, ParseFailed};
use crate::lexical::may_hold_plain_comment;
use crate::scan::{Standing, scan};

pub struct Fix {
    pub rewritten: Option<String>,
    pub deleted: usize,
    pub remaining: Vec<Finding>,
}

#[derive(Debug)]
pub enum CannotFix {
    Parse(ParseFailed),
    CommentsRemain,
    TokensDiffer,
}

impl From<ParseFailed> for CannotFix {
    fn from(failure: ParseFailed) -> Self {
        Self::Parse(failure)
    }
}

impl fmt::Display for CannotFix {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(failure) => write!(formatter, "{failure}"),
            Self::CommentsRemain => write!(formatter, "comments would remain after the fix"),
            Self::TokensDiffer => write!(
                formatter,
                "the fix would change the code, not only its comments"
            ),
        }
    }
}

pub fn fix(language: Language, source: &str) -> Result<Fix, CannotFix> {
    if !may_hold_plain_comment(language, source) {
        return Ok(Fix {
            rewritten: None,
            deleted: 0,
            remaining: Vec::new(),
        });
    }
    let original = scan(language, source)?;
    let doomed: Vec<_> = original
        .comments
        .iter()
        .filter(|comment| comment.standing == Standing::Plain)
        .map(|comment| comment.range.clone())
        .collect();
    if doomed.is_empty() {
        return Ok(Fix {
            rewritten: None,
            deleted: 0,
            remaining: findings_of(&original.comments, source),
        });
    }
    let edited = edits::rewrite(source, &doomed);
    let text = blank::tidy(&edited.text, &edited.seams);
    let after = scan(language, &text)?;
    if after
        .comments
        .iter()
        .any(|comment| comment.standing == Standing::Plain)
    {
        return Err(CannotFix::CommentsRemain);
    }
    if !proof::same_code(
        language,
        (&original.tree, source, &doomed),
        (&after.tree, &text),
    ) {
        return Err(CannotFix::TokensDiffer);
    }
    Ok(Fix {
        remaining: findings_of(&after.comments, &text),
        rewritten: Some(text),
        deleted: doomed.len(),
    })
}
