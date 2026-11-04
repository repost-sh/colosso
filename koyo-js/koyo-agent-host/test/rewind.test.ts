// Going back in a chat (docs/chat.md C11), as a provider's session sees it and a client hears it.

import { afterEach, describe, expect, test } from "bun:test";

import type { RunId } from "../src/protocol/ids";
import { Run, type Thread } from "../src/protocol/model";
import { HostError } from "../src/protocol/wire";
import { FakeProvider } from "../src/providers/fake";
import type { SubagentStart } from "../src/providers/provider";
import { Store } from "../src/store/store";
import { Threads } from "../src/threads/threads";
import { prompt } from "./support/actor";
import { removeScratch, scratch } from "./support/scratch";
import { settle } from "./support/wait";

afterEach(removeScratch);

const START: SubagentStart = {
  description: "Count files",
  agentType: "general-purpose",
  prompt: "Count the files.",
  model: "haiku",
  effort: "low",
};

interface Chat {
  readonly store: Store;
  readonly provider: FakeProvider;
  readonly threads: Threads;
  readonly thread: Thread;
}

function chat(): Chat {
  const store = Store.open(":memory:");
  const provider = new FakeProvider();
  const threads = new Threads({
    store,
    providers: [provider],
    attachments: scratch(),
    drafts: "/not/here/chat-drafts.json",
    now: () => new Date().toISOString(),
  });
  const thread = threads.create({
    cwd: "/work",
    provider: "fake",
    model: null,
    effort: null,
    fast: false,
    permission: null,
    plan: false,
  });
  return { store, provider, threads, thread };
}

/** A turn of `text`, from start to finish. */
async function said(on: Chat, text: string): Promise<RunId> {
  const { runId } = on.threads.actor(on.thread.id).send(prompt(text), "auto");
  on.provider.session.started();
  await settle();
  on.provider.session.finished();
  await settle();
  return runId;
}

function shown(on: Chat): string[] {
  return on.store.latest(on.thread.id, 10).runs.map((run) => run.prompt.text);
}

function stored(on: Chat): Thread {
  const thread = on.store.thread(on.thread.id);
  if (thread === undefined) {
    throw new Error("The thread isn't stored.");
  }
  return thread;
}

describe("going back", () => {
  test("a message and what came after leave the chat, and its words come back", async () => {
    const on = chat();
    await said(on, "First");
    const second = await said(on, "Second");
    await said(on, "Third");
    const { session } = on.provider;
    on.provider.entry = "entry-of-first";

    const back = await on.threads.rewind(on.thread.id, second);

    expect(back.text).toBe("Second");
    expect(shown(on)).toEqual(["First"]);
    expect(on.store.latest(on.thread.id, 10).items).toHaveLength(1);
    expect(stored(on).resumeAt).toBe("entry-of-first");
    expect(session.closed).toBe(true);
  });

  test("the next turn goes on from there, and then from its own", async () => {
    const on = chat();
    await said(on, "First");
    const second = await said(on, "Second");
    on.provider.entry = "entry-of-first";
    await on.threads.rewind(on.thread.id, second);
    const { sessionId } = stored(on);

    on.threads.actor(on.thread.id).send(prompt("Other"), "auto");

    expect(on.provider.session.spec.resumeAt).toBe("entry-of-first");
    expect(on.provider.session.spec.resume).toBe(true);
    expect(on.provider.session.spec.sessionId).toBe(sessionId ?? "");
    on.provider.session.started();
    await settle();
    expect(stored(on).resumeAt).toBeNull();
  });

  test("the message that started the conversation has the next turn start another", async () => {
    const on = chat();
    const first = await said(on, "First");
    const old = stored(on).sessionId;
    on.provider.entry = null;

    await on.threads.rewind(on.thread.id, first);

    expect(shown(on)).toEqual([]);
    expect(stored(on).sessionId).toBeNull();
    on.threads.actor(on.thread.id).send(prompt("Other"), "auto");
    expect(on.provider.session.spec.resume).toBe(false);
    expect(on.provider.session.spec.sessionId).not.toBe(old ?? "");
  });

  test("a chat at work, or with messages waiting, stays as it is", async () => {
    const on = chat();
    const first = await said(on, "First");
    const actor = on.threads.actor(on.thread.id);
    actor.send(prompt("Running"), "queue");
    on.provider.session.started();
    await settle();

    expect(on.threads.rewind(on.thread.id, first)).rejects.toThrow(HostError);
    actor.send(prompt("Waiting"), "queue");
    on.provider.session.finished("interrupted");
    await settle();
    expect(on.threads.rewind(on.thread.id, first)).rejects.toThrow(HostError);

    expect(shown(on)).toEqual(["First", "Running", "Waiting"]);
  });

  test("a message sent while the conversation is read keeps the chat as it is", async () => {
    const on = chat();
    const first = await said(on, "First");
    const going = on.threads.rewind(on.thread.id, first);
    on.threads.actor(on.thread.id).send(prompt("Quick"), "auto");

    expect(going).rejects.toThrow(HostError);
    await settle();

    expect(shown(on)).toEqual(["First", "Quick"]);
  });

  test("a conversation without the message refuses, and the chat stays as it is", async () => {
    const on = chat();
    const first = await said(on, "First");
    on.provider.entry = new HostError("conflict", "It isn't there.");

    expect(on.threads.rewind(on.thread.id, first)).rejects.toThrow("It isn't there.");
    await settle();

    expect(shown(on)).toEqual(["First"]);
    expect(stored(on).resumeAt).toBeNull();
  });

  test("a message the chat doesn't have is refused", async () => {
    const on = chat();
    await said(on, "First");
    const none = Run.shape.id.parse("0193a2f0-7777-7b3a-9f21-4d5e6f708192");

    expect(on.threads.rewind(on.thread.id, none)).rejects.toThrow(HostError);
    await settle();
  });

  test("a client that held the runs hears they're gone, and one that opens doesn't", async () => {
    const on = chat();
    await said(on, "First");
    const second = await said(on, "Second");
    const head = on.store.head(on.thread.id);

    await on.threads.rewind(on.thread.id, second);

    const heard = on.store.since(on.thread.id, head);
    expect(heard.runs.map((run) => [run.id, run.status])).toEqual([[second, "removed"]]);
    expect(heard.items).toEqual([]);
    expect(on.store.before(on.thread.id, second, 10).runs).toHaveLength(1);
  });

  test("asking for the latest runs gets those that are left", async () => {
    const on = chat();
    const first = await said(on, "First");
    const second = await said(on, "Second");
    await said(on, "Third");
    const actor = on.threads.actor(on.thread.id);
    expect(actor.history(null, 1).runs.map((run) => run.prompt.text)).toEqual(["Third"]);

    await on.threads.rewind(on.thread.id, second);

    expect(actor.history(null, 5).runs.map((run) => run.id)).toEqual([first]);
    expect(actor.history(null, 5).items.length).toBeGreaterThan(0);
  });

  test("how a chat is doing no longer counts what's gone", async () => {
    const on = chat();
    await said(on, "First");
    const { runId } = on.threads.actor(on.thread.id).send(prompt("Fails"), "auto");
    on.provider.session.started();
    await settle();
    on.provider.session.finished("failed");
    await settle();
    expect(on.threads.actor(on.thread.id).summary().status).toBe("failed");

    await on.threads.rewind(on.thread.id, runId);

    expect(on.threads.actor(on.thread.id).summary().status).toBe("idle");
  });

  test("a subagent started by what's gone goes with it", async () => {
    const on = chat();
    await said(on, "First");
    const { runId } = on.threads.actor(on.thread.id).send(prompt("Second"), "auto");
    on.provider.session.started();
    await settle();
    const session = on.provider.session;
    session.report({ type: "subagent.started", key: "agent", parentKey: null, subagent: START });
    await settle();
    expect(on.store.children(on.thread.id)).toHaveLength(1);
    session.report({
      type: "subagent.ended",
      key: "agent",
      outcome: "completed",
      summary: null,
      usage: null,
    });
    session.finished();
    await settle();

    await on.threads.rewind(on.thread.id, runId);

    expect(on.store.children(on.thread.id)).toEqual([]);
  });
});
