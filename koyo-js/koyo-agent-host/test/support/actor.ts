// A thread's actor as tests drive it: on a store of its own, with the fake provider, a clock
// that ticks a second a call, and a subscriber that keeps every line it's sent.

import { z } from "zod";

import { events } from "../../src/protocol/events";
import type { ThreadId } from "../../src/protocol/ids";
import type { Rows } from "../../src/protocol/methods";
import type { AccountLimits, Item, ThreadSummary } from "../../src/protocol/model";
import { FakeProvider } from "../../src/providers/fake";
import type { Capabilities, Config } from "../../src/providers/provider";
import type { Change } from "../../src/store/store";
import { Store } from "../../src/store/store";
import { ThreadActor } from "../../src/threads/actor";
import type { Context } from "../../src/threads/context";
import { textPrompt } from "../../src/threads/records";
import { thread } from "./records";
import { settle } from "./wait";

export class Harness {
  readonly store = Store.open(":memory:");
  readonly provider: FakeProvider;
  readonly summaries: ThreadSummary[] = [];
  /** What responses said of the account's limits. */
  readonly limits: AccountLimits[] = [];
  /** The actors that ran a turn, as the threads' session pool hears of them. */
  readonly used: ThreadActor[] = [];
  /** The actors of the subagents' threads. */
  readonly children: ThreadActor[] = [];
  /** The lines the actor sent its subscriber, parsed. */
  readonly lines: unknown[] = [];
  readonly id: ThreadId;
  /** Koyo's settings, as the actor reads them. */
  config: Config = { claudePath: null, defaultMode: null, koyoTools: null };
  #tick = 0;
  #actor: ThreadActor | undefined;

  constructor(capabilities: Partial<Capabilities> = {}, given: Partial<Change> = {}) {
    this.provider = new FakeProvider({ capabilities });
    const chat = given.thread ?? thread();
    this.id = chat.id;
    this.store.commit(chat.id, { ...given, thread: chat });
  }

  /** The thread's actor, loaded from the store the first time it's wanted. */
  get actor(): ThreadActor {
    this.#actor ??= this.load();
    return this.#actor;
  }

  /** A new actor for the thread, as a host starting over makes. */
  load(): ThreadActor {
    const stored = this.store.thread(this.id);
    if (stored === undefined) {
      throw new Error("The thread isn't stored.");
    }
    const actor = ThreadActor.load(this.#context(), stored);
    actor.open({ send: (line) => this.lines.push(JSON.parse(line)) }, null);
    this.#actor = actor;
    return actor;
  }

  rows(): Rows {
    return this.store.latest(this.id, 100);
  }

  /** The thread's items in timeline order, with their text as the store has it. */
  items(): Item[] {
    return this.rows().items;
  }

  /** The text each `item.appended` line carried. */
  appends(): string[] {
    return this.lines.flatMap((line) => {
      const parsed = AppendedLine.safeParse(line);
      return parsed.success ? [parsed.data.params.text] : [];
    });
  }

  close(): void {
    this.#actor?.dispose();
    this.store.close();
  }

  #context(): Context {
    return {
      store: this.store,
      provider: this.provider,
      config: () => this.config,
      attachments: "/attachments",
      now: () => {
        this.#tick += 1;
        return new Date(Date.UTC(2026, 10, 3, 18, 0, this.#tick)).toISOString();
      },
      summarized: (summary) => {
        this.summaries.push(summary);
      },
      limited: (limits) => {
        this.limits.push(limits);
      },
      sessionUsed: (actor) => {
        this.used.push(actor);
      },
      spawn: (child, prompt, stop) => {
        const actor = ThreadActor.subagent(this.#context(), child, prompt, stop);
        this.children.push(actor);
        return actor;
      },
    };
  }
}

const AppendedLine = z.object({
  event: z.literal("item.appended"),
  params: events["item.appended"],
});

export const prompt = textPrompt;

const harnesses: Harness[] = [];

/** A harness that `closeHarnesses`, which a test file runs after each test, closes. */
export function harness(...args: ConstructorParameters<typeof Harness>): Harness {
  const made = new Harness(...args);
  harnesses.push(made);
  return made;
}

/** Closes every harness `harness` made. */
export function closeHarnesses(): void {
  for (const made of harnesses.splice(0)) {
    made.close();
  }
}

/** Starts a turn and has the session say it started. */
export async function running(chats: Harness, text = "Find the flaky test."): Promise<void> {
  chats.actor.send(prompt(text), "auto");
  chats.provider.session.started();
  await settle();
}
