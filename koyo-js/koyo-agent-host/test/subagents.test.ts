// A thread's subagents and the turns its agent starts itself (docs/chat.md 4.5.10), as a session
// reports them.

import { afterEach, describe, expect, test } from "bun:test";

import type { Item, RequestPayload, Response, Thread } from "../src/protocol/model";
import { FakeProvider, type FakeSession } from "../src/providers/fake";
import type {
  ItemBody,
  ItemEvent,
  Outcome,
  SubagentEvent,
  SubagentStart,
} from "../src/providers/provider";
import { Store } from "../src/store/store";
import { Threads } from "../src/threads/threads";
import { closeHarnesses, type Harness, harness, prompt } from "./support/actor";
import { removeScratch, scratch } from "./support/scratch";
import { settle } from "./support/wait";

afterEach(closeHarnesses);

const CALL = "toolu_agent";
const START: SubagentStart = {
  description: "Count files",
  agentType: "general-purpose",
  prompt: "Count the files.",
  model: "haiku",
  effort: "low",
};

function said(text: string): ItemBody {
  return { kind: "assistant_text", payload: { text } };
}

async function running(chats: Harness): Promise<FakeSession> {
  chats.actor.send(prompt("Go."), "auto");
  chats.provider.session.started();
  await settle();
  return chats.provider.session;
}

function heard(event: ItemEvent): SubagentEvent {
  return { type: "subagent.event", key: CALL, event };
}

function started(key = CALL, parentKey: string | null = null): SubagentEvent {
  return { type: "subagent.started", key, parentKey, subagent: START };
}

function ended(outcome: Outcome = "completed", key = CALL): SubagentEvent {
  return { type: "subagent.ended", key, outcome, summary: "One file.", usage: null };
}

/** A subagent started in a running turn: its session, row and thread. */
async function spawned(chats: Harness): Promise<{ session: FakeSession; child: Thread }> {
  const session = await running(chats);
  session.report(started());
  await settle();
  const child = chats.children.at(0)?.thread;
  if (child === undefined) {
    throw new Error("No subagent's thread was made.");
  }
  return { session, child };
}

function row(chats: Harness): Extract<Item, { kind: "subagent" }> {
  const found = chats.items().find((item) => item.kind === "subagent");
  if (found?.kind !== "subagent") {
    throw new Error("No subagent's row was written.");
  }
  return found;
}

describe("a subagent", () => {
  test("has a row in the timeline it started from, which names its thread", async () => {
    const chats = harness();
    const { child } = await spawned(chats);
    expect(row(chats)).toMatchObject({
      status: "running",
      payload: {
        description: "Count files",
        agentType: "general-purpose",
        threadId: child.id,
        summary: null,
        endedAt: null,
      },
    });
  });

  test("works on a thread of its own, its prompt its first message", async () => {
    const chats = harness();
    const { child } = await spawned(chats);
    expect(child).toMatchObject({
      title: "Count files",
      model: "haiku",
      effort: "low",
      sessionId: null,
      tabOpen: false,
      parent: { threadId: chats.id, itemId: row(chats).id },
    });
    const { runs, items } = chats.store.latest(child.id, 10);
    expect(runs.map((run) => run.status)).toEqual(["running"]);
    expect(items).toMatchObject([{
      kind: "user_message",
      payload: { text: "Count the files." },
    }]);
  });

  test("has its timeline told to its own thread", async () => {
    const chats = harness();
    const { session, child } = await spawned(chats);
    const item = said("I'll list them.");
    session.report(heard({ type: "item.started", key: "t", item }));
    session.report(heard({ type: "item.updated", key: "t", item, status: "completed" }));
    await settle();
    expect(chats.store.latest(child.id, 10).items.at(-1)).toMatchObject({
      kind: "assistant_text",
      status: "completed",
      payload: { text: "I'll list them." },
    });
    expect(chats.items().some((each) => each.kind === "assistant_text")).toBe(false);
  });

  test("has its row follow what it does", async () => {
    const chats = harness();
    const { session, child } = await spawned(chats);
    const usage = { tokens: 12, tools: 1 };
    const shown = { progress: "Running ls", usage };
    const change = { ...shown, model: "claude-opus-5-5" };
    session.report({ type: "subagent.changed", key: CALL, change });
    await settle();
    expect(row(chats).payload).toMatchObject(shown);
    expect(chats.store.thread(child.id)?.model).toBe("claude-opus-5-5");
  });

  test("ended closes its row and settles its thread's run", async () => {
    const chats = harness();
    const { session, child } = await spawned(chats);
    session.report({ type: "subagent.changed", key: CALL, change: { progress: "Running ls" } });
    session.report(ended());
    await settle();
    expect(row(chats)).toMatchObject({
      status: "completed",
      payload: { summary: "One file.", progress: null },
    });
    expect(row(chats).payload.endedAt).not.toBeNull();
    expect(chats.store.latest(child.id, 10).runs.map((run) => run.status)).toEqual(["completed"]);
  });

  test("that failed has its thread's run say why", async () => {
    const chats = harness();
    const { session, child } = await spawned(chats);
    session.report(ended("failed"));
    await settle();
    expect(row(chats).status).toBe("failed");
    expect(chats.store.latest(child.id, 10).runs.at(0)?.failure).toMatchObject({
      message: "One file.",
    });
  });

  test("started again runs anew on the thread it had", async () => {
    const chats = harness();
    const { session, child } = await spawned(chats);
    session.report(ended());
    session.report(started());
    await settle();
    expect(chats.children).toHaveLength(1);
    expect(row(chats)).toMatchObject({ status: "running", payload: { summary: null } });
    expect(chats.store.latest(child.id, 10).runs.map((run) => run.status))
      .toEqual(["completed", "running"]);
  });

  test("a subagent starts has its row in its timeline", async () => {
    const chats = harness();
    const { session, child } = await spawned(chats);
    session.report(started("toolu_inner", CALL));
    await settle();
    const inner = chats.children.at(1)?.thread;
    expect(inner?.parent?.threadId).toBe(child.id);
    expect(chats.store.latest(child.id, 10).items.map((item) => item.kind))
      .toEqual(["user_message", "subagent"]);
    session.report(ended("completed", "toolu_inner"));
    await settle();
    expect(chats.store.latest(child.id, 10).items.at(-1)?.status).toBe("completed");
  });

  test("takes no message of a person's", async () => {
    const chats = harness();
    await spawned(chats);
    const thread = chats.children.at(0);
    expect(() => thread?.send(prompt("Hello?"), "auto")).toThrow("A subagent runs on its own.");
  });

  test("is stopped through the session, and ends when it says", async () => {
    const chats = harness();
    const { session, child } = await spawned(chats);
    chats.children.at(0)?.stop(false);
    await settle();
    expect(session.subagentsStopped).toEqual([CALL]);
    expect(chats.store.latest(child.id, 10).runs.at(0)?.status).toBe("stopping");
    session.report(ended("interrupted"));
    await settle();
    expect(row(chats).status).toBe("interrupted");
    expect(chats.store.latest(child.id, 10).runs.at(0)?.status).toBe("interrupted");
    expect(session.interrupts).toBe(0);
  });
});

describe("a subagent in the background", () => {
  test("stays running past the run that started it", async () => {
    const chats = harness();
    const { session, child } = await spawned(chats);
    session.finished();
    await settle();
    expect(chats.rows().runs.map((run) => run.status)).toEqual(["completed"]);
    expect(row(chats).status).toBe("running");
    expect(chats.store.latest(child.id, 10).runs.at(0)?.status).toBe("running");
    expect(chats.actor.busy()).toBe(true);
    expect(chats.actor.closeSession()).toBe(false);
    session.report(ended());
    await settle();
    expect(row(chats).status).toBe("completed");
    expect(chats.actor.busy()).toBe(false);
    expect(chats.actor.closeSession()).toBe(true);
  });

  test("is told to its thread with no run of the parent's live", async () => {
    const chats = harness();
    const { session, child } = await spawned(chats);
    session.finished();
    await settle();
    const item = said("Later.");
    session.report(heard({ type: "item.started", key: "t", item }));
    await settle();
    expect(chats.store.latest(child.id, 10).items.at(-1)).toMatchObject({
      payload: { text: "Later." },
    });
  });

  test("goes with its session, however that goes", async () => {
    const chats = harness();
    const { session, child } = await spawned(chats);
    session.finished();
    await settle();
    session.report({ type: "session.ended", error: null });
    await settle();
    expect(row(chats).status).toBe("interrupted");
    expect(chats.store.latest(child.id, 10).runs.at(0)?.status).toBe("interrupted");
  });

  test("goes with a stop that ends the process", async () => {
    const chats = harness();
    const { session } = await spawned(chats);
    chats.actor.stop(true);
    await settle();
    expect(session.killed).toBe(true);
    expect(row(chats).status).toBe("interrupted");
  });

  test("ends with its session, so a stop after finds nothing to ask", async () => {
    const chats = harness();
    const { session, child } = await spawned(chats);
    session.report({ type: "session.ended", error: null });
    await settle();
    chats.children.at(0)?.stop(false);
    await settle();
    expect(chats.store.latest(child.id, 10).runs.at(0)?.status).toBe("interrupted");
  });
});

describe("a turn the agent starts itself", () => {
  test("is a run of no message, whose answer shows like any other", async () => {
    const chats = harness();
    const session = await running(chats);
    session.finished();
    await settle();
    session.report({ type: "turn.adopted" });
    const item = said("It's done.");
    session.report({ type: "item.started", key: "w", item });
    session.report({ type: "item.updated", key: "w", item, status: "completed" });
    await settle();
    expect(chats.rows().runs.map((run) => run.status)).toEqual(["completed", "running"]);
    session.finished();
    await settle();
    expect(chats.rows().runs.map((run) => run.status)).toEqual(["completed", "completed"]);
    const adopted = chats.rows().runs.at(1)?.id;
    expect(chats.items().filter((each) => each.runId === adopted).map((each) => each.kind))
      .toEqual(["assistant_text"]);
  });

  test("takes a message as any running turn does, and a stop", async () => {
    const chats = harness();
    const session = await running(chats);
    session.finished();
    await settle();
    session.report({ type: "turn.adopted" });
    await settle();
    expect(chats.actor.send(prompt("Also this."), "auto").delivered).toBe("steered");
    expect(session.steers).toHaveLength(1);
    chats.actor.stop(false);
    expect(session.interrupts).toBe(1);
  });

  test("is followed by what was queued behind it", async () => {
    const chats = harness();
    const session = await running(chats);
    session.finished();
    await settle();
    session.report({ type: "turn.adopted" });
    await settle();
    chats.actor.send(prompt("Next."), "queue");
    session.finished();
    await settle();
    expect(session.turns.map((turn) => turn.text)).toEqual(["Go.", "Next."]);
  });
});

describe("what a subagent asks", () => {
  const APPROVAL: RequestPayload = {
    kind: "approval",
    tool: "Bash",
    family: "command",
    title: "Run a command",
    label: "ls",
    detail: "ls",
    change: null,
    input: {},
    reason: null,
    allowSession: null,
  };

  test("is asked with no run live, in the latest run, and keeps the session up", async () => {
    const chats = harness();
    const { session } = await spawned(chats);
    session.finished();
    await settle();
    const { answer } = session.ask(APPROVAL, true);
    await settle();
    const [request] = chats.store.state(chats.id).pending;
    expect(request?.runId).toBe(chats.rows().runs.at(0)?.id);
    expect(chats.actor.busy()).toBe(true);
    if (request === undefined) {
      throw new Error("Nothing was asked.");
    }
    const response: Response = { kind: "approval", decision: "allow", message: null };
    chats.actor.respond(request.id, response);
    expect(await answer).toEqual(response);
  });

  test("isn't ended by the turn it asked in ending", async () => {
    const chats = harness();
    const { session } = await spawned(chats);
    session.ask(APPROVAL, true).answer.catch(() => {
      // Nobody answers.
    });
    await settle();
    session.finished();
    await settle();
    expect(chats.store.state(chats.id).pending).toHaveLength(1);
  });

  test("is cancelled as its session goes", async () => {
    const chats = harness();
    const { session } = await spawned(chats);
    const { answer } = session.ask(APPROVAL, true);
    await settle();
    session.report({ type: "session.ended", error: null });
    expect(answer).rejects.toThrow("The session ended.");
    await settle();
    expect(chats.store.state(chats.id).pending).toEqual([]);
  });

  test("of the agent itself is refused with no run live", async () => {
    const chats = harness();
    const session = await running(chats);
    session.finished();
    await settle();
    const { answer } = session.ask(APPROVAL);
    expect(answer).rejects.toThrow("No turn waits for an answer.");
    await settle();
  });
});

describe("the host's threads", () => {
  const NOW = (): string => new Date().toISOString();

  /** Threads with a running chat whose agent started a subagent. */
  async function family(): Promise<{
    threads: Threads;
    store: Store;
    session: () => FakeSession;
    parent: Thread;
    child: Thread;
  }> {
    const store = Store.open(":memory:");
    const provider = new FakeProvider();
    const threads = new Threads({ store, providers: [provider], attachments: scratch(), now: NOW });
    const parent = threads.create({
      cwd: "/work",
      provider: null,
      model: null,
      effort: null,
      fast: false,
      permission: null,
      plan: false,
    });
    threads.actor(parent.id).send(prompt("Go."), "auto");
    provider.session.started();
    await settle();
    provider.session.report(started());
    await settle();
    const childId = store.children(parent.id).at(0);
    const child = childId === undefined ? undefined : store.thread(childId);
    if (child === undefined) {
      throw new Error("No subagent's thread was made.");
    }
    return { threads, store, session: () => provider.session, parent, child };
  }

  afterEach(removeScratch);

  test("list what a person started, not their subagents'", async () => {
    const { threads, parent, session } = await family();
    const lines: string[] = [];
    const watcher = {
      send: (line: string): void => {
        lines.push(line);
      },
    };
    expect(threads.watch(watcher).map((each) => each.id)).toEqual([parent.id]);
    session().report({ type: "subagent.changed", key: CALL, change: { progress: "Running ls" } });
    session().report(ended());
    await settle();
    expect(lines.join()).not.toContain("Count files");
  });

  test("open a subagent's thread as any other", async () => {
    const { threads, child } = await family();
    const snapshot = threads.actor(child.id).open({
      send: () => {
        // It's only read.
      },
    }, null);
    expect(snapshot.thread.parent?.threadId).toBeString();
    expect(snapshot.items.at(0)?.kind).toBe("user_message");
  });

  test("delete a thread's subagents with it", async () => {
    const { threads, store, parent, child } = await family();
    threads.delete(parent.id);
    expect(store.thread(child.id)).toBeUndefined();
    expect(store.thread(parent.id)).toBeUndefined();
  });

  test("close what a host that stopped left of a subagent, row and run", async () => {
    const { store, parent, child, session } = await family();
    session().finished();
    await settle();
    const next = new Threads({
      store,
      providers: [new FakeProvider()],
      attachments: scratch(),
      now: NOW,
    });
    next.recover();
    const left = store.latest(parent.id, 10).items.find((item) => item.kind === "subagent");
    expect(left?.status).toBe("interrupted");
    expect(store.latest(child.id, 10).runs.at(0)?.status).toBe("interrupted");
  });

  test("say when one ended that a host stopped in the middle of its run", async () => {
    const { store, parent, session } = await family();
    const change = { progress: "Running ls" };
    session().report({ type: "subagent.changed", key: CALL, change });
    await settle();
    const next = new Threads({
      store,
      providers: [new FakeProvider()],
      attachments: scratch(),
      now: NOW,
    });
    next.recover();
    const left = store.latest(parent.id, 10).items.find((item) => item.kind === "subagent");
    expect(left).toMatchObject({ status: "interrupted", payload: { progress: null } });
    expect(left?.kind === "subagent" && left.payload.endedAt !== null).toBe(true);
  });

  test("stay up for a subagent working on after its run", async () => {
    const { threads, session } = await family();
    session().finished();
    await settle();
    expect(threads.busy()).toBe(true);
    session().report(ended());
    await settle();
    expect(threads.busy()).toBe(false);
  });
});
