# Chats

A chat is a tab in which the person talks to an agent. It has a timeline of turns above a
composer: the person types, presses Return to send (⇧Return adds a line, ⌘Return queues while a
turn runs), and the agent's answer streams into the timeline. ⌘. or Escape stops a turn. In the
test instance the agent is the fake provider (`KOYO_AGENT_PROVIDER=fake`, set in the bundle),
which answers `You said: <text>`, so no Claude session runs and nothing reaches `~/.claude`.

## Sub-features

- `chat-new`: a new chat opens from the launcher (⌘T, then "Ask a new chat…") with the
  keyboard in its composer.
- `chat-compose`: text typed into the composer shows there. Return sends it.
- `chat-answer`: the agent's answer appears in the timeline, and the chat goes from `working`
  back to `idle`.
- `chat-queue`: ⌘Return while a turn runs queues the message.
- `chat-stop`: ⌘. or Escape stops a running turn.
- `chat-recall`: ↑ in an empty composer recalls the last prompt.
- `chat-plan`: ⇧Tab toggles plan mode in the composer's footer.
- `chat-look`: how the chat looks (its backdrop, its width, its markdown).

## How to get to it (user POV)

- ⌘T, then type and pick "Ask a new chat…". Or the new-tab button, then the chat kind.
- A chat's row in the sidebar or its tab, to open an existing one.
- In the composer: Return, ⇧Return, ⌘Return, ↑, ⇧Tab, ⌃M (model menu), ⌘., Escape.
- App control: `chat_create`, `chat_send`, `chat_read`, `chat_list`, `chat_open`,
  `chat_stop`, `chat_set`, `chat_choices`.

## Driving it with koyo-verify

Preconditions: the baseline, and the window made key and main (SKILL.md, Keyboard focus).
`$B probe state` prints `key true, main true` before every send.

- **New chat from the launcher.** Press ⌘T, type, pick the chat kind and send:
  `$B probe 'key 17 t cmd' 'wait 400' 'type hello from the launcher' 'wait 200'`, then
  `$B shot chat-launcher`. The shot shows the launcher with "Ask a new chat…". Press Return with
  `$B probe 'key 36 "\r"' 'wait 1500' state`. A chat tab opens. `state` prints
  `first responder GPUIView`, and `$B app chat_list '{}'` lists one chat.
- **The fake agent answers.** `$B app chat_read '{"chat":"<id>"}'` shows the user's message and
  the assistant's `You said: hello from the launcher`. `status` is `idle`.
- **Type in the composer.** Click in the composer (find it with a shot: the box at the foot of
  the chat), check the point with `hover`, then
  `$B probe 'click <x> <y>' 'type second message' 'wait 200' state`, and `$B shot chat-typed`.
  The shot shows `second message` in the composer, and `state` prints `GPUIView`, key true.
  Send it with `$B probe 'key 36 "\r"' 'wait 1500'`. `chat_read` now has two turns.
- **Through app control.** `$B app chat_create '{"text":"made by app control","select":true}'`
  answers `{chat, place, runsWith}`. `$B app chat_send '{"chat":"<id>","text":"again"}'`
  answers `{turn}`. `chat_read` shows `You said: again`.
- **Stop.** Send `Keep working in the background` (the fake reports a background task), then
  press ⌘. (`$B probe 'key 47 . cmd'`). Alternatively, `$B app chat_stop '{"chat":"<id>"}'`.
  `chat_list` shows the chat `idle`.
- **Show your work.** Text that starts with `Show your work` makes the fake emit tool work, an
  edit and a plan, so the timeline's rows for them can be shot.
- **How it looks.** For a visual change (a backdrop, a width), take `$B shot chat-<state>` of the
  chat tab before and after, at the same window size. Add `$B probe windows` to the transcript
  so the size is on record.
- **Proof.** The `chat_read` answers in the transcript, a shot of the timeline with the answer,
  and `state` after each focus change.

## Gotchas

- **A turn that ends while Koyo has no main window posts a macOS notification**
  (`chat/notify.rs`). The first one makes macOS ask the owner, on their own screen, whether
  KoyoVerify may send notifications, and records `test.koyo.verify` in Notification Center. On
  the agent desktop the window is main only after `desktop_focus`. `$B app chat_create` and
  `chat_send` refuse unless `probe state` prints `main true`. Return pressed through the probe
  isn't guarded, so check `state` yourself before sending with a key. If a prompt did go to the
  owner, tell them.
- Selecting a chat tab (`tab_select`, `pane_focus`, `chat_open`, a click on its row) leaves the
  keyboard at the window root, not in the composer. Only a new chat from the launcher, or a
  press in the composer, puts the keyboard there. Type only after `state` confirms it.
- `chat_create` without `"select": true` doesn't show the chat.
- With the fake provider, `chat_choices` lists no models, and the tab's title stays "New chat".
  Don't pass `model`.
- Approvals, questions and plans can't be answered through app control. Answer them with keys
  in the window (⌘Return allows, ⌘⌫ denies).
- The agent host outlives the app (it exits after 10 idle minutes) and reads the provider only
  as it starts. `$B stop` stops it. A host left from an earlier run with the same data folder
  would keep its provider, which is why every run starts from an empty data folder.
- `chat_read` returns user messages and the assistant's text only. Tool rows and plans are
  visible only in a shot.
- ⌘. is `key 47 . cmd` (47 is the period key).
