import { randomUUID } from "node:crypto";

import { afterAll, describe, expect, test } from "bun:test";

import { PROGRESS_TOOL } from "../../src/progress/prompt";
import { ClaudeProvider } from "../../src/providers/claude/provider";
import { removeScratch, scratch } from "../support/scratch";
import { Sink, spec, turnOf } from "./support";

const LIVE = process.env["KOYO_LIVE_CLAUDE"] === "1";
const TURN_MS = 120_000;

afterAll(removeScratch);

describe.skipIf(!LIVE)("the real Claude CLI", () => {
  test("is ready, and runs a turn through the driver to its end", async () => {
    const cwd = scratch();
    const claude = new ClaudeProvider();
    expect(await claude.status(false)).toMatchObject({ state: "ready" });
    const sink = new Sink();
    const session = claude.open(spec({ cwd, permission: "ask", model: "haiku" }), sink);
    const promptId = randomUUID();
    session.start(turnOf("Reply with the single word pong, nothing else.", promptId));
    await sink.finished;
    await session.close();
    expect(sink.of("session.ready")[0]).toMatchObject({ type: "session.ready", mode: "ask" });
    expect(sink.of("turn.started")).toEqual([{ type: "turn.started", promptId }]);
    const said = sink.of("item.updated").flatMap((event) =>
      event.item.kind === "assistant_text" ? [event.item.payload.text] : []);
    expect(said.join(" ").toLowerCase()).toContain("pong");
    expect(sink.of("turn.finished")).toMatchObject([{ outcome: "completed", failure: null }]);
  }, TURN_MS + 30_000);

  test("keeps a plan with the progress tool, its call shown by what it did", async () => {
    const cwd = scratch();
    const claude = new ClaudeProvider();
    const sink = new Sink();
    const opened = spec({ cwd, permission: "ask", model: "haiku", progress: true });
    const session = claude.open(opened, sink);
    session.start(turnOf(
      `Call the ${PROGRESS_TOOL} tool once to declare a plan of two phases for a short poem about `
      + "tea: phase 1 \"Draft the tea poem's first verse\", running, and phase 2 \"Settle the tea "
      + "poem's closing line\", pending, each with a one-sentence detail. Then call it again to "
      + "mark phase 1 done. Write no poem and use no other tool; reply with the single word done.",
    ));
    await sink.finished;
    await session.close();
    expect(sink.planned.length).toBeGreaterThan(0);
    expect(sink.plan.current()?.phases.map((phase) => phase.status)).toEqual(["done", "pending"]);
    const labels = sink.of("item.started").flatMap((event) => {
      const { item } = event;
      return item.kind === "tool" && item.payload.name === PROGRESS_TOOL
        ? [item.payload.label]
        : [];
    });
    expect(labels[0]).toBe("Plan: 2 phases");
  }, TURN_MS + 30_000);
});
