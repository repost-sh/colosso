import { afterEach, describe, expect, test } from "bun:test";

import type { RequestId } from "../src/protocol/ids";
import type { Item, Request, RequestPayload, Response } from "../src/protocol/model";
import { HostError } from "../src/protocol/wire";
import type { Subscriber } from "../src/threads/state";
import { closeHarnesses, type Harness, harness, prompt, running } from "./support/actor";
import { request, run, thread } from "./support/records";
import { settle } from "./support/wait";

afterEach(closeHarnesses);

const TEXT: Item["kind"] = "assistant_text";

/** A client that opened the thread and reads nothing. */
const nobody: Subscriber = { send: () => true };

function textOf(items: readonly Item[]): string | undefined {
  const item = items.find((each) => each.kind === TEXT);
  return item?.kind === "assistant_text" ? item.payload.text : undefined;
}

const APPROVAL: RequestPayload = {
  kind: "approval",
  tool: "Bash",
  family: "command",
  title: "Run a command",
  label: "bun test",
  detail: "bun test",
  change: null,
  input: { command: "bun test" },
  reason: null,
  allowSession: null,
};

/** Why `answer` was refused. */
async function refusal(answer: Promise<unknown>): Promise<unknown> {
  try {
    await answer;
  } catch (error) {
    return error;
  }
  throw new Error("It was answered.");
}

function pending(chats: Harness): Request[] {
  return chats.rows().requests.filter((each) => each.status === "pending");
}

/** The id of the oldest request pending, which a test has made sure there is. */
function pendingId(chats: Harness): RequestId {
  const id = pending(chats).at(0)?.id;
  if (id === undefined) {
    throw new Error("No request is pending.");
  }
  return id;
}

describe("a turn", () => {
  test("starts, streams its text and settles with it whole", async () => {
    const chats = harness();
    const { delivered } = chats.actor.send(prompt("Find the flaky test."), "auto");
    expect(delivered).toBe("started");
    const session = chats.provider.session;
    expect(session.spec.resume).toBe(false);
    expect(chats.actor.thread.sessionId).toBe(session.spec.sessionId);
    expect(chats.actor.thread.title).toBe("Find the flaky test.");
    session.started();
    session.startsText("m1:0");
    session.report({ type: "item.appended", key: "m1:0", text: "It shares " });
    session.report({ type: "item.appended", key: "m1:0", text: "a folder." });
    await settle();
    expect(chats.appends()).toStrictEqual(["It shares ", "a folder."]);
    expect(chats.actor.summary().status).toBe("working");
    // What streamed is in a snapshot before the store has it.
    const snapshot = chats.actor.open(nobody, null);
    expect(textOf(snapshot.items)).toBe("It shares a folder.");

    session.finished();
    await settle();
    const { runs, items } = chats.rows();
    expect(runs.map((each) => each.status)).toStrictEqual(["completed"]);
    expect(items.map((each) => [each.kind, each.status])).toStrictEqual([
      ["user_message", "completed"],
      ["assistant_text", "completed"],
    ]);
    expect(textOf(items)).toBe("It shares a folder.");
    expect(chats.actor.summary()).toMatchObject({ status: "idle", unread: true });
  });

  test("answers a send repeated from what it became, and sends it once", async () => {
    const chats = harness();
    const message = prompt("Find the flaky test.");
    const first = chats.actor.send(message, "auto");
    expect(chats.actor.send(message, "auto")).toStrictEqual(first);
    chats.provider.session.started();
    await settle();
    const queued = prompt("Then fix it.");
    const later = chats.actor.send(queued, "queue");
    expect(chats.actor.send(queued, "auto")).toStrictEqual(later);
    expect(chats.provider.session.turns).toHaveLength(1);
    expect(chats.provider.session.steers).toHaveLength(0);
  });

  test("takes a message into the running turn, or queues it when asked to", async () => {
    const chats = harness();
    await running(chats);
    const steered = chats.actor.send(prompt("Keep the default at three."), "auto");
    expect(steered.delivered).toBe("steered");
    const queued = chats.actor.send(prompt("Then write the changelog."), "queue");
    expect(queued.delivered).toBe("queued");
    expect(chats.provider.session.steers.map((each) => each.text))
      .toStrictEqual(["Keep the default at three."]);
    const steer = chats.items().find((item) => item.kind === "user_message" && item.ordinal > 0);
    expect(steer).toMatchObject({ runId: steered.runId, payload: { delivered: "steer" } });
    expect(chats.actor.summary().queued).toBe(1);
  });

  test("queues a message the turn can't take", async () => {
    const chats = harness({ steer: false });
    await running(chats);
    expect(chats.actor.send(prompt("And the docs."), "auto").delivered).toBe("queued");
    const other = harness();
    await running(other);
    other.provider.session.steerable = false;
    expect(other.actor.send(prompt("And the docs."), "auto").delivered).toBe("queued");
  });

  test("starts the queue's next run once the one before settles", async () => {
    const chats = harness();
    await running(chats);
    chats.actor.send(prompt("Then write the changelog."), "queue");
    chats.provider.session.finished();
    await settle();
    const { runs } = chats.rows();
    expect(runs.map((each) => each.status)).toStrictEqual(["completed", "starting"]);
    expect(chats.provider.session.turns.map((each) => each.text))
      .toStrictEqual(["Find the flaky test.", "Then write the changelog."]);
  });

  test("settles from starting when it ends before it's seen to start", async () => {
    const chats = harness();
    chats.actor.send(prompt("/compact"), "auto");
    chats.provider.session.finished();
    await settle();
    expect(chats.rows().runs.at(0)?.status).toBe("completed");
  });

  test("fails when its session can't open", () => {
    const chats = harness();
    chats.provider.failOpen = new Error("claude is not installed");
    chats.actor.send(prompt("Find the flaky test."), "auto");
    const [failed] = chats.rows().runs;
    expect(failed).toMatchObject({
      status: "failed",
      failure: { kind: "process", message: "claude is not installed" },
    });
    expect(chats.actor.summary().status).toBe("failed");
  });

  test("fails when the agent's process ends under it", async () => {
    const chats = harness();
    await running(chats);
    chats.provider.session.report({ type: "session.ended", error: "exit code 1" });
    await settle();
    expect(chats.rows().runs.at(0)).toMatchObject({
      status: "failed",
      failure: { message: "exit code 1" },
    });
    expect(chats.actor.hasSession()).toBe(false);
  });
});

describe("stopping", () => {
  test("holds the queue until the person resumes it", async () => {
    const chats = harness();
    await running(chats);
    chats.actor.send(prompt("Then write the changelog."), "queue");
    chats.actor.stop(false);
    expect(chats.rows().runs.at(0)?.status).toBe("stopping");
    expect(chats.provider.session.interrupts).toBe(1);
    expect(chats.actor.thread.queueHeld).toBe(true);
    // The barrier: nothing joins a run that's stopping.
    expect(chats.actor.send(prompt("Wait."), "auto").delivered).toBe("queued");
    chats.provider.session.finished("interrupted");
    await settle();
    expect(chats.rows().runs.map((each) => each.status))
      .toStrictEqual(["interrupted", "queued", "queued"]);
    chats.actor.resume();
    expect(chats.rows().runs.map((each) => each.status))
      .toStrictEqual(["interrupted", "starting", "queued"]);
  });

  test("ends the process when the turn can't be asked to stop", async () => {
    const chats = harness();
    await running(chats);
    const stuck = chats.provider.session;
    stuck.failInterrupt = new Error("the CLI stopped answering");
    chats.actor.stop(false);
    await settle();
    expect(stuck.killed).toBe(true);
    expect(chats.rows().runs.at(0)).toMatchObject({
      status: "interrupted",
      stop: { by: "person" },
    });
    expect(chats.actor.hasSession()).toBe(false);
  });

  test("by force ends the process at once, and what it reports after is nothing", async () => {
    const chats = harness();
    await running(chats);
    const killed = chats.provider.session;
    chats.actor.stop(true);
    expect(killed.killed).toBe(true);
    expect(chats.rows().runs.at(0)).toMatchObject({
      status: "interrupted",
      stop: { by: "person", force: true },
    });
    killed.finished("completed");
    killed.report({ type: "notice", level: "info", text: "late" });
    await settle();
    expect(chats.rows().runs.at(0)?.status).toBe("interrupted");
    expect(chats.items().some((item) => item.kind === "notice")).toBe(false);
  });
});

describe("requests", () => {
  test("hand the person's answer to the agent once it's written", async () => {
    const chats = harness();
    await running(chats);
    const asked = chats.provider.session.ask(APPROVAL);
    await settle();
    const id = pendingId(chats);
    expect(chats.actor.summary().needs).toBe("approval");
    expect(chats.actor.busy()).toBe(true);
    const response: Response = { kind: "approval", decision: "allow", message: null };
    chats.actor.respond(id, response);
    expect(await asked.answer).toStrictEqual(response);
    expect(pending(chats)).toStrictEqual([]);
    expect(() => {
      chats.actor.respond(id, response);
    }).toThrow(HostError);
  });

  test("refuses an answer of another kind", async () => {
    const chats = harness();
    await running(chats);
    const asked = chats.provider.session.ask(APPROVAL);
    const refused = refusal(asked.answer);
    await settle();
    const id = pendingId(chats);
    expect(() => {
      chats.actor.respond(id, { kind: "plan", decision: "implement" });
    }).toThrow(/kind `approval`/u);
    asked.withdraw();
    await settle();
    expect(chats.rows().requests.at(0)?.status).toBe("cancelled");
    expect(await refused).toMatchObject({ message: "The agent withdrew the request." });
  });

  test("are cancelled when the turn that waited on them ends", async () => {
    const chats = harness();
    await running(chats);
    const refused = refusal(chats.provider.session.ask(APPROVAL).answer);
    await settle();
    chats.provider.session.finished("interrupted");
    await settle();
    expect(chats.rows().requests.at(0)?.status).toBe("cancelled");
    expect(await refused).toMatchObject({ message: "The turn ended." });
    expect(chats.actor.busy()).toBe(false);
  });

  test("a plan is answered by the next message, or implemented", async () => {
    const chats = harness();
    await running(chats);
    const plan: RequestPayload = { kind: "plan", markdown: "1. Retry the fetch." };
    chats.provider.session.report({ type: "request", request: plan });
    chats.provider.session.finished();
    await settle();
    expect(chats.actor.summary().needs).toBe("plan");
    const refine = prompt("Retry three times at most.");
    chats.actor.send(refine, "auto");
    expect(chats.rows().requests.at(0)).toMatchObject({
      status: "answered",
      answer: { kind: "message", messageId: refine.messageId },
    });
    chats.provider.session.report({ type: "request", request: plan });
    chats.provider.session.started();
    chats.provider.session.finished();
    await settle();
    chats.actor.update({ plan: true });
    chats.actor.respond(pendingId(chats), { kind: "plan", decision: "implement" });
    expect(chats.actor.thread.plan).toBe(false);
    expect(chats.provider.session.turns.at(-1)?.text).toBe("Implement the plan above.");
  });
});

describe("the thread", () => {
  test("takes a generated title only while the seed is the title", async () => {
    const chats = harness();
    chats.provider.nextTitle = "Fix the flaky ledger test";
    chats.actor.send(prompt("The ledger test fails one run in ten."), "auto");
    await settle();
    expect(chats.actor.thread).toMatchObject({
      title: "Fix the flaky ledger test",
      titleSource: "generated",
    });
    const renamed = harness();
    renamed.provider.nextTitle = "Fix the flaky ledger test";
    renamed.actor.send(prompt("The ledger test fails one run in ten."), "auto");
    renamed.actor.update({ title: "Ledger" });
    await settle();
    expect(renamed.actor.thread).toMatchObject({ title: "Ledger", titleSource: "user" });
  });

  test("takes its title from the first words, not a message of pictures alone", async () => {
    const chats = harness();
    chats.provider.nextTitle = "What the shot shows";
    const picture = { file: "shot.png", name: "shot.png", mime: "image/png" };
    chats.actor.send({ ...prompt(""), attachments: [picture] }, "auto");
    await settle();
    expect(chats.actor.thread).toMatchObject({ title: "", titleSource: "seed" });
    chats.provider.session.started();
    chats.provider.session.finished();
    await settle();

    chats.actor.send(prompt("What's in it?"), "auto");
    await settle();

    expect(chats.actor.thread).toMatchObject({
      title: "What the shot shows",
      titleSource: "generated",
    });
  });

  test("gives a live session the settings that changed, and reopens one that can't take them",
    async () => {
      const chats = harness();
      await running(chats);
      const first = chats.provider.session;
      first.configures = "reopen";
      chats.actor.update({ model: "claude-opus-5-5", fast: false });
      await settle();
      expect(first.changes).toStrictEqual([{ model: "claude-opus-5-5" }]);
      first.finished();
      await settle();
      chats.actor.send(prompt("Again."), "auto");
      expect(first.closed).toBe(true);
      expect(chats.provider.session).not.toBe(first);
      expect(chats.provider.session.spec).toMatchObject({ resume: true, model: "claude-opus-5-5" });
    });

  test("a change the session can't take reopens it for a turn the same step starts", async () => {
    const chats = harness();
    await running(chats);
    const first = chats.provider.session;
    first.finished();
    await settle();
    first.configures = "reopen";
    chats.actor.update({ effort: "high" });
    chats.actor.send(prompt("Again."), "auto");
    expect(first.closed).toBe(true);
    expect(chats.provider.session).not.toBe(first);
  });

  test("once deleted, takes nothing that comes after", async () => {
    const chats = harness();
    chats.provider.nextTitle = "Fix the flaky ledger test";
    chats.actor.send(prompt("The ledger test fails one run in ten."), "auto");
    chats.actor.dispose();
    await settle();
    expect(chats.store.thread(chats.id)?.titleSource).toBe("seed");
  });

  test("a client catching up gets the streaming item whole", async () => {
    const chats = harness();
    await running(chats);
    const session = chats.provider.session;
    session.startsText("m1:0");
    await settle();
    const seen = chats.store.head(chats.id);
    session.report({ type: "item.appended", key: "m1:0", text: "Found it." });
    await settle();
    const caughtUp = chats.actor.open(nobody, seen);
    expect(textOf(caughtUp.items)).toBe("Found it.");
  });
});

describe("recovery", () => {
  test("settles what a host that stopped left live, and holds the queue", () => {
    const chat = thread();
    const live = run(chat, "running");
    const queued = run(chat, "queued");
    const given = { thread: chat, runs: [live, queued], requests: [request(live, APPROVAL)] };
    const chats = harness({}, given);
    expect(chats.actor.summary()).toMatchObject({ status: "idle", needs: null, queued: 1 });
    const { runs, requests } = chats.rows();
    expect(runs.map((each) => [each.status, each.stop?.by])).toStrictEqual([
      ["interrupted", "host"],
      ["queued", undefined],
    ]);
    expect(requests.at(0)?.status).toBe("expired");
    expect(chats.actor.thread.queueHeld).toBe(true);
  });
});
