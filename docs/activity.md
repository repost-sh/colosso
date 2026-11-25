# Activity

What goes on in a chat, as Koyo says it on rows, tabs and marks: whether it works, and whether it
has something for the person. This file is the reference for that state, from the agent's
harness to what's drawn, and says what a new harness has to report. Decisions are marked
**[asked, date]** (the owner asked for it) or **[ours]**.

## 1. What's said

Two things are said, and neither hides the other. **[asked, 2026-11-25]**

| | Values | Drawn as |
|---|---|---|
| **Work** | an agent's at it: the chat's turn, or any subagent | the spinner, amber |
| | only tasks in the background: a shell, a Monitor | the spinner, faded amber |
| | nothing | nothing |
| **Attention** | it needs the person (approval, question, plan), it failed, or it finished unseen | a dot before the spinner |
| | nothing | nothing |

- **An agent at it comes before the background.** While a turn or a subagent runs, a shell
  running too changes nothing drawn. **[asked, 2026-11-25]** A subagent running past its turn is
  an agent at it, so it has the full spinner. **[ours]**
- **How many subagents work** is written up and right of the spinner, as an exponent is: `x3`.
  It's every one at work, those that subagents started among them. **[asked, 2026-11-25]** A
  space's mark is too small for it and shows the spinner alone. **[ours]**
- **A turn that waits for the person isn't work**: the dot says it. What runs beside it still is.
- **Put together**, chats say all of their work and the most urgent attention: subagents and
  tasks are summed, a turn is any turn. A tab says its panes' chats, a branch its tabs', a project
  its branches', a space its projects'.
- **Said once**, on the last row in sight (`docs/sidebar.md` 5). **[asked, 2026-11-25]**
- **A chat a person closed** says its work, and that it needs them, and nothing else: that it
  finished or failed waits until it's opened.
- **A terminal that waits for the person** is attention too, on the rows over its tab.

On a sidebar row the dot is indigo whatever it's for; on a rail tab its colour says which: the
text's for "needs you", red for failed, muted for finished. The spinner stays in the middle of the
rows' one column, the dot to its left and the count to its right; a dot alone takes the middle.

## 2. Where it lives

Each step has one type, and the next reads only that.

| Step | Type | Where |
|---|---|---|
| A harness says what happens | `DriverEvent` | `koyo-agent-host/src/providers/provider.ts` |
| The host keeps it for a chat | `beside`, the runs, the requests | `threads/state.ts`, `threads/subagents.ts` |
| Lists are told | `ThreadSummary`: `status`, `needs`, `beside`, `unread` | `threads/rules.ts` |
| An open chat is told | `thread.beside`, and the open's snapshot | `threads/state.ts` |
| The app reads the work | `Work`, `Pace` | `koyo-agent/src/work.rs` |
| The app adds what's for the person | `Activity`, `Attention` | `koyo/src/chat/activity.rs` |
| A place puts its chats together | `tab_activity`, `checkout_status`, `project_activity`, `space_activity` | `koyo/src/chat/status.rs`, `shell/spaces.rs` |
| It's drawn | `Sign`, `spinner` | `koyo/src/chat/sign.rs`, `chat/spinner.rs` |

`Beside` is what an agent runs beside its turn: `subagents` at work and `tasks` in the background.
It's the live session's, so the host holds it in memory and never stores it: a host that starts
again has nothing beside any turn, with nothing to clear.
Nothing past the summary knows which harness a chat runs on.

`Work` is read off an open chat (`Replica::work`) the same way as off its summary, so the
composer's working edge, the progress card and quitting agree with the rows.

To say something new, add it to `Work` or `Attention`, with its rank and how it adds up, and the
places follow. A place that shows chats reads its `Activity` and draws it by `Sign::new`, given its
work and its dot's colour; a space's mark, which lays the two out round a dot 7 points wide, draws
the same spinner and dot itself (`shell/sidebar/foot.rs`).

## 3. What a harness reports

A harness is a `Provider` (`providers/provider.ts`) that turns its agent's own messages into
`DriverEvent`s. These are all that activity comes from; every one is the harness's whole duty here,
and nothing in the host or the app needs to change for it.

| It happens | The harness reports |
|---|---|
| A turn's prompt was taken | `turn.started` |
| The agent started a turn itself, as a task it waited on ended | `turn.adopted` |
| The turn ended | `turn.finished`, with its outcome |
| The agent waits on the person | `SessionSink.ask`, or `request` for something it stopped at |
| A subagent started, or one that had ended goes on again | `subagent.started` with its `key`, and `parentKey` if a subagent started it |
| A subagent ended, however it did | `subagent.ended` |
| The number of tasks in the background changed | `background` with how many run now |
| The process went | `session.ended` |

Three rules make it right:

1. **`background` is a level, not a change.** It carries the number running now, 0 once there are
   none. A harness that's only told of starts and ends counts them itself.
2. **A subagent is never a background task.** It's said by `subagent.*` alone, past its turn or
   not. A harness whose list of background work has both leaves the subagents out, as Claude's does
   (`providers/claude/background.ts`).
3. **Housekeeping isn't work.** What the agent didn't start, as a watcher of the harness's own,
   isn't reported.

The host does the rest: it counts subagents by their keys, has both at nothing once the session
goes, and keeps a session with anything beside its turn open
(`docs/chat.md` 4.5.6).

A harness without subagents or background tasks never reports them, and its chats say turns alone.

To test one: drive its session through the table above, and look at what its sink got
(`test/background.test.ts` and `test/claude/subagents.test.ts` do for Claude). What the host makes
of the events is tested once for all of them, with the fake's session (`test/background.test.ts`,
"a chat's summary").
