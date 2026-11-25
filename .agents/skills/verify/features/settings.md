# Settings

Settings open in the window as pages: General, Chat, Accounts, Appearance, Editor, Languages,
Browser, Keyboard, MCP server, Agent desktop, Devices and About. A change made on a page applies
at once and is saved to `settings.json`, and an edit of that file applies within 2 s while Koyo is in front, or as it comes back in front.

## Sub-features

- `settings-open`: each page opens, and the Settings tab shows it.
- `settings-change`: a change on a page (appearance, Tabs and Address bar, preview files…)
  applies to the window and lands in `settings.json`.
- `settings-agent`: `settings_get` / `settings_set` read and change the settings agents may
  change.
- `settings-file`: an edit of `settings.json` from outside applies while the app runs.
- `settings-keyboard`: the Keyboard page lists the shortcuts.

## How to get to it (user POV)

- ⌘, or the app menu's Settings… (a native menu: use the key).
- The pages' list in the Settings tab.
- App control: `settings_open {"page":…}`, `settings_get`, `settings_set`.

## Driving it with koyo-verify

Preconditions: the baseline. The test settings are `.superpowers/v/c/settings.json`, as
`$B prepare` seeded them.

- **Open a page.** `$B app settings_open '{"page":"appearance"}'`, then `$B shot settings-appearance`.
  The page shows. Try `general`, `chat`, `browser`, `keyboard` and `mcp` in turn.
- **Read and change through app control.** `$B app settings_get '{}'` lists the agent-settable
  fields. `$B app settings_set '{"appearance":"light"}'` turns the window light (take a shot), and
  `jq .appearance .superpowers/v/c/settings.json` prints `"Light"`. Set it back with
  `'{"appearance":"system"}'`.
- **Change by hand.** On a settings page, click the control (measured from a shot and checked
  with `hover`). Read the file afterwards: the field changed within half a second.
- **The file from outside.** Write a field with `jq` (`.sidebar_width = 320`). Within 2 s
  the window follows (take a shot), and `settings_get` agrees.
- **Proof.** A shot of the page before and after, the `settings_get` answer, and the `jq` line
  of the file.

## Gotchas

- On disk the keys are Rust's snake_case names (`agent_desktop`, `on_quit`, `worktrees_dir`,
  `address_bar`), with camelCase inside `externalControl` and `accounts`. The layout's values are
  `"TopRail"` / `"Sidebar"` for `tabs`, `"Sidebar"` / `"Page"` for `address_bar`. `settings_set`
  takes camelCase (`sidebarWidth`, `previewFiles`).
- A file that doesn't parse is dropped whole, and the app runs on defaults (external control
  off, accounts managed, the agent desktop's switch at its default). Never break the test
  file by hand: check every edit with `jq . file` before you save it.
- Don't touch `agent_desktop`, `externalControl` or `accounts` in the test settings. Turning
  the agent desktop on in a test Koyo would start a second desktop host and make another
  desktop in Mission Control. The MCP server page shows the `koyo-verify` client: leave it.
- The MCP server, Agent desktop and Devices pages describe the test instance, not the owner's
  Koyo. Nothing changed there reaches the owner's.
