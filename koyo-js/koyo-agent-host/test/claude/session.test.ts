import type { PermissionMode } from "@anthropic-ai/claude-agent-sdk";
import { afterEach, describe, expect, test } from "bun:test";

import type { Mode } from "../../src/protocol/model";
import { KOYO_PROMPT } from "../../src/providers/claude/options";
import { init, success } from "../fixtures/claude/frames";
import { settle } from "../support/wait";
import { bench, closeBenches, spec } from "./support";

afterEach(closeBenches);

const FRESH = "The earlier conversation wasn't available, so Claude starts this one fresh.";

describe("a session's options", () => {
  test("run Claude Code's prompt on the person's CLI, streaming, in the chat's mode", async () => {
    const opened = spec({ model: "claude-opus-5-5", effort: "high" });
    const query = await bench(opened).query();
    expect(query.options).toMatchObject({
      pathToClaudeCodeExecutable: "/opt/homebrew/bin/claude",
      cwd: "/work/rec-cwd",
      env: { PATH: "/usr/bin:/bin", HOME: "/Users/dev" },
      sessionId: opened.sessionId,
      systemPrompt: { type: "preset", preset: "claude_code", append: KOYO_PROMPT },
      includePartialMessages: true,
      extraArgs: { "thinking-display": "summarized" },
      model: "claude-opus-5-5",
      effort: "high",
      permissionMode: "default",
      toolConfig: { askUserQuestion: { previewFormat: "markdown" } },
    });
    expect(typeof query.options.canUseTool).toBe("function");
    expect(typeof query.options.spawnClaudeCodeProcess).toBe("function");
    expect(query.options.resume).toBeUndefined();
    expect(query.options.allowDangerouslySkipPermissions).toBeUndefined();
    expect(query.options.settings).toBeUndefined();
  });

  test.each<[Mode, PermissionMode]>([
    ["ask", "default"],
    ["accept_edits", "acceptEdits"],
    ["auto", "auto"],
  ])("ask for %s as %s, approving through the person", async (mode, permissionMode) => {
    const query = await bench(spec({ permission: mode })).query();
    expect(query.options.permissionMode).toBe(permissionMode);
    expect(typeof query.options.canUseTool).toBe("function");
  });

  test("in Full access bypass permissions, which the session allows, asking no one", async () => {
    const query = await bench(spec({ permission: "full_access" })).query();
    expect(query.options.permissionMode).toBe("bypassPermissions");
    expect(query.options.allowDangerouslySkipPermissions).toBe(true);
    expect(query.options.canUseTool).toBeUndefined();
  });

  test("in plan mode ask for plan, approving through the person in any mode", async () => {
    const query = await bench(spec({ permission: "full_access", plan: true })).query();
    expect(query.options.permissionMode).toBe("plan");
    expect(typeof query.options.canUseTool).toBe("function");
  });

  test("with no mode leave it to Claude's settings", async () => {
    const query = await bench(spec({ permission: null })).query();
    expect("permissionMode" in query.options).toBe(false);
  });

  test("in fast mode turn it on", async () => {
    const query = await bench(spec({ fast: true })).query();
    expect(query.options.settings).toEqual({ fastMode: true });
  });

  test("resume a conversation with turns on disk", async () => {
    const opened = spec({ resume: true });
    const made = bench(opened, { resumable: true });
    const query = await made.query();
    expect(query.options.resume).toBe(opened.sessionId);
    expect(query.options.sessionId).toBeUndefined();
    expect(made.sink.of("notice")).toEqual([]);
  });

  test("start fresh, with the same id, when the conversation has no turns to resume", async () => {
    const opened = spec({ resume: true });
    const made = bench(opened, { resumable: false });
    const query = await made.query();
    expect(query.options.resume).toBeUndefined();
    expect(query.options.sessionId).toBe(opened.sessionId);
    expect(made.sink.of("notice")).toEqual([{ type: "notice", level: "warning", text: FRESH }]);
  });
});

describe("a session gone back in its conversation", () => {
  test("resumes from the entry it went back to", async () => {
    const opened = spec({ resume: true, resumeAt: "entry" });
    const query = await bench(opened, { resumable: true }).query();
    expect(query.options.resumeSessionAt).toBe("entry");
  });

  test("has nothing to go back in, when it's starting fresh", async () => {
    const opened = spec({ resume: true, resumeAt: "entry" });
    const query = await bench(opened, { resumable: false }).query();
    expect(query.options.resumeSessionAt).toBeUndefined();
  });
});

describe("the session's init", () => {
  test("says the mode in force, none for plan mode, and the folder's slash commands", async () => {
    const made = bench();
    const { promptId, query } = await made.turn();
    query.send(init("default", { echo: [promptId] }));
    query.send(init("plan", { echo: [promptId] }));
    await settle();
    expect(made.sink.of("session.ready")).toEqual([
      { type: "session.ready", mode: "ask" },
      { type: "session.ready", mode: null },
    ]);
    expect(made.commands).toEqual([{ cwd: "/work/rec-cwd", names: ["review"] }]);
  });
});

describe("a settings change", () => {
  test("is applied to the live process before the next turn's prompt", async () => {
    const made = bench();
    const query = await made.query();
    expect(made.session.configure({ model: "claude-opus-5-5", effort: "low", fast: true }))
      .toBe("applied");
    expect(made.session.configure({ permission: "accept_edits" })).toBe("applied");
    await made.turn();
    expect(query.controls).toEqual([
      { method: "setModel", model: "claude-opus-5-5" },
      { method: "applyFlagSettings", flags: { effortLevel: "low" } },
      { method: "applyFlagSettings", flags: { fastMode: true } },
      { method: "setPermissionMode", mode: "acceptEdits" },
    ]);
    expect(query.journal.at(-1)).toBe("prompt");
  });

  test("the live process can't take says so at once", async () => {
    const made = bench();
    await made.query();
    expect(made.session.configure({ model: null })).toBe("reopen");
    expect(made.session.configure({ effort: null })).toBe("reopen");
    // Asking the person can't become bypassing them in a process opened to ask.
    expect(made.session.configure({ permission: "full_access" })).toBe("reopen");
    const bypassing = bench(spec({ permission: "full_access" }));
    await bypassing.query();
    expect(bypassing.session.configure({ permission: "ask" })).toBe("reopen");
    expect(bypassing.session.configure({ plan: true })).toBe("reopen");
  });

  test("to no mode returns to the one Claude's settings chose", async () => {
    const made = bench(spec({ permission: null }));
    const { promptId, query } = await made.turn();
    query.send(init("acceptEdits", { echo: [promptId] }), success([promptId]));
    await settle();
    expect(made.session.configure({ plan: true })).toBe("applied");
    expect(made.session.configure({ plan: false })).toBe("applied");
    await settle();
    expect(query.controls).toEqual([
      { method: "setPermissionMode", mode: "plan" },
      { method: "setPermissionMode", mode: "acceptEdits" },
    ]);
  });

  test("the process failed to take restarts it, resumed, before the next turn", async () => {
    const made = bench(spec(), { resumable: true });
    const first = await made.turn();
    first.query.send(init("default", { echo: [first.promptId] }), success([first.promptId]));
    await settle();
    first.query.refuses = true;
    expect(made.session.configure({ model: "claude-opus-5-5" })).toBe("applied");
    await settle();
    const second = await made.turn("Go on.");
    expect(made.queries).toHaveLength(2);
    expect(first.query.closed).toBe(true);
    expect(second.query).not.toBe(first.query);
    expect(second.query.options.model).toBe("claude-opus-5-5");
    expect(second.query.options.resume).toBe(first.query.options.sessionId);
    expect(made.sink.of("session.ended")).toEqual([]);
  });
});

describe("a session's end", () => {
  test("says why the CLI failed, with what it wrote on stderr before anything else", async () => {
    const made = bench();
    const query = await made.query();
    const spawned = query.options.spawnClaudeCodeProcess?.({
      command: "/bin/sh",
      args: ["-c", "echo 'Not logged in. Run claude to sign in.' >&2"],
      cwd: process.cwd(),
      env: {},
      signal: new AbortController().signal,
    });
    await new Promise<void>((resolve) => {
      spawned?.once("exit", () => {
        resolve();
      });
    });
    // What it wrote comes on its own pipe, a little after it exits.
    await settle(3);
    query.end(new Error("Claude Code process exited with code 1"));
    await settle();
    expect(made.sink.of("session.ended")).toEqual([{
      type: "session.ended",
      error: "Claude Code process exited with code 1\nNot logged in. Run claude to sign in.",
    }]);
  });

  test("on close is no error", async () => {
    const made = bench();
    const query = await made.query();
    await made.session.close();
    await settle();
    expect(query.closed).toBe(true);
    expect(made.sink.of("session.ended")).toEqual([{ type: "session.ended", error: null }]);
  });
});
