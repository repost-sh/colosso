# Koyo verification map

This folder is the maintained source for verifying what a person does in Koyo's window. Read
this index, then use the matching feature file as the recipe. `B` is
`.agents/skills/verify/bin/koyo-verify`, run from the checkout root. Every journey drives the
test instance that [SKILL.md](../SKILL.md) launches, never the owner's Koyo.

## Baseline preconditions

- The dev bundle is built at the commit under test (`nice -n 10 script/dev-bundle --no-run`).
  `$B doctor` shows no `warn` about a stale binary.
- The instance comes from `$B prepare`: an empty data folder, the fixture repository
  `.superpowers/v/fixture/repo` (one commit on `main`) as the only project, the fixture page
  `file://<checkout>/.superpowers/v/fixture/focus.html`, and Koyo's default layout (tabs on the
  top rail, the address bar in the sidebar) unless the journey says `--tabs Sidebar` or
  `--address-bar Page`.
- It was opened with `desktop_open` after the owner's go, and `$B confirm <pid>` passed.
- `$B doctor` exits 0, and `$B run <feature-id>` started the evidence folder.

## Driving conventions

- Start each recipe from the baseline. A recipe that needs more says so under `Preconditions:`.
- Set state up and read it back through `$B app` (or `$B mcp` for terminals). Act as a person
  does through `$B probe` (clicks, keys) or `desktop_*` (the window made key, real pointer
  events).
- Find points from a `$B shot` and check each one with `probe 'hover x y'` before clicking it.
  The points in these files are examples from one window size. Re-measure them on yours.
- After each act, read who has the keyboard (`probe state`) when focus matters, and read the
  state the act should change.
- Commands are literal. JSON goes in single quotes.

## Proof and skip reporting

- Capture the act and the state it leads to: the transcript holds both. Take a shot before and
  after a visual change.
- Check a change from a second view too: `window.json` or `pinned-tabs.json` in
  `.superpowers/v/d`, `chat_read`, a page's own record through `browser_eval` or `probe eval`.
- Name the feature id and the entry point next to each artifact (`$B note`).
- When a path can't be reached (no grant, a native menu that would show on the owner's
  desktop, a precondition the seed lacks), report the command you tried and why it failed. Don't
  count it as verified through another path.

## Feature entry contract

Each feature file starts with an H1 and one paragraph on what the person sees. Then come four
H2s, in this order: `Sub-features` (short ids, one line each), `How to get to it (user POV)`
(every entry point), `Driving it with koyo-verify` (it starts with `Preconditions:`, then each
labelled bullet pairs an act with its command and the result you observe), and `Gotchas`.

## Features

- [Shell](./shell.md): the window, the sidebar and the top rail, projects, tabs, panes, the
  launcher and the tab switcher.
- [Chats](./chats.md): a chat tab, its composer, sending, the fake agent's answer, stopping.
- [Terminals](./terminals.md): a terminal tab, typing in it, reading its screen and stream.
- [Browser pages](./browser-pages.md): native pages, the address field, page acts over CDP,
  screenshots, pinned pages, extensions' controls.
- [Keyboard focus](./keyboard-focus.md): the scripted sequences across pages, the chat
  composer and Koyo's own shortcuts.
- [Settings](./settings.md): the settings pages, settings changed from them and through app
  control, the file they're kept in.

## Proven

| Journey | Last proven | Run |
| --- | --- | --- |
| `page-open`, `page-agent` (`browser_list`), `page-shot` (engine and on-screen), `page-input` (click into the page, `abc` arrived once each), ⌘L opening the address panel | 2026-11-16 at cdcebcf5a | `.superpowers/v/evidence/20261116-123959-proof-browser-pages/` |
| `focus-pinned-page-composer`, the pinned tile | not yet: the window lost key to the address panel and couldn't get it back | same run, transcript's last notes |
