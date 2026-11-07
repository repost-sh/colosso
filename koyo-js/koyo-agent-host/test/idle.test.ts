// How long a session stays (docs/chat.md 4.5.6): closed after the idle time, unless the agent has
// tasks running in the background, shells and Monitors, as the turn each one's end starts can't
// start without it; then closed after the idle time once none runs. Its clock is the test's.

import { afterEach, beforeEach, describe, expect, jest, test } from "bun:test";

import type { Thread } from "../src/protocol/model";
import { FakeProvider, type FakeSession } from "../src/providers/fake";
import type { ItemBody } from "../src/providers/provider";
import { Store } from "../src/store/store";
import { Threads } from "../src/threads/threads";
import { closeHarnesses, type Harness, harness, prompt } from "./support/actor";
import { removeScratch, scratch } from "./support/scratch";

/** How long a session waits for its next turn before it's closed (4.1). */
const IDLE_MS = 5 * 60_000;

/**
 * Lets the steps queued so far run. The clock is the test's, which `Bun.sleep` waits on too, and
 * a step is queued as a microtask.
 */
async function settle(): Promise<void> {
  for (let round = 0; round < 10; round += 1) {
    await Promise.resolve();
  }
}

beforeEach(() => {
  jest.useFakeTimers();
});

afterEach(() => {
  closeHarnesses();
  removeScratch();
  jest.useRealTimers();
});

/** A turn that ran and ended, its session left open. */
async function ranTurn(chats: Harness): Promise<FakeSession> {
  chats.actor.send(prompt("Build it in the background."), "auto");
  const session = chats.provider.session;
  session.started();
  await settle();
  return session;
}

async function finished(session: FakeSession): Promise<void> {
  session.finished();
  await settle();
}

/** Has the session say it runs `tasks` in the background. */
async function inBackground(session: FakeSession, tasks: number): Promise<void> {
  session.report({ type: "background", tasks });
  await settle();
}

/** Lets `ms` of the clock pass, and what it set off run. */
async function idleFor(ms: number): Promise<void> {
  jest.advanceTimersByTime(ms);
  await settle();
}

describe("a session with nothing in the background", () => {
  test("closes after the idle time", async () => {
    const chats = harness();
    const session = await ranTurn(chats);
    await finished(session);
    await idleFor(IDLE_MS - 1);
    expect(session.closed).toBe(false);
    await idleFor(1);
    expect(session.closed).toBe(true);
  });
});

describe("a session with a task in the background", () => {
  test("stays open past the idle time, for as long as one runs", async () => {
    const chats = harness();
    const session = await ranTurn(chats);
    await inBackground(session, 2);
    await finished(session);
    await idleFor(30 * 60_000);
    expect(session.closed).toBe(false);
    expect(chats.actor.busy()).toBe(true);
    expect(chats.actor.closeSession()).toBe(false);
    await inBackground(session, 1);
    await idleFor(30 * 60_000);
    expect(session.closed).toBe(false);
  });

  test("closes after the idle time once none runs", async () => {
    const chats = harness();
    const session = await ranTurn(chats);
    await inBackground(session, 1);
    await finished(session);
    await idleFor(IDLE_MS * 2);
    await inBackground(session, 0);
    expect(chats.actor.busy()).toBe(false);
    await idleFor(IDLE_MS - 1);
    expect(session.closed).toBe(false);
    await idleFor(1);
    expect(session.closed).toBe(true);
  });

  test("wakes the chat as it ends: the agent's turn shows, then the session idles", async () => {
    const chats = harness();
    const session = await ranTurn(chats);
    await inBackground(session, 1);
    await finished(session);
    await idleFor(20 * 60_000);
    // As the CLI says it: the task leaves what runs, then the agent takes up its notification.
    await inBackground(session, 0);
    session.report({ type: "turn.adopted" });
    const item: ItemBody = { kind: "assistant_text", payload: { text: "The build finished." } };
    session.report({ type: "item.started", key: "woke", item });
    session.report({ type: "item.updated", key: "woke", item, status: "completed" });
    await settle();
    expect(chats.rows().runs.map((run) => run.status)).toEqual(["completed", "running"]);
    await idleFor(IDLE_MS);
    expect(session.closed).toBe(false);
    await finished(session);
    const woke = chats.rows().runs.at(1);
    expect(woke?.status).toBe("completed");
    expect(chats.items().filter((each) => each.runId === woke?.id)).toMatchObject([
      { kind: "assistant_text", payload: { text: "The build finished." } },
    ]);
    await idleFor(IDLE_MS - 1);
    expect(session.closed).toBe(false);
    await idleFor(1);
    expect(session.closed).toBe(true);
  });

  test("that ends anyway takes its tasks with it: the next session isn't kept for them",
    async () => {
      const chats = harness();
      const session = await ranTurn(chats);
      await inBackground(session, 1);
      await finished(session);
      session.report({ type: "session.ended", error: null });
      await settle();
      expect(chats.actor.busy()).toBe(false);
      chats.actor.send(prompt("Is it built?"), "auto");
      const next = chats.provider.session;
      expect(next).not.toBe(session);
      next.started();
      await finished(next);
      await idleFor(IDLE_MS);
      expect(next.closed).toBe(true);
    });
});

describe("the host's sessions", () => {
  function threadsOn(store: Store, provider: FakeProvider): Threads {
    return new Threads({
      store,
      providers: [provider],
      attachments: scratch(),
      drafts: "/not/here/chat-drafts.json",
      now: () => new Date().toISOString(),
    });
  }

  function chat(threads: Threads): Thread {
    return threads.create({
      cwd: "/work",
      provider: null,
      model: null,
      effort: null,
      fast: false,
      permission: null,
      plan: false,
    });
  }

  /** Runs a turn of `thread` to its end; gives the session it ran in. */
  async function turnOf(
    threads: Threads,
    provider: FakeProvider,
    thread: Thread,
  ): Promise<FakeSession> {
    threads.actor(thread.id).send(prompt("Go."), "auto");
    const session = provider.session;
    session.started();
    session.finished();
    await settle();
    return session;
  }

  test("pass over one with a task in the background when a fifth chat wants a session",
    async () => {
      const store = Store.open(":memory:");
      const provider = new FakeProvider();
      const threads = threadsOn(store, provider);
      const chats = [chat(threads), chat(threads), chat(threads), chat(threads), chat(threads)];
      const [building, oldest] = chats;
      if (building === undefined || oldest === undefined) {
        throw new Error("No chats were made.");
      }
      const kept = await turnOf(threads, provider, building);
      await inBackground(kept, 1);
      const passed = await turnOf(threads, provider, oldest);
      for (const each of chats.slice(2)) {
        await turnOf(threads, provider, each);
      }
      expect(kept.closed).toBe(false);
      expect(passed.closed).toBe(true);
      // Once nothing runs in the background, it's the least recently used again.
      await inBackground(kept, 0);
      await turnOf(threads, provider, oldest);
      expect(kept.closed).toBe(true);
      store.close();
    });
});
