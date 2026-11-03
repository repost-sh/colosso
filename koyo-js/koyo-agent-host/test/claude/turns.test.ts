import { randomUUID } from "node:crypto";

import { afterEach, describe, expect, test } from "bun:test";

import { Frames } from "../../src/providers/claude/frames";
import { Turns } from "../../src/providers/claude/turns";
import type { DriverEvent } from "../../src/providers/provider";
import {
  assistant,
  blockStart,
  failed,
  init,
  message,
  messageStart,
  status,
  stream,
  success,
  textBlock,
  textDelta,
  toolUse,
} from "../fixtures/claude/frames";
import { askTurn, QUESTIONS, SKY, textTurn } from "../fixtures/claude/sessions";
import { settle } from "../support/wait";
import { bench, closeBenches, spec, toolRequest, turnOf } from "./support";

afterEach(closeBenches);

const BACKGROUND = "Claude finished background work.";

describe("a turn", () => {
  test("sends its prompt with its id as the uuid the CLI echoes", async () => {
    const { promptId, query } = await bench().turn("Why is the sky blue?");
    expect(query.prompts).toEqual([{
      type: "user",
      message: { role: "user", content: [{ type: "text", text: "Why is the sky blue?" }] },
      parent_tool_use_id: null,
      uuid: promptId,
    }]);
  });

  test("runs as recorded: started, its text, finished with what it used", async () => {
    const made = bench();
    const { promptId, query } = await made.turn();
    query.send(...textTurn(promptId));
    await settle();
    expect(made.sink.types()).toEqual([
      "session.ready",
      "turn.started",
      "item.started",
      "item.appended",
      "item.appended",
      "item.updated",
      "turn.finished",
    ]);
    expect(made.sink.of("item.updated").at(-1)?.item).toEqual({
      kind: "assistant_text",
      payload: { text: SKY },
    });
    expect(made.sink.of("turn.finished")).toEqual([{
      type: "turn.finished",
      outcome: "completed",
      failure: null,
      usage: {
        inputTokens: 2,
        outputTokens: 104,
        cacheReadTokens: 0,
        cacheCreationTokens: 28_562,
        costUsd: 0.005_891_3,
        contextTokens: 2 + 28_562 + 104,
        contextWindow: 1_000_000 - 33_000,
      },
    }]);
    expect(query.closed).toBe(false);
  });

  test("is credited the conversation's frames only once the CLI echoes its prompt", async () => {
    const made = bench();
    const { promptId, query } = await made.turn();
    query.send(
      stream(messageStart("m0")),
      stream(blockStart(0, textBlock(""))),
      stream(textDelta(0, "Held.")),
    );
    await settle();
    expect(made.sink.types()).toEqual([]);
    query.send(status("requesting"), stream(messageStart("m1"), { echo: [promptId] }));
    await settle();
    expect(made.sink.types()).toEqual(["turn.started", "item.started", "item.appended"]);
  });

  test("shows what the CLI did of its own while no turn ran as background work", async () => {
    const made = bench();
    const first = await made.turn();
    first.query.send(success([first.promptId]));
    await settle();
    // A wake turn the CLI started itself: its own prompt, its own result.
    const wake = randomUUID();
    first.query.send(
      stream(messageStart("w1"), { echo: [wake] }),
      assistant(message("w1", [textBlock("The build finished.")]), { echo: [wake] }),
      success([wake], { text: "The build finished." }),
    );
    await settle();
    const before = made.sink.events.length;
    const second = await made.turn("And now?");
    second.query.send(stream(messageStart("m2"), { echo: [second.promptId] }));
    await settle();
    expect(made.sink.events.slice(before).filter((event) => event.type !== "usage")).toEqual([
      { type: "turn.started", promptId: second.promptId },
      { type: "notice", level: "info", text: BACKGROUND },
    ]);
  });

  test("drops what another turn held when that turn's result comes", async () => {
    const made = bench();
    const { promptId, query } = await made.turn();
    const wake = randomUUID();
    query.send(
      stream(messageStart("w1")),
      stream(blockStart(0, textBlock(""))),
      stream(textDelta(0, "Wake text.")),
      success([wake], { text: "Wake text." }),
      stream(messageStart("m1"), { echo: [promptId] }),
    );
    await settle();
    expect(made.sink.types()).toEqual(["turn.started", "notice"]);
    expect(made.sink.of("item.appended")).toEqual([]);
  });
});

describe("a prompt's lifecycle", () => {
  /** Turns reporting to `events`, each finished one noted there too. */
  function turns(): { turns: Turns; events: (DriverEvent | "finished")[] } {
    const events: (DriverEvent | "finished")[] = [];
    const emit = (event: DriverEvent): void => {
      events.push(event);
    };
    return {
      turns: new Turns(emit, new Frames(emit), () => {
        events.push("finished");
      }),
      events,
    };
  }

  test("started credits the turn", () => {
    const { turns: running, events } = turns();
    running.start("p1", false);
    running.sent("p1");
    running.lifecycle({ type: "command_lifecycle", command_uuid: "p1", state: "queued" });
    expect(events).toEqual([]);
    running.lifecycle({ type: "command_lifecycle", command_uuid: "p1", state: "started" });
    expect(events).toEqual([{ type: "turn.started", promptId: "p1" }]);
  });

  test("cancelled before the CLI took it ends the turn interrupted", () => {
    const { turns: running, events } = turns();
    running.start("p1", false);
    running.sent("p1");
    running.lifecycle({ type: "command_lifecycle", command_uuid: "p1", state: "cancelled" });
    expect(events).toEqual([
      { type: "turn.finished", outcome: "interrupted", failure: null, usage: null },
      "finished",
    ]);
  });

  test("completed with no turn for it fails the turn", () => {
    const { turns: running, events } = turns();
    running.start("p1", false);
    running.sent("p1");
    running.lifecycle({ type: "command_lifecycle", command_uuid: "p1", state: "completed" });
    expect(events).toEqual([{
      type: "turn.finished",
      outcome: "failed",
      failure: {
        kind: "process",
        message: "Claude acknowledged the message but never started a turn for it.",
        retryAt: null,
      },
      usage: null,
    }, "finished"]);
  });
});

describe("a steer", () => {
  test("goes to the CLI to land at the turn's next step", async () => {
    const made = bench();
    const { query } = await made.turn();
    const steerId = randomUUID();
    expect(made.session.steer(turnOf("Also, what is 2+2?", steerId))).toBe(true);
    const steered = await query.prompt(2);
    expect(steered).toMatchObject({ uuid: steerId, priority: "next" });
  });

  test("the CLI took into the turn ends with it, as recorded", async () => {
    const made = bench();
    const { promptId, query } = await made.turn();
    const steerId = randomUUID();
    made.session.steer(turnOf("Also, what is 2+2?", steerId));
    await query.prompt(2);
    query.send(
      init("acceptEdits", { echo: [promptId] }),
      stream(messageStart("m1"), { echo: [promptId] }),
      status("requesting"),
      stream(messageStart("m2"), { echo: [promptId, steerId] }),
      success([promptId, steerId], { text: "slept\n\n2 + 2 = 4" }),
    );
    await settle();
    expect(made.sink.of("turn.finished").map((event) => event.outcome)).toEqual(["completed"]);
  });

  test("the CLI hadn't taken by the turn's result runs right after, in the same turn", async () => {
    const made = bench();
    const { promptId, query } = await made.turn();
    const steerId = randomUUID();
    made.session.steer(turnOf("Also, what is 2+2?", steerId));
    await query.prompt(2);
    query.send(
      stream(messageStart("m1"), { echo: [promptId] }),
      success([promptId], { cost: 0.01 }),
    );
    await settle();
    expect(made.sink.of("turn.finished")).toEqual([]);
    query.send(
      stream(messageStart("m2"), { echo: [steerId] }),
      assistant(message("m2", [textBlock("4")]), { echo: [steerId] }),
      success([steerId], { text: "4", cost: 0.02 }),
    );
    await settle();
    const finished = made.sink.of("turn.finished");
    expect(finished.map((event) => event.outcome)).toEqual(["completed"]);
    expect(finished[0]?.usage?.costUsd).toBeCloseTo(0.03);
    expect(made.sink.of("item.updated").at(-1)?.item)
      .toEqual({ kind: "assistant_text", payload: { text: "4" } });
  });

  test("is refused once the turn is being stopped", async () => {
    const made = bench();
    await made.turn();
    await made.session.interrupt();
    expect(made.session.steer(turnOf("More."))).toBe(false);
  });
});

describe("a stop", () => {
  test("interrupts the turn and keeps the process when the result comes, as recorded", async () => {
    const made = bench();
    const { promptId, query } = await made.turn("Count from 1 to 300.");
    query.send(
      init("auto", { echo: [promptId] }),
      stream(messageStart("m1"), { echo: [promptId] }),
      stream(blockStart(0, textBlock(""))),
      stream(textDelta(0, "1\n2\n3\n")),
    );
    await settle();
    await made.session.interrupt();
    expect(query.controls).toEqual([{ method: "interrupt" }]);
    query.send(failed([promptId], ["[ede_diagnostic] result_type=user"], "aborted_streaming"));
    await settle();
    expect(made.sink.of("turn.finished").map((event) => event.outcome)).toEqual(["interrupted"]);
    // Past the time a stop has to end: nothing closes a process whose turn did end.
    await Bun.sleep(80);
    expect(query.closed).toBe(false);
    const next = await made.turn("Reply with just 'ok'.");
    expect(next.query).toBe(query);
  });

  test("closes the process when no result comes in time", async () => {
    const made = bench();
    const { query } = await made.turn();
    await made.session.interrupt();
    expect(query.closed).toBe(false);
    await query.promptsEnded();
    await settle();
    expect(query.closed).toBe(true);
    expect(made.sink.of("session.ended")).toEqual([{ type: "session.ended", error: null }]);
  });

  test("closes the process when a message of the turn is still queued behind it", async () => {
    const made = bench();
    const { query } = await made.turn();
    const steerId = randomUUID();
    made.session.steer(turnOf("Then this.", steerId));
    await query.prompt(2);
    query.receipt = { still_queued: [steerId] };
    await made.session.interrupt();
    await settle();
    expect(query.closed).toBe(true);
  });

  test("before the prompt reached the CLI ends the turn at once", async () => {
    const made = bench();
    await made.query();
    made.session.start(turnOf("Never mind."));
    await made.session.interrupt();
    expect(made.sink.of("turn.finished").map((event) => event.outcome)).toEqual(["interrupted"]);
    expect(made.queries[0]?.controls).toEqual([]);
  });
});

describe("a tool the CLI asks to run", () => {
  test("is put to the person, its item started first", async () => {
    const made = bench();
    const { promptId, query } = await made.turn();
    query.send(stream(messageStart("m1"), { echo: [promptId] }));
    await settle();
    const answer = query.options.canUseTool?.("Bash", { command: "ls" }, toolRequest({
      toolUseID: "toolu_ls",
    }));
    await settle();
    expect(made.sink.of("item.started").map((event) => event.key)).toEqual(["toolu_ls"]);
    made.sink.asked[0]?.answer({ kind: "approval", decision: "allow", message: null });
    expect(await answer).toEqual({ behavior: "allow", updatedInput: { command: "ls" } });
  });

  test("is denied when no turn of the chat runs", async () => {
    const made = bench();
    const query = await made.query();
    const answer = await query.options.canUseTool?.("Bash", { command: "ls" }, toolRequest());
    expect(answer).toEqual({ behavior: "deny", message: "No turn of this chat is running." });
    expect(made.sink.asked).toEqual([]);
  });

  test("AskUserQuestion, as recorded, is asked of the person live", async () => {
    const made = bench();
    const { promptId, query } = await made.turn();
    query.send(...askTurn(promptId));
    await settle();
    const answer = query.options.canUseTool?.("AskUserQuestion", QUESTIONS, toolRequest());
    await settle();
    expect(made.sink.asked[0]?.payload).toMatchObject({ kind: "question", live: true });
    // The question stands in the timeline for the call, which has no item of its own.
    expect(made.sink.of("item.started")).toEqual([]);
    const answers = { "Do you prefer tabs or spaces?": ["Tabs"] };
    made.sink.asked[0]?.answer({ kind: "question", answers });
    expect(await answer).toMatchObject({
      behavior: "allow",
      updatedInput: { answers: { "Do you prefer tabs or spaces?": "Tabs" } },
    });
  });
});

describe("plan mode", () => {
  test("left by a turn out of it restores the chat's mode before the prompt", async () => {
    const made = bench(spec({ permission: "accept_edits", plan: true }));
    const { promptId, query } = await made.turn("Make a two-step plan.");
    query.send(
      init("plan", { echo: [promptId] }),
      assistant(message("m1", [toolUse("toolu_plan", "ExitPlanMode", { plan: "# Plan" })]), {
        echo: [promptId],
      }),
      success([promptId]),
    );
    await settle();
    expect(made.session.configure({ plan: false })).toBe("applied");
    await made.turn("Thanks.");
    expect(query.journal).toEqual(["prompt", "setPermissionMode", "prompt"]);
    expect(query.controls).toEqual([{ method: "setPermissionMode", mode: "acceptEdits" }]);
  });

  test("a denied ExitPlanMode left the CLI in is left before the next turn", async () => {
    const made = bench(spec({ permission: "ask" }));
    const { promptId, query } = await made.turn();
    // The CLI stays in plan mode after it entered it and its ExitPlanMode was denied.
    query.send(
      init("default", { echo: [promptId] }),
      status(null, { permissionMode: "plan" }),
      success([promptId]),
    );
    await settle();
    await made.turn("Next.");
    expect(query.controls).toEqual([{ method: "setPermissionMode", mode: "default" }]);
  });
});
