import { afterEach, describe, expect, test } from "bun:test";
import { z } from "zod";

import { events } from "../src/protocol/events";
import { Thread } from "../src/protocol/model";
import { SHOWN_PLAN } from "../src/providers/fake";
import { Board } from "./progress/board";
import { closeHarnesses, harness, running } from "./support/actor";
import { settle } from "./support/wait";

afterEach(closeHarnesses);

const PLAN = {
  steps: [
    {
      id: "1",
      title: "Reproduce the flaky ledger test",
      detail: "Run the suite until it fails.",
      status: "running",
    },
    { id: "1.1", parent: "1", title: "Keep the seed that breaks it", status: "running" },
    { id: "2", title: "Fix the rounding race in settle()", detail: "Settle under one lock." },
  ],
  now: "running the ledger suite",
};

const ChangedLine = z.object({
  event: z.literal("thread.changed"),
  params: events["thread.changed"],
});

describe("a call of the progress tool", () => {
  test("is the chat's plan: sent with the thread, stored, and kept by a host that starts over",
    async () => {
      const chats = harness();
      await running(chats);
      const before = chats.actor.thread;
      const answer = await chats.provider.session.plan(PLAN);
      expect(answer).toMatchObject({ refused: false });
      expect(answer.text).toStartWith("0/2 phases done · 0%. The whole plan:");
      const plan = chats.actor.thread.progress;
      expect(plan?.phases.map((phase) => [phase.id, phase.status, phase.steps.length]))
        .toEqual([["1", "running", 1], ["2", "pending", 0]]);
      expect(plan?.now).toBe("running the ledger suite");
      expect(chats.actor.thread.updatedAt).toBe(before.updatedAt);
      const sent = chats.lines.flatMap((line) => {
        const parsed = ChangedLine.safeParse(line);
        return parsed.success && parsed.data.params.thread !== null
          ? [parsed.data.params.thread.progress]
          : [];
      });
      expect(sent.at(-1)).toEqual(plan ?? null);
      expect(Thread.parse(chats.store.thread(chats.id)).progress).toEqual(plan ?? null);
      expect(chats.load().thread.progress).toEqual(plan ?? null);
    });

  test("refused changes neither the plan nor the thread, and says why", async () => {
    const chats = harness();
    await running(chats);
    await chats.provider.session.plan(PLAN);
    const thread = chats.actor.thread;
    const lines = chats.lines.length;
    const refused = await chats.provider.session.plan({ steps: [{ id: "1", status: "done" }] });
    expect(refused.refused).toBe(true);
    expect(refused.text).toStartWith("Phase 1 has open sub-steps 1.1,");
    expect(chats.actor.thread).toBe(thread);
    expect(chats.lines).toHaveLength(lines);
  });

  test("from a session that was closed is refused, the plan as it was", async () => {
    const chats = harness();
    await running(chats);
    const session = chats.provider.session;
    session.finished();
    await settle();
    expect(chats.actor.closeSession()).toBe(true);
    expect(await session.plan(PLAN)).toEqual({
      refused: true,
      text: "This chat's session was closed, so the plan wasn't changed.",
    });
    expect(chats.actor.thread.progress).toBeNull();
  });
});

describe("the fake's plan", () => {
  test("is taken call by call, and ends with its three phases all done", () => {
    const board = new Board();
    for (const input of SHOWN_PLAN) {
      board.advance(800);
      board.text(input);
    }
    expect(board.kept.phases.map((phase) => [phase.id, phase.status, phase.steps.length]))
      .toEqual([["1", "done", 2], ["2", "done", 2], ["3", "done", 0]]);
    expect(board.kept.now).toBeNull();
  });
});
