import { afterEach, describe, expect, test } from "bun:test";
import { z } from "zod";

import { events } from "../src/protocol/events";
import { NOTHING_BESIDE } from "../src/protocol/model";
import { Background } from "../src/providers/claude/background";
import type { DriverEvent } from "../src/providers/provider";
import { bench, closeBenches, Sink } from "./claude/support";
import { init } from "./fixtures/claude/frames";
import { textTurn } from "./fixtures/claude/sessions";
import { backgroundTasks as listed } from "./fixtures/claude/tasks";
import { closeHarnesses, harness, running } from "./support/actor";
import { ended, started } from "./support/subagents";
import { settle } from "./support/wait";

const BesideLine = z.object({
  event: z.literal("thread.beside"),
  params: events["thread.beside"],
});

afterEach(closeHarnesses);
afterEach(closeBenches);

describe("what Claude runs in the background", () => {
  test("is counted from the CLI's list, leaving out subagents and what isn't activity", () => {
    const sink = new Sink();
    const background = new Background(sink.event);
    expect(background.hears(init("default"))).toBe(false);
    expect(background.hears(listed({ type: "local_bash" }, { type: "monitor" }))).toBe(true);
    background.hears(listed({ type: "local_bash" }, { type: "monitor" }, {
      type: "monitor",
      ambient: true,
    }));
    background.hears(listed({ type: "local_bash" }, { type: "monitor" }, { type: "local_agent" }));
    background.hears(listed({ type: "monitor" }));
    background.reset();
    background.reset();
    expect(sink.of("background")).toStrictEqual([
      { type: "background", tasks: 2 },
      { type: "background", tasks: 1 },
      { type: "background", tasks: 0 },
    ]);
  });
});

describe("Claude's session", () => {
  test("says what runs in the background, during its turn or after it", async () => {
    const made = bench();
    const { promptId, query } = await made.turn();
    query.send(listed({ type: "local_bash" }));
    query.send(...textTurn(promptId));
    query.send(listed({ type: "local_bash" }, { type: "monitor" }));
    await settle();
    expect(made.sink.of("background")).toStrictEqual([
      { type: "background", tasks: 1 },
      { type: "background", tasks: 2 },
    ]);
  });
});

describe("a chat's background", () => {
  test("is what its session last said, past its turn, and none once it's gone", async () => {
    const chats = harness();
    await running(chats);
    const session = chats.provider.session;
    session.report({ type: "background", tasks: 2 });
    session.finished();
    await settle();
    expect(chats.actor.summary().status).toBe("idle");
    expect(chats.actor.summary().beside).toEqual({ subagents: 0, tasks: 2 });

    session.report({ type: "session.ended", error: null });
    await settle();
    expect(chats.actor.summary().beside).toEqual(NOTHING_BESIDE);
  });
});

describe("a chat's summary", () => {
  test("says each step of what the agent's at, and what runs beside its turn", async () => {
    const chats = harness();
    const told = (): unknown => {
      const { status, beside } = chats.actor.summary();
      return [status, beside.subagents, beside.tasks];
    };
    const steps: unknown[] = [];
    await running(chats);
    const { session } = chats.provider;
    const step = async (event: DriverEvent): Promise<void> => {
      session.report(event);
      await settle();
      expect(chats.summaries.at(-1)).toStrictEqual(chats.actor.summary());
      steps.push(told());
    };
    steps.push(told());
    await step(started("a"));
    await step(started("b", "a"));
    await step({ type: "background", tasks: 1 });
    await step({ type: "turn.finished", outcome: "completed", failure: null, usage: null });
    await step(ended("completed", "b"));
    await step(started("b", "a"));
    await step(ended("completed", "b"));
    await step(ended("completed", "a"));
    await step({ type: "background", tasks: 0 });
    expect(steps).toEqual([
      ["working", 0, 0],
      ["working", 1, 0],
      ["working", 2, 0],
      ["working", 2, 1],
      ["idle", 2, 1],
      ["idle", 1, 1],
      ["idle", 2, 1],
      ["idle", 1, 1],
      ["idle", 0, 1],
      ["idle", 0, 0],
    ]);
  });

  test("says nothing runs beside once the session goes", async () => {
    const chats = harness();
    await running(chats);
    const session = chats.provider.session;
    session.report({ type: "background", tasks: 1 });
    session.finished();
    session.report({ type: "session.ended", error: null });
    await settle();
    expect(chats.summaries.at(-1)?.beside).toEqual(NOTHING_BESIDE);
    const besides = chats.lines.flatMap((line) => {
      const parsed = BesideLine.safeParse(line);
      return parsed.success ? [parsed.data.params.beside] : [];
    });
    expect(besides).toEqual([{ subagents: 0, tasks: 1 }, NOTHING_BESIDE]);
  });
});
