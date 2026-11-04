import { describe, expect, test } from "bun:test";

import {
  apiRetry,
  assistant,
  blockStart,
  blockStop,
  boundary,
  informational,
  message,
  messageDelta,
  messageStart,
  rateLimit,
  replay,
  status,
  stream,
  success,
  textBlock,
  textDelta,
  thinkingBlock,
  thinkingDelta,
} from "../fixtures/claude/frames";
import { SKY, textTurn } from "../fixtures/claude/sessions";
import { mapped, ofType } from "./mapped";

describe("text and thinking", () => {
  test("a text block streams into one item, which its snapshot settles", () => {
    const { events, send } = mapped();
    send(...textTurn("p1").filter((frame) => frame.type !== "result"));
    expect(events).toEqual([
      {
        type: "item.started",
        key: "msg_011CggdQ7qq4WaL7TjL1qvXv:text:0",
        parentKey: null,
        item: { kind: "assistant_text", payload: { text: "" } },
      },
      {
        type: "item.appended",
        key: "msg_011CggdQ7qq4WaL7TjL1qvXv:text:0",
        text: "Sunlight holds every color, ",
      },
      {
        type: "item.appended",
        key: "msg_011CggdQ7qq4WaL7TjL1qvXv:text:0",
        text: "but air scatters blue the most.",
      },
      {
        type: "item.updated",
        key: "msg_011CggdQ7qq4WaL7TjL1qvXv:text:0",
        item: { kind: "assistant_text", payload: { text: SKY } },
        status: "completed",
      },
    ]);
  });

  test("thinking is reasoning, and a thinking block whose text is withheld shows nothing", () => {
    const { events, send } = mapped();
    send(
      stream(messageStart("m1")),
      stream(blockStart(0, thinkingBlock())),
      stream(thinkingDelta(0, "")),
      assistant(message("m1", [thinkingBlock()])),
      stream(blockStop(0)),
      stream(blockStart(1, thinkingBlock())),
      stream(thinkingDelta(1, "Rayleigh scattering.")),
      assistant(message("m1", [thinkingBlock()])),
      stream(blockStop(1)),
    );
    expect(events).toEqual([
      {
        type: "item.started",
        key: "m1:thinking:1",
        parentKey: null,
        item: { kind: "reasoning", payload: { text: "" } },
      },
      { type: "item.appended", key: "m1:thinking:1", text: "Rayleigh scattering." },
      {
        type: "item.updated",
        key: "m1:thinking:1",
        item: { kind: "reasoning", payload: { text: "Rayleigh scattering." } },
        status: "completed",
      },
    ]);
  });

  test("a block is found by its place among its kind, as each snapshot has one block", () => {
    const { events, send } = mapped();
    send(
      stream(messageStart("m1")),
      stream(blockStart(0, thinkingBlock())),
      stream(thinkingDelta(0, "Think.")),
      assistant(message("m1", [thinkingBlock("Think.")])),
      stream(blockStop(0)),
      stream(blockStart(1, textBlock(""))),
      stream(textDelta(1, "Say.")),
      assistant(message("m1", [textBlock("Say, whole.")])),
      stream(blockStop(1)),
    );
    expect(ofType(events, "item.updated").map((event) => [event.key, event.item])).toEqual([
      ["m1:thinking:0", { kind: "reasoning", payload: { text: "Think." } }],
      ["m1:text:0", { kind: "assistant_text", payload: { text: "Say, whole." } }],
    ]);
  });

  test("a snapshot of a block that never streamed shows it whole", () => {
    const { events, send } = mapped();
    send(assistant(message("m1", [textBlock("All at once.")])));
    expect(events).toEqual([
      {
        type: "item.started",
        key: "m1:text:0",
        parentKey: null,
        item: { kind: "assistant_text", payload: { text: "All at once." } },
      },
      {
        type: "item.updated",
        key: "m1:text:0",
        item: { kind: "assistant_text", payload: { text: "All at once." } },
        status: "completed",
      },
    ]);
  });

  test("what the turn left open is the actor's to close; its text isn't kept after", () => {
    const { frames, events, send } = mapped();
    const streamed = [
      stream(messageStart("m1")),
      stream(blockStart(0, textBlock(""))),
      stream(textDelta(0, "1\n2")),
    ];
    send(...streamed);
    const shown = events.length;
    frames.end();
    expect(events).toHaveLength(shown);
    // Forgotten, the block starts anew rather than growing what the last turn wrote.
    send(...streamed);
    expect(events.at(shown)).toMatchObject({ type: "item.started", key: "m1:text:0" });
  });

  test("a subagent's own text isn't shown", () => {
    const { events, send } = mapped();
    send(
      stream(messageStart("sub1"), { parent: "toolu_agent" }),
      stream(blockStart(0, textBlock("")), { parent: "toolu_agent" }),
      stream(textDelta(0, "Searching."), { parent: "toolu_agent" }),
      assistant(message("sub1", [textBlock("Searching.")]), { parent: "toolu_agent" }),
    );
    expect(events).toEqual([]);
  });

  test("a replay of what was said, as a compaction's summary, isn't shown again", () => {
    const { events, send } = mapped();
    send(replay("This session is being continued from a previous conversation."));
    expect(events).toEqual([]);
  });
});

describe("usage", () => {
  test("is counted from message_start and message_delta, with the context it leaves", () => {
    const { usages, send } = mapped();
    send(
      stream(messageStart("m1", { input: 2, output: 1, cacheRead: 100, cacheCreation: 28_562 })),
      stream(messageDelta(104)),
    );
    expect(usages.at(-1)).toEqual({
      type: "usage",
      usage: {
        inputTokens: 2,
        outputTokens: 104,
        cacheReadTokens: 100,
        cacheCreationTokens: 28_562,
        costUsd: null,
        contextTokens: 2 + 100 + 28_562 + 104,
        contextWindow: null,
      },
    });
  });
});

describe("compaction", () => {
  test("a /compact turn is one item, from its status, result and boundary frames", () => {
    const { frames, events, usages, send } = mapped(true);
    send(
      status("compacting"),
      status(null, { compact_result: "success" }),
      boundary(28_812, 9578),
      replay("This session is being continued from a previous conversation."),
    );
    expect(events).toEqual([
      {
        type: "item.started",
        key: "turn-1:compaction:1",
        parentKey: null,
        item: { kind: "compaction", payload: { trigger: "manual", before: null, after: null } },
      },
      {
        type: "item.updated",
        key: "turn-1:compaction:1",
        item: { kind: "compaction", payload: { trigger: "manual", before: null, after: null } },
        status: "completed",
      },
      {
        type: "item.updated",
        key: "turn-1:compaction:1",
        item: { kind: "compaction", payload: { trigger: "manual", before: 28_812, after: 9578 } },
        status: "completed",
      },
    ]);
    expect(frames.compacted).toBe(true);
    expect(usages.at(-1)).toMatchObject({ usage: { contextTokens: 9578 } });
  });

  test("a /compact that didn't compact says why in its result, as a notice", () => {
    const { frames, events } = mapped(true);
    const said = "Not enough messages to compact.";
    expect(frames.result(success(["p1"], { text: said }), false).ending.outcome).toBe("completed");
    expect(events).toEqual([{ type: "notice", level: "info", text: said }]);
  });

  test("the result of any other turn says nothing of it", () => {
    const { frames, events } = mapped();
    frames.result(success(["p1"], { text: "Done." }), false);
    expect(events).toEqual([]);
  });

  test("a compaction that failed says why", () => {
    const { events, send } = mapped();
    send(
      status("compacting"),
      status(null, { compact_result: "failed", compact_error: "Not enough messages to compact." }),
    );
    expect(events.slice(1)).toEqual([
      {
        type: "item.updated",
        key: "turn-1:compaction:1",
        item: { kind: "compaction", payload: { trigger: "auto", before: null, after: null } },
        status: "failed",
      },
      { type: "notice", level: "error", text: "Not enough messages to compact." },
    ]);
  });
});

describe("notices", () => {
  test("the API retrying is said once a turn", () => {
    const { events, send } = mapped();
    send(apiRetry(1), apiRetry(2), apiRetry(3));
    expect(events).toEqual([{
      type: "notice",
      level: "warning",
      text: "Claude's API answered with an error; Claude Code is trying again.",
    }]);
  });

  test("no answer from the API at all is said as what it is", () => {
    const { events, send } = mapped();
    send(apiRetry(1, null));
    expect(events).toEqual([{
      type: "notice",
      level: "warning",
      text: "Claude's API can't be reached; Claude Code is trying again.",
    }]);
  });

  test("the CLI's warnings and notices are shown, its info and suggestions aren't", () => {
    const { events, send } = mapped();
    send(
      informational("info", "Loaded 3 skills."),
      informational("suggestion", "Try /review."),
      informational("warning", "Your settings file has an error."),
      informational("notice", "Auto mode is on."),
    );
    expect(events).toEqual([
      { type: "notice", level: "warning", text: "Your settings file has an error." },
      { type: "notice", level: "info", text: "Auto mode is on." },
    ]);
  });

  test("a message of no model is the CLI's own words, an error when it says one", () => {
    const { events, send } = mapped();
    send(assistant(message("m1", [textBlock("API Error: 500 Internal error")], "<synthetic>"), {
      error: "server_error",
    }));
    expect(events).toEqual([
      { type: "notice", level: "error", text: "API Error: 500 Internal error" },
    ]);
  });

  test("a usage limit gives the time it resets, said once", () => {
    const { frames, events, send } = mapped();
    send(
      rateLimit({ status: "allowed", resetsAt: 1_793_865_600, overageStatus: "rejected" }),
      rateLimit({ status: "rejected", resetsAt: 1_793_865_600, overageStatus: "rejected" }),
      rateLimit({ status: "rejected", resetsAt: 1_793_865_600, overageStatus: "rejected" }),
    );
    expect(events).toEqual([
      { type: "notice", level: "warning", text: "Claude's usage limit is reached." },
    ]);
    expect(frames.signs(false).limited).toEqual({ retryAt: "2026-11-05T08:00:00.000Z" });
  });

  test("a usage limit the overage covers isn't one", () => {
    const { frames, events, send } = mapped();
    send(rateLimit({ status: "rejected", resetsAt: 1_793_865_600, overageStatus: "allowed" }));
    expect(events).toEqual([]);
    expect(frames.signs(false).limited).toBeNull();
  });
});
