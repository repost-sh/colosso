use crate::language::{Language, ParseFailed};
use crate::lexical::may_hold_plain_comment;
use crate::rule::{COMMENT, Rule, SAFETY};
use crate::scan::{Comment, Standing, scan};
use crate::text::character_column;

const EXCERPT_CHARACTERS: usize = 80;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub column: usize,
    pub rule: &'static Rule,
    pub excerpt: String,
}

pub fn check(language: Language, source: &str) -> Result<Vec<Finding>, ParseFailed> {
    if !may_hold_plain_comment(language, source) {
        return Ok(Vec::new());
    }
    Ok(findings_of(&scan(language, source)?.comments, source))
}

pub fn findings_of(comments: &[Comment], source: &str) -> Vec<Finding> {
    comments
        .iter()
        .filter_map(|comment| {
            let rule = match comment.standing {
                Standing::Plain => &COMMENT,
                Standing::SafetyHead { attached: false } => &SAFETY,
                Standing::SafetyHead { attached: true } | Standing::SafetyTail => return None,
            };
            Some(Finding {
                line: comment.start.row + 1,
                column: character_column(source, comment.range.start),
                rule,
                excerpt: excerpt(&source[comment.range.clone()]),
            })
        })
        .collect()
}

fn excerpt(comment: &str) -> String {
    let first_line = comment.lines().next().unwrap_or_default().trim();
    match first_line.char_indices().nth(EXCERPT_CHARACTERS) {
        Some((cut, _)) => format!("{}...", first_line[..cut].trim_end()),
        None => first_line.to_owned(),
    }
}
