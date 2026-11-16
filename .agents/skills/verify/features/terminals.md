# Terminals

A terminal is a tab running a shell or a command in a worktree, kept alive by Koyo's terminal
daemon (`koyo termd`) when its tab closes or the app quits with "Keep". The person types in
it, reads it, scrolls it, splits it beside other panes and closes it. Agents read and drive
terminals through the `terminal_*` tools.

## Sub-features

- `term-open`: a terminal tab opens in a worktree with a command, and its screen shows the
  prompt.
- `term-type`: keys typed in the window reach the shell, and its output shows.
- `term-tools`: `terminal_send` / `terminal_read` / `terminal_wait` act and read as an agent
  does.
- `term-kept`: closing the tab leaves the terminal running, and it can be shown again.
- `term-end`: quitting with `on_quit: "End"` ends the terminals.

## How to get to it (user POV)

- ⌘T, then the terminal kind. Or the sidebar's new-terminal button.
- Typing in a shown terminal, scrolling, ⌘K / ⌘F where bound.
- App control: `tab_open {"kind":"terminal"}`. The `koyo mcp` tools: `terminal_open`,
  `terminal_send`, `terminal_read`, `terminal_wait`, `terminal_list`, `terminal_close`.

## Driving it with koyo-verify

Preconditions: the baseline. Always give a command: `["/bin/zsh","-f"]` reads no rc file and
saves no history.

- **Open.** `$B mcp terminal_open '{"name":"verify","command":["/bin/zsh","-f"]}'` answers with
  the terminal's handle. `$B app tab_list '{}'` lists its tab. Alternatively,
  `$B app tab_open '{"kind":"terminal","command":["/bin/zsh","-f"],"select":true}'`.
- **Send and read as an agent.**
  `$B mcp terminal_send '{"terminal":"<handle>","text":"echo verify-$((6*7))","enter":true,"then":{"wait":{"for":"text","pattern":"verify-42"},"read":{"mode":"screen"}}}'`.
  The screen holds `verify-42`.
- **Type as a person.** Show the tab (`$B app tab_select '{"pane":<pane>}'`), make the window
  key, click in the terminal, then
  `$B probe 'type echo typed-by-hand' 'key 36 "\r"' 'wait 500'`. Read it back with
  `$B mcp terminal_read '{"terminal":"<handle>","mode":"screen"}'`, which holds
  `typed-by-hand`, and take `$B shot term-typed`.
- **Kept when its tab closes.** `$B app tab_close '{"pane":<pane>}'`. Then
  `$B mcp terminal_list '{}'` still lists it as running.
- **Proof.** The `terminal_read` answers in the transcript, and a shot of the screen after the
  typed command.

## Gotchas

- `koyo app` has no terminal operations. Terminals are `$B mcp` (the `koyo mcp` relay), which
  finds the test daemon through `KOYO_DATA_DIR`.
- Shells in test terminals don't get `KOYO_DATA_DIR`, but they do get `KOYO_TERMINAL`. A `koyo …`
  run inside one reaches the owner's Koyo. Never run `koyo`, `claude` or anything that pushes
  from a test terminal.
- The daemon outlives the app. `$B stop` stops the processes that carry the instance's data
  folder, the daemon among them. Its log is under `.superpowers/v/d/term/`, and `stop` copies
  it into the run.
- Not proven by a run yet: whether `koyo mcp`'s terminal tools ask for the client's approval
  the way `koyo app` does. If `$B mcp` answers `control_disabled` or `user_declined`, record
  the answer in this file.
