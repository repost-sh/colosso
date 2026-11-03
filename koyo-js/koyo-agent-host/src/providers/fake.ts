// A provider that runs no agent (docs/chat.md C6), for tests: each session records what the
// actor asked of it and reports what its test tells it to, through the same sink a real
// driver reports to.

import type { Catalog, ProviderStatus } from "../protocol/methods";
import type { RequestPayload, Response } from "../protocol/model";
import type {
  Capabilities,
  DriverEvent,
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

export class FakeProvider implements Provider {
  readonly id = "fake";
  readonly capabilities: Capabilities;
  /** Every session opened, the latest last. */
  readonly sessions: FakeSession[] = [];
  /** The title `title` gives; `undefined` gives none. */
  nextTitle: string | undefined;
  /** Makes `open` fail, as a missing CLI does. */
  failOpen: Error | undefined;

  constructor(capabilities: Partial<Capabilities> = {}) {
    this.capabilities = { ...EVERYTHING, ...capabilities };
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
    return await Promise.resolve({ models: [], commands: [], defaultMode: null });
  }

  open(spec: SessionSpec, sink: SessionSink): ProviderSession {
    if (this.failOpen !== undefined) {
      throw this.failOpen;
    }
    const session = new FakeSession(spec, sink);
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
  interrupts = 0;
  killed = false;
  closed = false;
  /** Whether the running turn takes a message; a turn about to end doesn't. */
  steerable = true;
  /** What a settings change does to the session. */
  configures: "applied" | "reopen" = "applied";
  /** Makes `interrupt` fail, as a process that stopped answering does. */
  failInterrupt: Error | undefined;

  constructor(spec: SessionSpec, sink: SessionSink) {
    this.spec = spec;
    this.#sink = sink;
  }

  start(turn: TurnInput): void {
    this.turns.push(turn);
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
    this.report({ type: "item.started", key, parentKey: null, item });
  }

  finished(outcome: Outcome = "completed"): void {
    this.report({ type: "turn.finished", outcome, failure: null, usage: null });
  }

  ask(payload: RequestPayload): Asked {
    const controller = new AbortController();
    return {
      answer: this.#sink.ask(payload, controller.signal),
      withdraw: () => {
        controller.abort();
      },
    };
  }
}
