<p align="center">
  <img src="koyo-rs/koyo/icons/icon.png" width="128" alt="Koyo">
</p>

<h1 align="center">Koyo</h1>

<p align="center">
  One window for the work of a branch: its terminals, its agents, its files, and a real browser.
</p>

<p align="center">
  <a href="https://github.com/repost-sh/koyo/releases/latest">Download for macOS</a>
  ·
  <a href="docs/">Documentation</a>
</p>

Koyo is a native macOS app for people who work with coding agents. Every project is a sidebar of branches, and each branch has its own tabs: terminals, Claude chats, files, the branch's changes, and browser pages. Agents drive the same window through one MCP server, so what an agent opens is what the person sees.

## Installation

```sh
open https://github.com/repost-sh/koyo/releases/latest   # download Koyo-<version>.dmg
```

Open the disk image and drag Koyo to Applications. Koyo is signed with a Developer ID and notarized by Apple.

Koyo updates itself. A released Koyo looks for a newer release at launch and every hour, and a pill in the top bar installs it in one click. See [Updates](docs/updates.md).

### Requirements

| Requirement | Value |
|---|---|
| macOS | 13.0 or later |
| Processor | Apple silicon |

## Features

| Feature | What it does | Reference |
|---|---|---|
| Layout | A sidebar of projects and their branches, a top rail, and split panes between them. Spaces group projects. | [Layout](docs/layout.md) |
| Terminals | Long-lived terminals owned by a daemon, one set per branch. Quitting Koyo leaves them running. | [Terminals](docs/terminals.md) |
| Chats | Claude in a tab, working in the branch's checkout, through Anthropic's Agent SDK. A running turn outlives the window. | [Chat](docs/chat.md) |
| Browser | Chromium pages as tabs, with profiles, extensions, DevTools, downloads, and H.264 and AAC. | [Browser](docs/browser.md) |
| Responsive mode | A page at a phone's or a tablet's size, with touch and the device's user agent. | [Responsive](docs/responsive.md) |
| Files and changes | An editor with language servers, a markdown preview, and the branch's diff. | [Layout](docs/layout.md) |
| Agent desktop | A macOS desktop of the agent's own, where it opens and uses apps while the person keeps working. | [Agent desktop](docs/agent-desktop.md) |
| Devices | iPhone and iPad simulators, and devices on a cable, live in a tab. | [Devices](docs/devices.md) |
| Claude accounts | Several Claude Code logins, with each one's usage, switched by hand or at a threshold. | [Claude accounts](docs/claude-accounts.md) |
| App control | One MCP server through which an agent sees and drives all of the above. | [App control](docs/app-control.md) |

## Keyboard

| Shortcut | Action |
|---|---|
| `⌘T` | Open the launcher for a new tab of any kind |
| `⌘L` | Change the address of the page shown, or open a page when no tab is open |
| `⌘P` | Open a file |
| `⌃⇥` | Switch tabs |
| `⌘⇧T` | Reopen the tab closed last |

See [Keyboard](docs/keyboard.md) for which pane has the keys.

## Agents and MCP

An agent in a Koyo terminal or chat reaches the app without setup. Any other MCP client on the Mac connects once **Settings → MCP server** is on.

| Transport | Address |
|---|---|
| Streamable HTTP | `http://127.0.0.1:6272/mcp` |
| stdio | `/Applications/Koyo.app/Contents/MacOS/koyo mcp` |

The server is off by default and listens on `127.0.0.1` only. It lists tools for projects, worktrees, tabs, panes, browser pages, files, terminals, the agent desktop, devices, and settings. Koyo asks the person before an agent deletes or discards anything.

```sh
/Applications/Koyo.app/Contents/MacOS/koyo app help          # the same operations from a shell
/Applications/Koyo.app/Contents/MacOS/koyo app tab_list
```

See [App control](docs/app-control.md) for the catalog and the permission scopes.

## Building from source

Building needs Xcode and [rustup](https://rustup.rs). Every other tool (Bun, Zig, CMake, Ninja, and the Chromium Embedded Framework) is fetched into `.tools/` inside the checkout on first use.

```sh
git clone https://github.com/repost-sh/koyo.git
cd koyo
git config core.hooksPath script/hooks    # the scripts refuse to build without the repository's hooks
script/dev-bundle                          # build the debug app into a bundle and run it
```

| Command | What it does |
|---|---|
| `script/dev-bundle` | Builds the debug app with the browser engine inside a bundle, and runs it. Pages need the bundle. |
| `script/dev` | Runs with hot reload. Edits to render code apply without a restart. This build shows no pages. |
| `script/lint` | Clippy, the repository's own hygiene rules, `tsc`, and ESLint. Git runs it before each commit. |
| `script/check` | `script/lint`, then every test, including the ones that start the real browser engine. |
| `script/bundle` | Builds `Koyo.app`, a release build, at `koyo-rs/target/bundle/`. |
| `script/cef` | Builds Chromium from source with H.264, AAC, and Koyo's patches. |

A debug build runs on the framework that CEF publishes, which plays no H.264 and shows no extension icons. `script/bundle` packages only a framework that `script/cef` built. That build takes 90 GB of disk and about three hours.

See [How to release Koyo](docs/releasing.md) for signed releases.

## Repository layout

| Path | Contents |
|---|---|
| `koyo-rs/` | The Cargo workspace: the app (`koyo`), the terminal daemon, the browser host, the MCP servers, and their libraries. |
| `koyo-js/` | The chat's agent host, in TypeScript, compiled to one executable. |
| `script/` | Every build, check, and release command. |
| `docs/` | The specifications. Each feature has one, and it is kept in step with the code. |
| `distribution/` | Terraform for the domain and for what releases are served from. |
| `AGENTS.md` | The rules a change must pass, each one enforced by `script/lint`. |

## License

Apache-2.0. See [LICENSE](LICENSE).
