// A thread's actor as tests drive it: on a store of its own, with the fake provider, a clock
// that ticks a second a call, and a subscriber that keeps every line it's sent.

import { z } from "zod";

import { events } from "../../src/protocol/events";
import type { ThreadId } from "../../src/protocol/ids";
import type { Rows } from "../../src/protocol/methods";
import type { Item, ThreadSummary } from "../../src/protocol/model";
import { FakeProvider } from "../../src/providers/fake";
import type { Capabilities } from "../../src/providers/provider";
import type { Change } from "../../src/store/store";
import { Store } from "../../src/store/store";
import { type Context, ThreadActor } from "../../src/threads/actor";
import { textPrompt } from "../../src/threads/records";
import { thread } from "./records";

export class Harness {
  readonly store = Store.open(":memory:");
  readonly provider: FakeProvider;
  readonly summaries: ThreadSummary[] = [];
  /** The actors that ran a turn, as the threads' session pool hears of them. */
  readonly used: ThreadActor[] = [];
  /** The lines the actor sent its subscriber, parsed. */
  readonly lines: unknown[] = [];
  readonly id: ThreadId;
  #tick = 0;
  #actor: ThreadActor | undefined;

  constructor(capabilities: Partial<Capabilities> = {}, given: Partial<Change> = {}) {
    this.provider = new FakeProvider(capabilities);
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
      attachments: "/attachments",
      now: () => {
        this.#tick += 1;
        return new Date(Date.UTC(2026, 10, 3, 18, 0, this.#tick)).toISOString();
      },
      summarized: (summary) => {
        this.summaries.push(summary);
      },
      sessionUsed: (actor) => {
        this.used.push(actor);
      },
    };
  }
}

const AppendedLine = z.object({
  event: z.literal("item.appended"),
  params: events["item.appended"],
});

export const prompt = textPrompt;

/** Lets the steps queued behind the running one run. */
export async function settle(): Promise<void> {
  await Bun.sleep(0);
}
