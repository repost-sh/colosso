// What the actor decides at the provider seam, so that every provider gets it right without
// doing it itself (docs/chat.md 4.5, 4.6): how a run's end closes what it left open, the mode
// a session opens in, which turn compacts, and what title a model's reply makes.

import { afterEach, describe, expect, test } from "bun:test";

import type { FakeSession } from "../src/providers/fake";
import type { ItemBody, Outcome } from "../src/providers/provider";
import { closeHarnesses, type Harness, harness, prompt, running } from "./support/actor";
import { settle } from "./support/wait";

afterEach(closeHarnesses);

/** One open item of each kind a provider can leave open, by the key it started it under. */
const OPEN: readonly (readonly [string, ItemBody])[] = [
  ["text", { kind: "assistant_text", payload: { text: "Half a" } }],
  ["thinking", { kind: "reasoning", payload: { text: "Hm" } }],
  ["tool", {
    kind: "tool",
    payload: {
      name: "Bash",
      family: "command",
      label: "ls",
      input: null,
      output: null,
      exitCode: null,
      patch: null,
      files: [],
      server: null,
    },
  }],
  ["compaction", { kind: "compaction", payload: { trigger: "auto", before: null, after: null } }],
];

function startsAllOpen(session: FakeSession): void {
  for (const [key, item] of OPEN) {
    session.report({ type: "item.started", key, item });
  }
}

/** Each item's kind and status, but the message that started the turn. */
function statuses(chats: Harness): [string, string][] {
  return chats.items()
    .filter((item) => item.kind !== "user_message")
    .map((item) => [item.kind, item.status]);
}

describe("a run's end", () => {
  test("after it completed fails a tool or a compaction left open", async () => {
    const chats = harness();
    await running(chats);
    startsAllOpen(chats.provider.session);
    chats.provider.session.finished();
    await settle();
    expect(statuses(chats)).toStrictEqual([
      ["assistant_text", "completed"],
      ["reasoning", "completed"],
      ["tool", "failed"],
      ["compaction", "failed"],
    ]);
  });

  test.each<Outcome>(["interrupted", "failed"])(
    "after it ended %s leaves everything left open interrupted",
    async (outcome) => {
      const chats = harness();
      await running(chats);
      startsAllOpen(chats.provider.session);
      chats.provider.session.finished(outcome);
      await settle();
      expect(statuses(chats).map(([, status]) => status))
        .toStrictEqual(OPEN.map(() => "interrupted"));
    },
  );

  test("keeps the text that streamed into an item it closes", async () => {
    const chats = harness();
    await running(chats);
    const session = chats.provider.session;
    session.startsText("m1:0");
    session.report({ type: "item.appended", key: "m1:0", text: "1\n2" });
    session.finished("interrupted");
    await settle();
    expect(chats.items().at(-1)).toMatchObject({
      kind: "assistant_text",
      status: "interrupted",
      payload: { text: "1\n2" },
    });
  });
});

describe("a session's mode", () => {
  test("is the chat's, else Koyo's default, else null when neither names one", () => {
    const neither = harness();
    neither.actor.send(prompt("Go."), "auto");
    expect(neither.provider.session.spec.permission).toBeNull();
    const koyos = harness();
    koyos.config = { claudePath: null, defaultMode: "auto", koyoTools: null };
    koyos.actor.send(prompt("Go."), "auto");
    expect(koyos.provider.session.spec.permission).toBe("auto");
    const own = harness();
    own.config = { claudePath: null, defaultMode: "auto", koyoTools: null };
    own.actor.update({ permission: "ask" });
    own.actor.send(prompt("Go."), "auto");
    expect(own.provider.session.spec.permission).toBe("ask");
  });

  test("changes on a live session the same way, and only when the chat's changes", async () => {
    const chats = harness();
    chats.config = { claudePath: null, defaultMode: "auto", koyoTools: null };
    await running(chats);
    const session = chats.provider.session;
    chats.actor.update({ permission: "ask" });
    chats.actor.update({ permission: null });
    expect(session.changes).toStrictEqual([{ permission: "ask" }, { permission: "auto" }]);
    // Koyo's default changing leaves a live session in the mode it has.
    chats.config = { claudePath: null, defaultMode: "accept_edits", koyoTools: null };
    chats.actor.update({ model: "claude-opus-5-5" });
    expect(session.changes.at(-1)).toStrictEqual({ model: "claude-opus-5-5" });
  });
});

describe("a compact turn", () => {
  test("is marked on the session's input; another turn and a steer never are", async () => {
    const chats = harness();
    chats.actor.send(prompt("/compact focus on the ledger"), "auto");
    const session = chats.provider.session;
    expect(session.turns.at(0)?.compact).toBe(true);
    session.finished();
    await settle();
    chats.actor.send(prompt("/compacted is no command"), "auto");
    expect(session.turns.at(1)?.compact).toBe(false);
    session.started();
    await settle();
    chats.actor.send(prompt("/compact"), "auto");
    expect(session.steers.map((each) => [each.text, each.compact]))
      .toStrictEqual([["/compact", false]]);
  });
});

describe("a generated title", () => {
  test("is made of the model's reply", async () => {
    const chats = harness();
    chats.provider.nextTitle = "\n\"**Fix the flaky ledger test in CI now.**\"\n";
    chats.actor.send(prompt("The ledger test fails one run in ten."), "auto");
    await settle();
    expect(chats.actor.thread).toMatchObject({
      title: "Fix the flaky ledger test in",
      titleSource: "generated",
    });
  });

  test("isn't written when nothing is left of the reply", async () => {
    const chats = harness();
    chats.provider.nextTitle = "  \n";
    chats.actor.send(prompt("The ledger test fails one run in ten."), "auto");
    await settle();
    expect(chats.actor.thread).toMatchObject({
      title: "The ledger test fails one run in ten.",
      titleSource: "seed",
    });
  });
});
