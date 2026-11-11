// The driver against the person's real `claude`, run only with KOYO_LIVE_CLAUDE=1: it signs in
// as them and spends a little of their usage. The chat runs in a scratch folder in the
// checkout; Claude Code keeps its transcript in ~/.claude/projects/, as it does for any chat.

import { randomUUID } from "node:crypto";

import { afterAll, describe, expect, test } from "bun:test";

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
});
