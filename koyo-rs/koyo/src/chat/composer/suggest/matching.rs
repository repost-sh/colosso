//! What a word being typed completes, and what of a list goes with it.

use std::ops::Range;

use gpui_kit::SharedString;
use koyo_agent::protocol::methods::Command;

/// What's being completed, as far as it's typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Token {
    pub(super) kind: TokenKind,
    /// All of it, in bytes, whichever part of it the caret is in; its mark included.
    pub(super) range: Range<usize>,
    /// What's typed of it before the caret.
    pub(super) query: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TokenKind {
    File,
    Command,
}

/// What the word the caret's in, at byte `cursor` of `text`, completes: anything that starts `@`,
/// and what starts `/` as the first word of a message, which is where a command goes.
pub(super) fn token_at(text: &str, cursor: usize) -> Option<Token> {
    let before = text.get(..cursor)?;
    let after = text.get(cursor..)?;
    let start = before.trim_end_matches(|at: char| !at.is_whitespace()).len();
    let end = cursor + after.find(char::is_whitespace).unwrap_or(after.len());
    let word = before.get(start..)?;
    let (kind, query) = match (word.strip_prefix('@'), word.strip_prefix('/')) {
        (Some(query), _) => (TokenKind::File, query),
        (_, Some(query)) if start == 0 => (TokenKind::Command, query),
        _ => return None,
    };
    Some(Token {
        kind,
        range: start..end,
        query: query.to_owned(),
    })
}

/// A command Koyo does itself, rather than the agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Own {
    Model,
    Effort,
    Plan,
    Compact,
    Clear,
}

const OWN: [(&str, &str, Own); 5] = [
    ("model", "Choose the model", Own::Model),
    ("effort", "Choose how hard the agent works", Own::Effort),
    ("plan", "Plan first, or stop planning first", Own::Plan),
    ("compact", "Summarize the conversation, to free up context", Own::Compact),
    ("clear", "Start a new chat", Own::Clear),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CommandRow {
    pub(super) name: SharedString,
    pub(super) hint: Option<SharedString>,
    pub(super) description: SharedString,
    pub(super) own: Option<Own>,
}

/// The commands whose names have `query` in them, those starting with it first; Koyo's own go by
/// their names ahead of any of the agent's. A chat that hasn't begun has nothing to compact.
pub(super) fn commands_matching(query: &str, listed: &[Command], begun: bool) -> Vec<CommandRow> {
    let query = query.to_lowercase();
    let has_it = |name: &str| name.to_lowercase().contains(&query);
    let own = (OWN.iter())
        .filter(|(name, _, own)| has_it(name) && (begun || *own != Own::Compact))
        .map(|&(name, description, own)| CommandRow {
            name: name.into(),
            hint: None,
            description: description.into(),
            own: Some(own),
        });
    let theirs = (listed.iter())
        .filter(|command| has_it(&command.name) && !OWN.iter().any(|(name, _, _)| *name == command.name))
        .map(|command| CommandRow {
            name: command.name.clone().into(),
            hint: command.argument_hint.clone().map(Into::into),
            description: command.description.clone().into(),
            own: None,
        });
    let mut rows: Vec<CommandRow> = own.chain(theirs).collect();
    rows.sort_by_cached_key(|row| !row.name.to_lowercase().starts_with(&query));
    rows
}

/// How a file at `path` is named to the agent, with a space to go on typing after. A name with
/// spaces in it goes in quotes, or it would stop at the first.
pub(super) fn naming(path: &str) -> String {
    if path.contains(char::is_whitespace) {
        format!("@\"{path}\" ")
    } else {
        format!("@{path} ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(text: &str, cursor: usize) -> Option<(TokenKind, Range<usize>, String)> {
        token_at(text, cursor).map(|token| (token.kind, token.range, token.query))
    }

    #[test]
    fn a_word_that_starts_with_an_at_names_a_file_wherever_it_is() {
        assert_eq!(token("@", 1), Some((TokenKind::File, 0..1, String::new())));
        assert_eq!(token("see @src/ma", 11), Some((TokenKind::File, 4..11, "src/ma".into())));
        assert_eq!(token("a\n@b c", 4), Some((TokenKind::File, 2..4, "b".into())));
    }

    #[test]
    fn all_of_the_word_goes_though_only_what_is_before_the_caret_is_looked_for() {
        assert_eq!(token("@main.rs next", 3), Some((TokenKind::File, 0..8, "ma".into())));
    }

    #[test]
    fn an_at_in_a_word_is_an_address() {
        assert_eq!(token("me@example.com", 14), None);
    }

    #[test]
    fn a_slash_is_a_command_only_as_the_first_word() {
        assert_eq!(token("/rev", 4), Some((TokenKind::Command, 0..4, "rev".into())));
        assert_eq!(token("/review this", 12), None);
        assert_eq!(token("see /usr", 8), None);
    }

    #[test]
    fn a_caret_after_the_word_is_not_in_it() {
        assert_eq!(token("@main ", 6), None);
        assert_eq!(token("", 0), None);
    }

    #[test]
    fn a_caret_inside_a_character_finds_nothing() {
        assert_eq!(token("é", 1), None);
    }

    #[test]
    fn a_word_of_other_scripts_is_measured_in_bytes() {
        assert_eq!(token("é @日本", 10), Some((TokenKind::File, 3..10, "日本".into())));
    }

    fn command(name: &str) -> Command {
        Command {
            name: name.into(),
            description: format!("All about {name}"),
            argument_hint: None,
        }
    }

    fn names(rows: &[CommandRow]) -> Vec<&str> {
        rows.iter().map(|row| row.name.as_ref()).collect()
    }

    #[test]
    fn koyos_own_commands_come_first_and_stand_in_for_the_agents() {
        let listed = [command("review"), command("compact")];
        let rows = commands_matching("", &listed, true);
        assert_eq!(names(&rows), ["model", "effort", "plan", "compact", "clear", "review"]);
        assert_eq!(rows.get(3).and_then(|row| row.own), Some(Own::Compact));
    }

    #[test]
    fn a_chat_that_has_not_begun_has_nothing_to_compact() {
        assert_eq!(names(&commands_matching("", &[], false)), ["model", "effort", "plan", "clear"]);
    }

    #[test]
    fn a_name_that_starts_with_it_goes_ahead_of_one_that_only_has_it() {
        let listed = [command("code-review"), command("review")];
        assert_eq!(names(&commands_matching("rev", &listed, true)), ["review", "code-review"]);
        assert_eq!(names(&commands_matching("REV", &listed, true)), ["review", "code-review"]);
    }

    #[test]
    fn a_file_with_spaces_in_its_name_is_quoted() {
        assert_eq!(naming("src/main.rs"), "@src/main.rs ");
        assert_eq!(naming("my notes/a b.md"), "@\"my notes/a b.md\" ");
    }
}
