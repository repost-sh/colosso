// A session's subagents, frame by frame as a real CLI sent them (0.3.317, recorded running one
// subagent in the foreground and one in the background).

import { afterEach, describe, expect, test } from "bun:test";

import type { DriverEvent, ItemEvent } from "../../src/providers/provider";
import {
  assistant,
  message,
  messageStart,
  stream,
  success,
  textBlock,
  thinkingBlock,
  toolResults,
  toolUse,
} from "../fixtures/claude/frames";
import {
  saidToSubagent,
  shellStarted,
  taskNotification,
  taskProgress,
  taskStarted,
  taskUpdated,
} from "../fixtures/claude/tasks";
import { settle, until } from "../support/wait";
import { ofType } from "./mapped";
import { type Bench, bench, closeBenches, type FakeQuery, toolRequest } from "./support";

afterEach(closeBenches);

const CALL = "toolu_agent";
const TASK = "a2c5ec90b454f99e7";
const PROMPT = "Count the files.";
const AGENT = { description: "Count files", subagent_type: "general-purpose", prompt: PROMPT };
const LAUNCHED = [
  "Async agent launched successfully. (This tool result is internal metadata.)",
  "agentId: a2c5ec90b454f99e7 (internal ID - do not mention to user.)",
].join("\n");
const ACK = {
  isAsync: true,
  status: "async_launched",
  agentId: TASK,
  description: "Count files",
  resolvedModel: "claude-opus-5-5",
  prompt: PROMPT,
  outputFile: "/tmp/agent.output",
};
const REPORT = {
  status: "completed",
  prompt: PROMPT,
  agentId: TASK,
  content: [
    { type: "text", text: "<tools-used-before-summary>Bash</tools-used-before-summary>\n" },
    { type: "text", text: "There's one file." },
  ],
  resolvedModel: "claude-opus-5-5",
  totalDurationMs: 4988,
  totalTokens: 23_099,
  totalToolUseCount: 1,
};

/** A running turn whose CLI has just called the Agent tool. */
async function called(
  input: unknown = AGENT,
): Promise<{ made: Bench; query: FakeQuery; promptId: string }> {
  const made = bench();
  const { promptId, query } = await made.turn();
  query.send(
    stream(messageStart("m1"), { echo: [promptId] }),
    assistant(message("m1", [toolUse(CALL, "Agent", input)]), { echo: [promptId] }),
  );
  await settle();
  return { made, query, promptId };
}

/** What was said of the timeline of subagent `CALL`. */
function timeline(events: readonly DriverEvent[]): ItemEvent[] {
  return ofType([...events], "subagent.event").map((event) => event.event);
}

describe("a subagent", () => {
  test("starts with its Agent call, which is no item of the turn's", async () => {
    const { made } = await called();
    expect(made.sink.of("subagent.started")).toEqual([{
      type: "subagent.started",
      key: CALL,
      parentKey: null,
      subagent: {
        description: "Count files",
        agentType: "general-purpose",
        prompt: PROMPT,
        model: null,
        effort: null,
      },
    }]);
    expect(made.sink.of("item.started")).toEqual([]);
  });

  test("names the model and effort it was asked to run with", async () => {
    const { made } = await called({ ...AGENT, model: "haiku", effort: "low" });
    expect(made.sink.of("subagent.started").at(0)?.subagent).toMatchObject({
      model: "haiku",
      effort: "low",
    });
  });

  test("shows what it said and did in its own timeline, not the prompt again", async () => {
    const { made, query } = await called();
    query.send(
      taskStarted(TASK, CALL),
      saidToSubagent(CALL, PROMPT),
      assistant(message("s1", [thinkingBlock("")]), { parent: CALL }),
      assistant(message("s1", [textBlock("I'll list them.")]), { parent: CALL }),
      assistant(message("s1", [toolUse("bash", "Bash", { command: "ls" })]), { parent: CALL }),
      toolResults([{ id: "bash", content: "hello.txt" }], undefined, CALL),
    );
    await settle();
    expect(timeline(made.sink.events).map((event) => [event.type, "key" in event && event.key]))
      .toEqual([
        ["item.updated", "s1:text:0"],
        ["item.started", "bash"],
        ["item.updated", "bash"],
      ]);
    expect(made.sink.of("item.started")).toEqual([]);
  });

  test("says what's said to it after it started", async () => {
    const { made, query } = await called();
    query.send(saidToSubagent(CALL, PROMPT), saidToSubagent(CALL, "Count the folders too."));
    await settle();
    expect(timeline(made.sink.events).at(0)).toMatchObject({
      type: "item.updated",
      item: { kind: "user_message", payload: { text: "Count the folders too." } },
    });
  });

  test("says what it's doing and what it used", async () => {
    const { made, query } = await called();
    query.send(taskStarted(TASK, CALL), taskProgress(TASK, CALL));
    await settle();
    expect(made.sink.of("subagent.changed")).toEqual([{
      type: "subagent.changed",
      key: CALL,
      change: {
        progress: "Running List files in the working directory",
        usage: { tokens: 19_613, tools: 1 },
      },
    }]);
  });

  test("leaves a shell it ran to its own timeline, as the tool call it is", async () => {
    const { made, query } = await called();
    query.send(
      taskStarted(TASK, CALL),
      shellStarted("bo2i3e0a3", "toolu_bash"),
      taskNotification("bo2i3e0a3", "toolu_bash", "Wait for 8 seconds"),
    );
    await settle();
    expect(made.sink.of("subagent.started")).toHaveLength(1);
    expect(made.sink.of("subagent.ended")).toEqual([]);
  });
});

describe("in the foreground, a subagent", () => {
  test("ends with its notification, and the call's result adds nothing but its model", async () => {
    const { made, query } = await called();
    query.send(
      taskStarted(TASK, CALL),
      taskNotification(TASK, CALL, "There's one file."),
      toolResults([{ id: CALL, content: "[Subagent hand-back] There's one file." }], REPORT),
    );
    await settle();
    expect(made.sink.of("subagent.ended")).toEqual([{
      type: "subagent.ended",
      key: CALL,
      outcome: "completed",
      summary: "There's one file.",
      usage: { tokens: 23_219, tools: 1 },
    }]);
    expect(made.sink.of("subagent.changed")).toEqual([
      { type: "subagent.changed", key: CALL, change: { model: "claude-opus-5-5" } },
    ]);
  });

  test("with no notification ends with the call's result, without the CLI's notes", async () => {
    const { made, query } = await called();
    const content = "[Subagent hand-back] There's one file.";
    query.send(toolResults([{ id: CALL, content }], REPORT));
    await settle();
    expect(made.sink.of("subagent.ended")).toEqual([{
      type: "subagent.ended",
      key: CALL,
      outcome: "completed",
      summary: "There's one file.",
      usage: { tokens: 23_099, tools: 1 },
    }]);
  });

  test("whose call failed has failed", async () => {
    const { made, query } = await called();
    query.send(toolResults([{ id: CALL, content: "No such agent.", isError: true }]));
    await settle();
    expect(made.sink.of("subagent.ended").at(0)).toMatchObject({
      outcome: "failed",
      summary: "No such agent.",
    });
  });

  test("still running as its turn ends goes with the turn", async () => {
    const { made, query, promptId } = await called();
    query.send(taskStarted(TASK, CALL), success([promptId]));
    await settle();
    const ended = made.sink.events.findIndex((event) => event.type === "subagent.ended");
    const finished = made.sink.events.findIndex((event) => event.type === "turn.finished");
    expect(made.sink.of("subagent.ended").at(0)).toMatchObject({ outcome: "failed" });
    expect(ended).toBeLessThan(finished);
  });
});

describe("in the background, a subagent", () => {
  /** A subagent launched, and the turn that launched it over. */
  async function launched(): Promise<Awaited<ReturnType<typeof called>>> {
    const started = await called();
    const { query, promptId } = started;
    query.send(
      taskStarted(TASK, CALL, { background: true }),
      toolResults([{ id: CALL, content: LAUNCHED }], ACK),
      success([promptId]),
    );
    await settle();
    return started;
  }

  test("isn't ended by the call's result, which says it was launched", async () => {
    const { made } = await launched();
    expect(made.sink.of("subagent.ended")).toEqual([]);
    expect(JSON.stringify(made.sink.events)).not.toContain("Async agent launched");
    expect(made.sink.of("subagent.changed")).toEqual([
      { type: "subagent.changed", key: CALL, change: { model: "claude-opus-5-5" } },
    ]);
  });

  test("says what it does and how it ended after its turn, with no turn adopted", async () => {
    const { made, query } = await launched();
    query.send(
      assistant(message("s1", [textBlock("Starting now.")]), { parent: CALL }),
      taskProgress(TASK, CALL),
      taskNotification(TASK, CALL, "Done."),
    );
    await settle();
    expect(made.sink.of("turn.adopted")).toEqual([]);
    expect(timeline(made.sink.events).map((event) => event.type)).toEqual(["item.updated"]);
    expect(made.sink.of("subagent.ended").at(0)).toMatchObject({
      outcome: "completed",
      summary: "Done.",
    });
  });

  test("isn't ended by a launch that says so in words alone, among other results", async () => {
    const { made, query } = await called();
    query.send(
      taskStarted(TASK, CALL, { background: true }),
      toolResults([{ id: CALL, content: LAUNCHED }, { id: "bash", content: "ok" }], ACK),
    );
    await settle();
    expect(made.sink.of("subagent.ended")).toEqual([]);
  });

  test("moved there by a task's update outlives its turn", async () => {
    const { made, query, promptId } = await called();
    query.send(taskStarted(TASK, CALL), taskUpdated(TASK, true), success([promptId]));
    await settle();
    expect(made.sink.of("subagent.ended")).toEqual([]);
  });

  test("stopped says so", async () => {
    const { made, query } = await launched();
    query.send(taskNotification(TASK, CALL, "Stopped.", "stopped"));
    await settle();
    expect(made.sink.of("subagent.ended").at(0)).toMatchObject({ outcome: "interrupted" });
  });

  test("ended, then started again, runs anew in the same name", async () => {
    const { made, query } = await launched();
    query.send(
      taskNotification(TASK, CALL, "Done."),
      taskStarted(TASK, CALL, { background: true, prompt: "One more." }),
    );
    await settle();
    expect(made.sink.of("subagent.started").at(-1)).toMatchObject({
      key: CALL,
      subagent: { prompt: "One more." },
    });
  });

  test("waiting on a task of its own goes on, whatever its notification says", async () => {
    const { made, query } = await launched();
    query.send(
      shellStarted("shell", "toolu_bash", TASK),
      taskNotification(TASK, CALL, "This agent has not reported yet."),
    );
    await settle();
    expect(made.sink.of("subagent.ended")).toEqual([]);
    expect(made.sink.of("subagent.changed").at(-1)).toMatchObject({
      change: { progress: "Waiting on what it started" },
    });
    query.send(
      taskNotification("shell", "toolu_bash", "Slept."),
      taskStarted(TASK, CALL, { background: true }),
      taskNotification(TASK, CALL, "There's one file."),
    );
    await settle();
    expect(made.sink.of("subagent.started")).toHaveLength(1);
    expect(made.sink.of("subagent.ended").at(0)).toMatchObject({
      outcome: "completed",
      summary: "There's one file.",
    });
  });

  test("that failed has failed, whatever it was waiting on", async () => {
    const { made, query } = await launched();
    query.send(
      shellStarted("shell", "toolu_bash", TASK),
      taskNotification(TASK, CALL, "Out of tokens.", "failed"),
    );
    await settle();
    expect(made.sink.of("subagent.ended").at(0)).toMatchObject({ outcome: "failed" });
  });

  test("hands back its report as its answer, whatever else still runs", async () => {
    const { made, query } = await launched();
    query.send(
      shellStarted("monitor", "toolu_monitor", TASK),
      assistant(message("s1", [toolUse("toolu_back", "SubagentHandback", {
        message: "There's one file.",
      })]), { parent: CALL }),
      toolResults([{ id: "toolu_back", content: "Report delivered." }], { success: true }, CALL),
      taskNotification(TASK, CALL, "This agent's report was delivered to you as a message."),
    );
    await settle();
    expect(timeline(made.sink.events)).toEqual([
      {
        type: "item.updated",
        key: "toolu_back",
        item: { kind: "assistant_text", payload: { text: "There's one file." } },
        status: "completed",
      },
    ]);
    expect(made.sink.of("subagent.ended").at(0)).toMatchObject({
      outcome: "completed",
      summary: "There's one file.",
    });
  });

  test("isn't shown the CLI's notice of its task's end as words said to it", async () => {
    const { made, query } = await launched();
    query.send(saidToSubagent(CALL, "<task-notification>\n<task-id>shell</task-id>"));
    await settle();
    expect(timeline(made.sink.events)).toEqual([]);
  });

  test("the CLI started by itself shows too", async () => {
    const made = bench();
    const { promptId, query } = await made.turn();
    query.send(success([promptId]), taskStarted(TASK, CALL, { background: true }));
    await settle();
    expect(made.sink.of("subagent.started").at(0)).toMatchObject({
      key: CALL,
      subagent: { prompt: PROMPT, description: "Count files" },
    });
  });
});

describe("stopping a subagent", () => {
  test("asks the CLI to stop its task, and its notification ends it", async () => {
    const { made, query } = await called();
    query.send(taskStarted(TASK, CALL));
    await settle();
    made.session.stopSubagent(CALL);
    await settle();
    expect(query.stopped).toEqual([TASK]);
    expect(made.sink.of("subagent.ended")).toEqual([]);
    query.send(taskNotification(TASK, CALL, "Stopped.", "stopped"));
    await settle();
    expect(made.sink.of("subagent.ended")).toHaveLength(1);
  });

  test("ends it anyway when the CLI doesn't say it stopped in time", async () => {
    const { made, query } = await called();
    query.send(taskStarted(TASK, CALL));
    await settle();
    made.session.stopSubagent(CALL);
    await until(() => made.sink.of("subagent.ended").length > 0, 1000);
    expect(made.sink.of("subagent.ended").at(0)).toMatchObject({ outcome: "interrupted" });
  });

  test("that waits on a task of its own ends it once the CLI took it", async () => {
    const started = await called();
    const { made, query, promptId } = started;
    query.send(
      taskStarted(TASK, CALL, { background: true }),
      toolResults([{ id: CALL, content: LAUNCHED }], ACK),
      success([promptId]),
      shellStarted("shell", "toolu_bash", TASK),
      taskNotification(TASK, CALL, "This agent has not reported yet."),
    );
    await settle();
    made.session.stopSubagent(CALL);
    await settle();
    expect(made.sink.of("subagent.ended").at(0)).toMatchObject({ outcome: "interrupted" });
  });

  test("that has no task yet ends it", async () => {
    const { made } = await called();
    made.session.stopSubagent(CALL);
    await settle();
    expect(made.sink.of("subagent.ended").at(0)).toMatchObject({ outcome: "interrupted" });
  });
});

describe("a subagent's request", () => {
  test("is asked with no turn running, and starts its call in its own timeline", async () => {
    const { made, query, promptId } = await called();
    query.send(
      taskStarted(TASK, CALL, { background: true }),
      toolResults([{ id: CALL, content: LAUNCHED }], ACK),
      success([promptId]),
    );
    await settle();
    const answer = query.options.canUseTool?.("Bash", { command: "ls" }, toolRequest({
      toolUseID: "toolu_bash",
      agentID: TASK,
    }));
    await settle();
    expect(made.sink.asked.map((asked) => asked.bySubagent)).toEqual([true]);
    made.sink.asked[0]?.answer({ kind: "approval", decision: "allow", message: null });
    expect(await answer).toEqual({ behavior: "allow", updatedInput: { command: "ls" } });
    expect(timeline(made.sink.events).at(0)).toMatchObject({
      type: "item.started",
      key: "toolu_bash",
    });
    expect(made.sink.of("item.started")).toEqual([]);
  });
});
