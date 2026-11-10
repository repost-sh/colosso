//! What the server tells agents about using it (`docs/app-control.md` 8, `docs/terminals.md`
//! 8.7), shipped in the binary so it always matches the app and daemon it runs with: the
//! MCP `instructions`, and the `koyo_guide` and `terminal_guide` prompts.

use rmcp::ErrorData;
use rmcp::model::{GetPromptResult, Prompt, PromptMessage, Role};

/// The MCP `instructions`.
pub const INSTRUCTIONS: &str = "\
You are connected to Koyo, the app the person works in. These tools see and drive its \
window as the person would: projects, worktrees, tabs, split panes, files, browser pages, \
panels, layout and settings, and the terminals of the worktree you run in. Changes appear \
in the window as if the person made them. The desktop_* tools open and drive apps on the \
agent desktop, a desktop of your own on the person's Mac that one Koyo tab shows. Drive Koyo only through these tools: never \
edit its settings, data or other files.

Start with app_status (what the window shows and what is selected) or project_list (the \
projects and their worktrees). Pane ids come from tab_list; the pane, tab and browser \
tools take them. Changing tools accept a `requestId`: a repeat with the same id returns \
the first result instead of acting again.

The terminal loop: terminal_open (or terminal_list to find one), then terminal_send with \
`then: { wait: { for: \"prompt\" }, read: {} }` to act, wait and read in one call. \
Prefer waiting for text, the prompt or quiet over fixed sleeps, and never poll faster \
than once a second. A call that names no terminal means the one you last acted on. Read \
`mode: \"screen\"` for what a full-screen program shows, `mode: \"stream\"` for a \
command's output from a cursor on; terminal_log reads what happened.

The person comes first. `person_has_control` means the person took that terminal or \
browser page, is using the agent desktop, or runs the app you asked for as theirs: do not \
take it over, and do not retry at once (the agent desktop comes back a few seconds after \
they stop; a terminal, a page or an app of theirs stays theirs). Read it if you need \
its content, and open or pick another terminal or page for your own work; return to it \
only when the person gives it back or tells you to. What you make or open (project_add, \
worktree_create, tab_open, pane_split, file_open, changes_open, chat_create) works whether \
it's shown or not. With `select` left out, the person's setting in Settings → MCP server \
decides whether the window switches to it; by default it doesn't, and it's only listed. \
Pass `select: true` only when the person asked to see it.

Failures are results with `{ code, message, retryable }`. `app_not_running` means \
Koyo isn't running: call app_launch, then try again. `control_disabled` means the \
person has to turn on Settings → MCP server (for a chat, Settings → Chat → Koyo tools), \
`scope_denied` that the person hasn't \
allowed this client that kind of change, and `user_declined` that the person refused (or \
didn't answer within two minutes) the confirmation Koyo shows before deleting or \
discarding, so nothing changed. The koyo_guide and terminal_guide prompts say more.";

/// The app guide's name.
pub const KOYO_GUIDE_NAME: &str = "koyo_guide";

/// The `koyo_guide` prompt: how to drive the app, at length.
pub const KOYO_GUIDE: &str = include_str!("guide/koyo.md");

/// The prompt's name.
pub const GUIDE_NAME: &str = "terminal_guide";

/// The `terminal_guide` prompt: how to work in the terminals, at length.
pub const GUIDE: &str = "\
# Working in Koyo's terminals

The terminals belong to the worktree you run in. The person sees each one as a tab in \
Koyo and can type into any of them at any moment.

## The loop

1. Find or make a terminal: terminal_list, or terminal_open with a `name` (opening a \
name that exists returns that terminal, `existed: true`).
2. Act, wait and read in one call: terminal_send with `text` and `enter: true`, and \
`then: { wait: { for: \"prompt\" }, read: {} }`. The answer says whether the command was \
seen to run (`submission`), whether the wait was met, and what the screen shows.
3. Read again when you need to: terminal_read for the screen, `mode: \"stream\"` with \
the last `nextCursor` for output since then.

A call that names no terminal means the one you last acted on, else the only one in your \
worktree. Name terminals by `name` or by their `term_…` handle.

## Waiting

terminal_wait, or `then.wait` on a send, waits for: `prompt` (the shell is back), `idle` \
(no program running), `text` (a regular expression on the screen, `pattern`), `gone` (that \
text went away), `stable` (the screen stopped changing for `stableMs`), `exit`, \
`needs_input`, `running`, `status_change`, or `control` (no person holds it). Waits time \
out after `timeoutMs` (25 s by default, up to 10 minutes) with `met: false`, which is an \
answer, not an error. Prefer these waits to sleeping, and never poll faster than once a \
second.

## Full-screen programs and agents

Programs that draw the whole screen (editors, `top`, coding agents) are read with \
`mode: \"screen\"`. Send keys by name in `keys` (`[\"ctrl+c\"]`, `[\"escape\"]`, \
`[\"up\", \"enter\"]`), long text with `paste`, then wait for `text` or `stable` and \
read. `interrupt: true` sends Ctrl+C first.

## Control

Writing needs control. terminal_open claims a free terminal for you; terminal_send claims \
one that no one holds; terminal_claim does it outright. Your hold lasts while you act and \
lapses after two minutes without acting; terminal_release gives it up. Another agent's \
terminal answers `control_taken_by`: open your own instead, or claim with `takeover: \
true` only when the work is yours to take over.

`person_has_control` means the person took this terminal, by typing in it or by taking \
control. Do not wait for it, do not retry, do not claim it with takeover. Read it if you \
need its output, and open or pick another terminal for your own work. Return to it only \
after the person gives it back (terminal_control shows who holds it) or tells you to.

## Sizes

terminal_resize pins a terminal at `cols` × `rows` (2 to 1000 each) for every viewer, the \
person's pane included, and answers once the program has redrawn: it waits for the screen \
to hold still 300 ms and reads it (`then: {}` skips both; a wait that runs out answers \
`met: false` with the screen as it is). To check how a program lays out at a size, resize \
it, read its screen (the answer has it), then resize with `fill: true` to give it back \
to the person's pane. terminal_open with `size` opens it pinned at that size. The person \
sees the size you set and can change it, and their change wins; your size goes when your \
connection does.

## What happened

terminal_log reads a terminal's logs from a `cursor` on: `activity` (who did what), \
`commands` (each command with its exit code and duration; `failed: true` keeps the \
failures), and `notices`. Resources say the same for clients that read them: \
`koyo://terminals/{handle}/screen` and `/activity`, which can be subscribed to.

## Errors

An error has a `code`, a `message` and `retryable`. Retry only what is retryable. Give \
mutating calls a `requestId` when you might send them twice: the same id with the same \
arguments returns the first answer instead of acting again.

## Ending

terminal_close closes a terminal whose program has ended; one still running needs \
stopping first (terminal_signal, or terminal_send with `interrupt`) or the person.";

/// The prompts, for `prompts/list`.
pub fn prompts() -> Vec<Prompt> {
    vec![
        Prompt::new(
            KOYO_GUIDE_NAME,
            Some("How to drive Koyo: projects, worktrees, tabs, panes, browser pages, settings"),
            None,
        )
        .with_title("Koyo guide"),
        Prompt::new(
            GUIDE_NAME,
            Some("How to work in Koyo's terminals: the loop, waits, control, logs"),
            None,
        )
        .with_title("Terminal guide"),
    ]
}

/// The prompt called `name`.
pub fn prompt(name: &str) -> Result<GetPromptResult, ErrorData> {
    let (text, description) = match name {
        KOYO_GUIDE_NAME => (KOYO_GUIDE, "How to drive Koyo"),
        GUIDE_NAME => (GUIDE, "How to work in Koyo's terminals"),
        _ => {
            let message = format!("there is no prompt called {name}");
            return Err(ErrorData::invalid_params(message, None));
        }
    };
    let message = PromptMessage::new_text(Role::User, text);
    Ok(GetPromptResult::new(vec![message]).with_description(description))
}
