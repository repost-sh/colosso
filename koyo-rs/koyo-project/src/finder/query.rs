//! What's typed into Go to File: a fuzzy pattern, in fzf's syntax, and perhaps a line to go
//! to after a colon at its end, `main.rs:42`.

/// A query as typed, split into what's matched and the line asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    /// The pattern the paths are matched against, trimmed.
    pub text: String,
    /// The line after a trailing `:`, if digits follow it.
    pub line: Option<u32>,
}

impl Query {
    pub fn parse(typed: &str) -> Query {
        let (text, line) = match typed.rsplit_once(':') {
            Some((text, digits))
                if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                (text, digits.parse().ok())
            }
            // A colon with nothing after it yet is the line about to be typed.
            Some((text, "")) => (text, None),
            _ => (typed, None),
        };
        Query {
            text: text.trim().to_owned(),
            line,
        }
    }
}

/// Whether every path `new` matches is among those `old` matched, so only those need
/// matching again. Typing on narrows, except after an atom that excludes (`!`), anchors at
/// the end (`$`) or escapes (`\`), where more text can match more.
pub(super) fn narrows(old: &str, new: &str) -> bool {
    new.starts_with(old) && !old.contains(['!', '$', '\\'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(typed: &str) -> (String, Option<u32>) {
        let query = Query::parse(typed);
        (query.text, query.line)
    }

    #[test]
    fn a_line_follows_a_trailing_colon() {
        assert_eq!(parsed("main.rs:42"), ("main.rs".into(), Some(42)));
        assert_eq!(parsed("main.rs:"), ("main.rs".into(), None));
        assert_eq!(parsed(":7"), (String::new(), Some(7)));
        assert_eq!(parsed("a:b"), ("a:b".into(), None));
        assert_eq!(parsed(" src main "), ("src main".into(), None));
    }

    #[test]
    fn typing_on_narrows_unless_an_atom_excludes() {
        assert!(narrows("mai", "main"));
        assert!(narrows("src", "src ma"));
        assert!(narrows("^ma", "^main"));
        assert!(!narrows("main", "mai"));
        assert!(!narrows("!ma", "!main"));
        assert!(!narrows("rs$", "rs$x"));
    }
}
