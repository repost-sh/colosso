import { describe, expect, test } from "bun:test";

import { bounded, family, toolStarted } from "../../src/providers/claude/tools";
import {
  assistant,
  message,
  permissionDenied,
  taskNotification,
  toolResults,
  toolUse,
} from "../fixtures/claude/frames";
import { NOTES, toolsTurn } from "../fixtures/claude/sessions";
import { mapped, ofType } from "./mapped";

describe("families", () => {
  test("each tool's family is its name's, legacy names as they're called now", () => {
    const names = [
      "Bash", "Read", "Edit", "Write", "MultiEdit", "NotebookEdit", "Grep", "Glob", "LS",
      "WebSearch", "WebFetch", "mcp__github__create_issue", "Agent", "Task", "TodoWrite",
      "Skill", "ToolSearch",
    ];
    expect(names.map((name) => family(name))).toEqual([
      "command", "read", "edit", "edit", "edit", "edit", "search", "search", "search",
      "web", "web", "mcp", "agent", "agent", "todo", "skill", "other",
    ]);
    expect(toolStarted("Task", { description: "Find it" }).payload).toMatchObject({
      name: "Agent",
      family: "agent",
    });
  });

  test("an MCP tool is shown by its server and its own name", () => {
    expect(toolStarted("mcp__github__create_issue", { title: "Flaky" }).payload).toMatchObject({
      family: "mcp",
      label: "github: create_issue",
      server: "github",
    });
  });

  test("a long input is kept cut", () => {
    const input = { content: "x".repeat(10_000) };
    const kept = bounded(input);
    expect(JSON.stringify(kept).length).toBeLessThan(4200);
    expect(toolStarted("Write", input).payload.input).toEqual(kept);
  });
});

describe("a recorded turn's tools", () => {
  test("start on their block and settle on their result, with output and patch", () => {
    const { events, send } = mapped();
    send(...toolsTurn("p1"));
    const started = ofType(events, "item.started");
    expect(started.filter((event) => event.item.kind === "tool").map((event) => [
      event.key,
      event.item.kind === "tool" ? event.item.payload.label : "",
    ])).toEqual([
      ["toolu_write", NOTES],
      ["toolu_edit", NOTES],
      ["toolu_bash", "Print contents of notes.txt"],
    ]);
    const tools = ofType(events, "item.updated").filter((event) => event.item.kind === "tool");
    expect(tools.map((event) => [event.key, event.status, event.item.payload])).toEqual([
      ["toolu_write", "completed", {
        name: "Write",
        family: "edit",
        label: NOTES,
        input: { file_path: NOTES, content: "one\n" },
        output: `File created successfully at: ${NOTES}`,
        exitCode: null,
        patch: [],
        files: [NOTES],
        server: null,
      }],
      ["toolu_edit", "completed", {
        name: "Edit",
        family: "edit",
        label: NOTES,
        input: { replace_all: false, file_path: NOTES, old_string: "one", new_string: "two" },
        output: `The file ${NOTES} has been updated successfully.`,
        exitCode: null,
        patch: [{ oldStart: 1, oldLines: 1, newStart: 1, newLines: 1, lines: ["-one", "+two"] }],
        files: [NOTES],
        server: null,
      }],
      ["toolu_bash", "completed", {
        name: "Bash",
        family: "command",
        label: "Print contents of notes.txt",
        input: { command: "cat notes.txt", description: "Print contents of notes.txt" },
        output: "two",
        exitCode: 0,
        patch: null,
        files: [],
        server: null,
      }],
    ]);
  });
});

describe("a tool call", () => {
  test("that failed has the exit code its result names", () => {
    const { events, send } = mapped();
    send(
      assistant(message("m1", [toolUse("t1", "Bash", { command: "bun test" })])),
      toolResults([{ id: "t1", content: "Exit code 2\n1 test failed", isError: true }], {
        stdout: "1 test failed",
        stderr: "",
      }),
    );
    expect(ofType(events, "item.updated")[0]).toMatchObject({
      status: "failed",
      item: { payload: { exitCode: 2, output: "1 test failed" } },
    });
  });

  test("takes the CLI's structured result only when its message has one result", () => {
    const { events, send } = mapped();
    send(
      assistant(message("m1", [
        toolUse("t1", "Bash", { command: "echo a" }),
        toolUse("t2", "Bash", { command: "echo b" }),
      ])),
      toolResults([{ id: "t1", content: "a" }, { id: "t2", content: "b" }], {
        stdout: "a",
        stderr: "",
      }),
    );
    const outputs = ofType(events, "item.updated").map((event) =>
      event.item.kind === "tool" ? event.item.payload.output : null);
    expect(outputs).toEqual(["a", "b"]);
  });

  test("open at the turn's end is the actor's to close, and forgotten here", () => {
    const { frames, events, send } = mapped();
    send(assistant(message("m1", [toolUse("t1", "Bash", { command: "sleep 9" })])));
    frames.end();
    expect(ofType(events, "item.updated")).toEqual([]);
    send(assistant(message("m1", [toolUse("t1", "Bash", { command: "sleep 9" })])));
    expect(ofType(events, "item.started")).toHaveLength(2);
  });

  test("seen first in its permission request starts once", () => {
    const { frames, events, send } = mapped();
    const input = { command: "ls" };
    frames.toolStarted("t1", "Bash", input, bounded(input));
    send(assistant(message("m1", [toolUse("t1", "Bash", input)])));
    expect(events.filter((event) => event.type === "item.started")).toHaveLength(1);
  });

  test("that Claude's checks denied reads as declined", () => {
    const { events, send } = mapped();
    send(
      assistant(message("m1", [toolUse("t1", "Bash", { command: "rm -rf /" })])),
      permissionDenied("t1"),
      toolResults([{ id: "t1", content: "Permission to use Bash was denied.", isError: true }]),
    );
    expect(ofType(events, "item.updated").map((event) => event.status)).toEqual(["declined"]);
  });

  test("to Agent is a subagent, its calls under it, settled with its report", () => {
    const { events, send } = mapped();
    send(
      assistant(message("m1", [toolUse("ta", "Agent", {
        description: "Find the flaky test",
        subagent_type: "Explore",
        prompt: "Look in test/.",
      })])),
      assistant(message("s1", [toolUse("tg", "Grep", { pattern: "flaky" })]), { parent: "ta" }),
      toolResults([{ id: "tg", content: "test/a.test.ts" }], undefined, "ta"),
      taskNotification("ta", "It's test/a.test.ts."),
    );
    const started = ofType(events, "item.started");
    expect(started.map((event) => [event.key, event.parentKey, event.item.kind])).toEqual([
      ["ta", null, "subagent"],
      ["tg", "ta", "tool"],
    ]);
    expect(ofType(events, "item.updated").at(-1)).toEqual({
      type: "item.updated",
      key: "ta",
      item: {
        kind: "subagent",
        payload: {
          description: "Find the flaky test",
          agentType: "Explore",
          summary: "It's test/a.test.ts.",
        },
      },
      status: "completed",
    });
  });

  test("to TodoWrite shows the turn's to-do list, one item each call updates", () => {
    const { events, send } = mapped();
    const todos = (done: boolean): unknown => ({
      todos: [
        {
          content: "Find it",
          status: done ? "completed" : "in_progress",
          activeForm: "Finding it",
        },
        { content: "Fix it", status: "pending", activeForm: "Fixing it" },
      ],
    });
    send(
      assistant(message("m1", [toolUse("w1", "TodoWrite", todos(false))])),
      assistant(message("m2", [toolUse("w2", "TodoWrite", todos(true))])),
    );
    const lists = events.filter((event) => "key" in event && event.key === "turn-1:todos");
    expect(lists.map((event) => event.type))
      .toEqual(["item.started", "item.updated", "item.updated"]);
    expect(lists.at(-1)).toMatchObject({
      item: {
        kind: "todos",
        payload: {
          todos: [{ text: "Find it", status: "completed" }, { text: "Fix it", status: "pending" }],
        },
      },
    });
  });
});
