// A provider that runs no agent (docs/chat.md C6), for tests: each session records what the
// actor asked of it and reports what its test tells it to, through the same sink a real
// driver reports to. Given `answer`, its sessions answer each turn by themselves instead, for
// the host run end to end (`KOYO_AGENT_PROVIDER=fake`); a turn that starts `Show your work`
// first does the work Claude would, for the chat's visual check to draw.

import type { Catalog, ProviderStatus } from "../protocol/methods";
import type { Mode, ProviderId, RequestPayload, Response } from "../protocol/model";
import type {
  Capabilities,
  Config,
  DriverEvent,
  FileChanges,
  ItemBody,
  Outcome,
  Provider,
  ProviderSession,
  SessionChange,
  SessionSink,
  SessionSpec,
  TurnInput,
} from "./provider";

const EVERYTHING: Capabilities = {
  steer: true,
  resume: true,
  images: true,
  compact: true,
  plan: true,
};

/** What a session does as a turn starts, beside recording it. */
type OnStart = (session: FakeSession, turn: TurnInput) => void;

interface FakeOptions {
  /** What it can do, everything unless said otherwise. */
  readonly capabilities?: Partial<Capabilities>;
  readonly onStart?: OnStart;
}

export class FakeProvider implements Provider {
  readonly id: ProviderId = "fake";
  readonly capabilities: Capabilities;
  /** Every session opened, the latest last. */
  readonly sessions: FakeSession[] = [];
  /** The reply `title` gives; `undefined` gives none. */
  nextTitle: string | undefined;
  /** Makes `open` fail, as a missing CLI does. */
  failOpen: Error | undefined;
  /** Koyo's settings, as they were last given. */
  config: Config | undefined;
  /** The default mode its own settings name. */
  settingsMode: Mode | null = null;
  /** What `entryBefore` says, or the error it refuses with. */
  entry: string | null | Error = null;
  /** What `filesAt` says, or the error it fails with. */
  changes: FileChanges | null | Error = null;
  /** The prompts whose files were put back, in order. */
  readonly restored: string[] = [];
  /** What `restoreFiles` does to the disk, which may be to fail. */
  onRestore: (() => void) | undefined;
  readonly #onStart: OnStart | undefined;

  constructor(options: FakeOptions = {}) {
    this.capabilities = { ...EVERYTHING, ...options.capabilities };
    this.#onStart = options.onStart;
  }

  setConfig(config: Config): void {
    this.config = config;
  }

  async entryBefore(_sessionId: string, _cwd: string, _promptId: string): Promise<string | null> {
    const { entry } = this;
    return await (entry instanceof Error ? Promise.reject(entry) : Promise.resolve(entry));
  }

  async filesAt(_sessionId: string, _cwd: string, _promptId: string): Promise<FileChanges | null> {
    const { changes } = this;
    return await (changes instanceof Error ? Promise.reject(changes) : Promise.resolve(changes));
  }

  async restoreFiles(_sessionId: string, _cwd: string, promptId: string): Promise<void> {
    this.onRestore?.();
    this.restored.push(promptId);
    await Promise.resolve();
  }

  /** The session opened last. */
  get session(): FakeSession {
    const session = this.sessions.at(-1);
    if (session === undefined) {
      throw new Error("No session was opened.");
    }
    return session;
  }

  async status(_refresh: boolean): Promise<ProviderStatus> {
    return await Promise.resolve({
      provider: this.id,
      state: "ready",
      executable: null,
      version: null,
      account: null,
      message: null,
    });
  }

  async catalog(_cwd: string): Promise<Catalog> {
    return await Promise.resolve({ models: [], commands: [], defaultMode: this.settingsMode });
  }

  open(spec: SessionSpec, sink: SessionSink): ProviderSession {
    if (this.failOpen !== undefined) {
      throw this.failOpen;
    }
    const session = new FakeSession(spec, sink, this.#onStart);
    this.sessions.push(session);
    return session;
  }

  async title(_prompt: string, _cwd: string): Promise<string | undefined> {
    return await Promise.resolve(this.nextTitle);
  }
}

/** A question the session asked, with the answer to come and a way to stop waiting for it. */
export interface Asked {
  readonly answer: Promise<Response>;
  readonly withdraw: () => void;
}

export class FakeSession implements ProviderSession {
  readonly spec: SessionSpec;
  readonly #sink: SessionSink;
  readonly turns: TurnInput[] = [];
  readonly steers: TurnInput[] = [];
  readonly changes: SessionChange[] = [];
  readonly subagentsStopped: string[] = [];
  interrupts = 0;
  killed = false;
  closed = false;
  /** Whether the running turn takes a message; a turn about to end doesn't. */
  steerable = true;
  /** What a settings change does to the session. */
  configures: "applied" | "reopen" = "applied";
  /** Makes `interrupt` fail, as a process that stopped answering does. */
  failInterrupt: Error | undefined;

  readonly #onStart: OnStart | undefined;

  constructor(spec: SessionSpec, sink: SessionSink, onStart?: OnStart) {
    this.spec = spec;
    this.#sink = sink;
    this.#onStart = onStart;
  }

  start(turn: TurnInput): void {
    this.turns.push(turn);
    this.#onStart?.(this, turn);
  }

  steer(turn: TurnInput): boolean {
    if (this.steerable) {
      this.steers.push(turn);
    }
    return this.steerable;
  }

  async interrupt(): Promise<void> {
    this.interrupts += 1;
    await (this.failInterrupt === undefined
      ? Promise.resolve()
      : Promise.reject(this.failInterrupt));
  }

  configure(change: SessionChange): "applied" | "reopen" {
    this.changes.push(change);
    return this.configures;
  }

  kill(): void {
    this.killed = true;
  }

  async close(): Promise<void> {
    this.closed = true;
    await Promise.resolve();
  }

  /** Reports `event`, as a driver does. */
  report(event: DriverEvent): void {
    this.#sink.event(event);
  }

  /** Reports that the latest turn started. */
  started(): void {
    const turn = this.turns.at(-1);
    if (turn === undefined) {
      throw new Error("No turn was started.");
    }
    this.report({ type: "turn.started", promptId: turn.promptId });
  }

  /** Reports an answer starting, named `key`, empty, for text to stream into. */
  startsText(key: string): void {
    const item: ItemBody = { kind: "assistant_text", payload: { text: "" } };
    this.report({ type: "item.started", key, item });
  }

  stopSubagent(key: string): void {
    this.subagentsStopped.push(key);
  }

  finished(outcome: Outcome = "completed"): void {
    this.report({ type: "turn.finished", outcome, failure: null, usage: null });
  }

  ask(payload: RequestPayload, bySubagent = false): Asked {
    const controller = new AbortController();
    return {
      answer: this.#sink.ask(payload, controller.signal, bySubagent),
      withdraw: () => {
        controller.abort();
      },
    };
  }
}

/** What a turn starts with for the fake to do some work before it answers. */
const SHOW_WORK = "Show your work";

type ToolPayload = Extract<ItemBody, { kind: "tool" }>["payload"];

/** The work a turn that asks for it does: it thinks, reads, edits, tests, writes and plans. */
function work(session: FakeSession, turn: number): void {
  const done = (key: string, item: ItemBody, started: ItemBody = item): void => {
    const id = `${key}-${String(turn)}`;
    session.report({ type: "item.started", key: id, item: started });
    session.report({ type: "item.updated", key: id, item, status: "completed" });
  };
  const tool = (key: string, payload: ToolPayload): void => {
    const started = { ...payload, output: null, exitCode: null, patch: null };
    done(key, { kind: "tool", payload }, { kind: "tool", payload: started });
  };
  const base = { input: {}, output: null, exitCode: null, patch: null, files: [], server: null };
  done("thought", {
    kind: "reasoning",
    payload: { text: "The last line goes missing when a file doesn't end in a newline." },
  });
  tool("read", {
    ...base, name: "Read", family: "read", label: "src/lines.rs", files: ["src/lines.rs"],
    output: [
      "1\tpub fn lines(text: &str) -> Vec<&str> {",
      "2\t    text.split_terminator('\\n').collect()",
      "3\t}",
    ].join("\n"),
  });
  tool("search", {
    ...base, name: "Grep", family: "search", label: "split_terminator",
    output: "src/lines.rs:2\nsrc/main.rs:12",
  });
  tool("edit", {
    ...base, name: "Edit", family: "edit", label: "src/lines.rs", files: ["src/lines.rs"],
    patch: [
      {
        oldStart: 1, oldLines: 4, newStart: 1, newLines: 5, lines: [
          " /// The lines of `text`, the last one kept whether or not a newline ends it.",
          " pub fn lines(text: &str) -> Vec<&str> {",
          "-    text.split_terminator('\\n').collect()",
          "+    let trimmed = text.strip_suffix('\\n').unwrap_or(text);",
          "+    trimmed.split('\\n').collect()",
          " }",
        ],
      },
      {
        oldStart: 40, oldLines: 3, newStart: 41, newLines: 3, lines: [
          " #[cfg(test)]",
          "-mod test {",
          "+mod tests {",
          "     use super::*;",
        ],
      },
    ],
  });
  const printed = [
    "   Compiling lines v0.1.0 (/work/lines)",
    "    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.42s",
    "     Running unittests src/lib.rs (target/debug/deps/lines-3f2a)",
    "",
    "running 4 tests",
    "test tests::empty_text_has_no_lines ... ok",
    "test tests::a_trailing_newline_starts_no_line ... ok",
    "test tests::the_last_line_is_kept ... ok",
    "test tests::windows_endings_stay ... ok",
    "",
    "test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
  ];
  tool("command", {
    ...base, name: "Bash", family: "command", label: "cargo test", exitCode: 0,
    output: printed.join("\n"),
  });
  tool("write", {
    ...base, name: "Write", family: "edit", label: "tests/lines.rs", files: ["tests/lines.rs"],
    patch: [{
      oldStart: 0, oldLines: 0, newStart: 1, newLines: 6, lines: [
        "+use lines::lines;",
        "+",
        "+#[test]",
        "+fn the_last_line_is_kept() {",
        "+    assert_eq!(lines(\"a\\nb\"), [\"a\", \"b\"]);",
        "+}",
      ],
    }],
  });
  done("todos", {
    kind: "todos",
    payload: {
      todos: [
        { text: "Keep the last line", status: "completed" },
        { text: "Test it", status: "completed" },
        { text: "Say what changed", status: "in_progress" },
      ],
    },
  });
}

/** Answers `turn` at once with its text said back, streamed in two pieces. */
export function answer(session: FakeSession, turn: TurnInput): void {
  const key = `answer-${String(session.turns.length)}`;
  const opening = "You said: ";
  session.started();
  if (turn.text.startsWith(SHOW_WORK)) {
    work(session, session.turns.length);
  }
  session.startsText(key);
  session.report({ type: "item.appended", key, text: opening });
  session.report({ type: "item.appended", key, text: turn.text });
  const item: ItemBody = { kind: "assistant_text", payload: { text: `${opening}${turn.text}` } };
  session.report({ type: "item.updated", key, item, status: "completed" });
  session.finished();
}
