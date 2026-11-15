//! Who asks for one of the person's pages, as their prompt names it. What a client says of
//! itself in its hello proves nothing (`docs/app-control.md` 4): a chat's id, a terminal's
//! handle and a client's name are its own words. So a prompt says what the caller says it is,
//! and marks it as said, in Koyo's words for that chat or terminal: its title or name, and its
//! worktree as `project/branch`. And the worktree a caller works in, where a copy of a page it
//! asks for opens; and the words of a client or a page made fit to show in a prompt.

use std::path::Path;

use gpui_kit::App;
use koyo_agent::protocol::ids::ThreadId;
use koyo_term_control::Handle;

use super::super::Caller;
use super::super::select::{self, containing};
use crate::shell::Shell;

/// The most characters of a name a prompt shows.
pub(super) const NAME: usize = 60;
/// The most characters of a title a prompt shows.
pub(super) const TITLE: usize = 80;
/// The most characters of a caller's reason a prompt shows.
pub(super) const REASON: usize = 200;

/// The punctuation a prompt keeps of a client's or a page's words, beside letters, digits and
/// spaces. Everything else goes: quote marks of every kind, which could make their words read
/// as Koyo's own, combining marks, symbols, emoji, and control, format and invisible
/// characters.
const KEPT: &str = "-_.,:;!?()[]/&+#@%";

/// Letters that show nothing, as blank as a space but not one, and letters drawn as a quote
/// mark (the saltillo).
const ODD_LETTERS: [char; 6] = ['\u{115F}', '\u{1160}', '\u{3164}', '\u{FFA0}', '\u{A78B}', '\u{A78C}'];

/// Whether `c` is a letter or a digit a prompt shows as one: not a modifier letter, many drawn
/// as quote marks (ˮ ʺ ʼ ʻ), and no combining mark, which stacks over the words round it,
/// whatever Unicode says of it being alphabetic.
fn plain_alphanumeric(c: char) -> bool {
    use unicode_general_category::{GeneralCategory, get_general_category};
    let marked = matches!(
        get_general_category(c),
        GeneralCategory::ModifierLetter
            | GeneralCategory::NonspacingMark
            | GeneralCategory::SpacingMark
            | GeneralCategory::EnclosingMark
    );
    c.is_alphanumeric() && !marked && !ODD_LETTERS.contains(&c)
}

/// `text` as a prompt may show it: letters and digits of any script, spaces and the
/// punctuation of [`KEPT`] only, its spaces single, cut to `most` characters with an ellipsis.
pub(super) fn clean(text: &str, most: usize) -> String {
    let kept: String = (text.chars())
        .filter_map(|c| {
            if c.is_whitespace() {
                Some(' ')
            } else if plain_alphanumeric(c) || KEPT.contains(c) {
                Some(c)
            } else {
                None
            }
        })
        .collect();
    let words = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.chars().count() <= most {
        return words;
    }
    let cut: String = words.chars().take(most.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// The checkout at `(project, branch)`, as a prompt names it: `project/branch`.
pub(super) fn label(shell: &Shell, (project, branch): (usize, usize)) -> String {
    let project = &shell.workspace.projects[project];
    let unit = &project.branches[branch];
    format!("{}/{}", clean(&project.name, NAME), clean(&unit.name(), NAME))
}

/// What the person's prompt calls `caller`, as the start of a sentence: what it says it is,
/// said to be its word, in Koyo's words for it.
pub(super) fn named(caller: &Caller, shell: &Shell, cx: &App) -> String {
    let at = |path: Option<&Path>| {
        let checkout = containing(&shell.workspace, path?)?;
        Some(format!(" ({})", label(shell, checkout)))
    };
    if let Some(id) = &caller.chat {
        let chats = shell.chats().read(cx);
        let Some(chat) = chats.summary(&ThreadId(id.clone())) else {
            return "An agent saying it's from a Koyo chat".to_owned();
        };
        let (title, at) = (clean(&chat.title, TITLE), at(Some(Path::new(&chat.cwd))));
        let at = at.unwrap_or_default();
        return if title.is_empty() {
            format!("An agent saying it's from a Koyo chat{at}")
        } else {
            format!("An agent saying it's from the chat “{title}”{at}")
        };
    }
    if let Some(handle) = &caller.terminal {
        let Some((name, worktree)) = shell.terminal_named(&Handle(handle.clone())) else {
            return "An agent saying it runs in a Koyo terminal".to_owned();
        };
        let at = at(worktree).unwrap_or_default();
        return format!("An agent saying it runs in the terminal “{}”{at}", clean(name, NAME));
    }
    format!(
        "An MCP client calling itself “{}”",
        clean(&caller.client.name, NAME)
    )
}

/// The checkout `caller` works in, as `(project, branch)`: a chat's, the one its folder is in;
/// an agent's in a Koyo terminal, the terminal's; else as its hello says, `KOYO_WORKTREE` or its
/// working folder. `None` for a caller in no checkout Koyo has.
pub(super) fn worktree(caller: &Caller, shell: &Shell, cx: &App) -> Option<(usize, usize)> {
    let workspace = &shell.workspace;
    let known = if let Some(id) = &caller.chat {
        let chats = shell.chats().read(cx);
        let chat = chats.summary(&ThreadId(id.clone()));
        chat.and_then(|chat| containing(workspace, Path::new(&chat.cwd)))
    } else if let Some(handle) = &caller.terminal {
        let named = shell.terminal_named(&Handle(handle.clone()));
        named.and_then(|(_, worktree)| containing(workspace, worktree?))
    } else {
        None
    };
    known.or_else(|| select::worktree(caller, workspace, Some("current"), None).ok())
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn a_clients_or_a_pages_words_keep_only_letters_digits_spaces_and_plain_punctuation() {
        assert_eq!(clean("“Koyo” says:\n\"Allow\"", 60), "Koyo says: Allow");
        assert_eq!(clean("it's\u{202E}gnp.exe\t `x`", 60), "itsgnp.exe x");
        assert_eq!(clean("❝Allow❞ ‛it‟ «now»", 60), "Allow it now");
        assert_eq!(clean("A\u{E0041}\u{200F}B\u{3164}C", 60), "ABC");
        let zalgo = format!("a{}b", "\u{0301}".repeat(50));
        assert_eq!(clean(&zalgo, 60), "ab");
        // Modifier letters drawn as quotes, the saltillo, and marks Unicode calls alphabetic.
        assert_eq!(clean("xˮ The chat ʺFixˮ ʼitʻ ꞌnowꞌ", 60), "x The chat Fix it now");
        let marked = format!("a{}{}{}b", "\u{0345}".repeat(20), "\u{05B0}".repeat(20), "\u{064B}".repeat(10));
        assert_eq!(clean(&marked, 60), "ab");
        assert_eq!(clean("Inbox (3) - you@mail.example.com: 50% #1!", 80), "Inbox (3) - you@mail.example.com: 50% #1!");
        assert_eq!(clean("Écrire à Jürgen 日本語 Ελληνικά", 60), "Écrire à Jürgen 日本語 Ελληνικά");
        assert_eq!(clean("ok 🎉 ✓ ★ ©", 60), "ok");
        assert_eq!(clean("abcdefghij", 5), "abcd…");
        assert_eq!(clean("ab   cd", 60), "ab cd");
    }
}
