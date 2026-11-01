//! What the server tells agents about using it (`docs/terminals.md` 8.7), shipped in the
//! binary so it always matches the daemon it runs with.

/// The MCP `instructions`.
pub const INSTRUCTIONS: &str = "\
Terminals in the worktree you run in, kept by Colosso's terminal daemon; the person sees \
them in the app's tabs and can take any of them over.

The loop: terminal_open (or terminal_list to find one), then terminal_send with \
`then: { wait: { for: \"prompt\" }, read: {} }` to act, wait and read in one call. \
Prefer waiting for text, the prompt or quiet over fixed sleeps, and never poll faster \
than once a second. A call that names no terminal means the one you last acted on.

Read `mode: \"screen\"` for what a full-screen program shows, `mode: \"stream\"` for a \
command's output from a cursor on. terminal_log reads what happened: actions, commands \
with exit codes, and notices.

Writing needs control: terminal_open claims a free terminal, and terminal_send claims one \
no one holds. `person_has_control` means the person took the terminal: do not wait for \
it, do not retry, do not claim with takeover. Read it if you need its output, and open \
or pick another terminal for your own work; return to it only when the person gives it \
back or tells you to.";

/// The prompt's name.
pub const GUIDE_NAME: &str = "terminal_guide";

/// The `terminal_guide` prompt: how to work in the terminals, at length.
pub const GUIDE: &str = "\
# Working in Colosso's terminals

The terminals belong to the worktree you run in. The person sees each one as a tab in \
Colosso and can type into any of them at any moment.

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

## What happened

terminal_log reads a terminal's logs from a `cursor` on: `activity` (who did what), \
`commands` (each command with its exit code and duration; `failed: true` keeps the \
failures), and `notices`. Resources say the same for clients that read them: \
`colosso://terminals/{handle}/screen` and `/activity`, which can be subscribed to.

## Errors

An error has a `code`, a `message` and `retryable`. Retry only what is retryable. Give \
mutating calls a `requestId` when you might send them twice: the same id with the same \
arguments returns the first answer instead of acting again.

## Ending

terminal_close closes a terminal whose program has ended; one still running needs \
stopping first (terminal_signal, or terminal_send with `interrupt`) or the person.";
