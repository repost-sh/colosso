import type {
  HookCallback,
  HookEvent,
  HookJSONOutput,
  Options,
  PostToolUseHookInput,
  PreToolUseHookInput,
  UserPromptSubmitHookInput,
} from "@anthropic-ai/claude-agent-sdk";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { afterEach, describe, expect, test } from "bun:test";

import {
  PROGRESS_DESCRIPTION,
  PROGRESS_PROMPT,
  PROGRESS_SERVER,
  PROGRESS_TOOL,
} from "../../src/progress/prompt";
import { promptReminder, STALE_CALLS } from "../../src/progress/reminders";
import { report } from "../../src/progress/report";
import { KOYO_PROMPT, KOYO_TOOLS_PROMPT } from "../../src/providers/claude/options";
import { MOD_TOOL } from "../../src/providers/claude/progress";
import { init, success } from "../fixtures/claude/frames";
import { settle } from "../support/wait";
import { bench, closeBenches, type Sink, spec } from "./support";

afterEach(closeBenches);

const KOYO = "/Applications/Koyo.app/Contents/MacOS/koyo";
const PLAN = {
  steps: [
    {
      id: 1,
      title: "Reproduce the flaky ledger test",
      detail: "Run the suite until it fails.",
      status: "running",
    },
    { id: "2", title: "Fix the rounding race in settle()", detail: "Settle under one lock." },
  ],
};
const HOOKED = { session_id: "s1", transcript_path: "/work/s1.jsonl", cwd: "/work/rec-cwd" };
const SIGNAL = { signal: new AbortController().signal };

async function opened(progress = true): Promise<{ options: Options; sink: Sink }> {
  const made = bench(spec({ progress }));
  const { options } = await made.query();
  return { options, sink: made.sink };
}

function hook(options: Options, event: HookEvent): HookCallback {
  const callback = options.hooks?.[event]?.[0]?.hooks[0];
  if (callback === undefined) {
    throw new Error(`The session has no ${event} hook.`);
  }
  return callback;
}

/** A client of the session's progress server, as the CLI is. */
async function progressClient(options: Options): Promise<Client> {
  const server = options.mcpServers?.[PROGRESS_SERVER];
  if (server?.type !== "sdk") {
    throw new Error("The session has no progress server in the host.");
  }
  const [ours, theirs] = InMemoryTransport.createLinkedPair();
  await server.instance.connect(theirs);
  const client = new Client({ name: "koyo-test", version: "1.0.0" });
  await client.connect(ours);
  return client;
}

function preToolUse(agentId?: string): PreToolUseHookInput {
  return {
    ...HOOKED,
    hook_event_name: "PreToolUse",
    tool_name: PROGRESS_TOOL,
    tool_input: PLAN,
    tool_use_id: "toolu_1",
    ...(agentId === undefined ? {} : { agent_id: agentId }),
  };
}

function postToolUse(toolName: string, agentId?: string): PostToolUseHookInput {
  return {
    ...HOOKED,
    hook_event_name: "PostToolUse",
    tool_name: toolName,
    tool_input: {},
    tool_response: "",
    tool_use_id: "toolu_2",
    ...(agentId === undefined ? {} : { agent_id: agentId }),
  };
}

function promptSubmit(source: UserPromptSubmitHookInput["source"]): UserPromptSubmitHookInput {
  return {
    ...HOOKED,
    hook_event_name: "UserPromptSubmit",
    prompt: "Go on.",
    ...(source === undefined ? {} : { source }),
  };
}

/** What a hook's output adds to the conversation, if anything. */
function context(output: HookJSONOutput): string | null {
  const specific = "hookSpecificOutput" in output ? output.hookSpecificOutput : undefined;
  return specific !== undefined && "additionalContext" in specific
    ? specific.additionalContext ?? null
    : null;
}

describe("the progress tool in a session", () => {
  test("is Koyo's own server in the host, its tool allowed, the person's mod's taken away",
    async () => {
      const { options } = await opened();
      expect(options.mcpServers?.[PROGRESS_SERVER]).toMatchObject({
        type: "sdk",
        name: PROGRESS_SERVER,
      });
      expect(Object.keys(options.mcpServers ?? {})).toEqual([PROGRESS_SERVER]);
      expect(options.allowedTools).toEqual([PROGRESS_TOOL]);
      expect(options.disallowedTools).toEqual([MOD_TOOL]);
      expect(options.systemPrompt).toEqual({
        type: "preset",
        preset: "claude_code",
        append: `${KOYO_PROMPT}\n\n${PROGRESS_PROMPT}`,
      });
      expect(options.hooks?.PreToolUse).toMatchObject([{ matcher: PROGRESS_TOOL }]);
      expect(Object.keys(options.hooks ?? {}))
        .toEqual(["PreToolUse", "PostToolUse", "UserPromptSubmit"]);
    });

  test("comes after Koyo's tools and their prompt, each list keeping both", async () => {
    const { options } = await bench(spec({ progress: true, koyoTools: KOYO })).query();
    expect(Object.keys(options.mcpServers ?? {})).toEqual(["koyo", PROGRESS_SERVER]);
    expect(options.allowedTools).toContain("mcp__koyo__terminal_read");
    expect(options.allowedTools?.at(-1)).toBe(PROGRESS_TOOL);
    expect(options.disallowedTools).toEqual([
      "EnterWorktree",
      "ExitWorktree",
      "mcp__koyo__worktree_delete",
      "mcp__koyo__app_launch",
      MOD_TOOL,
    ]);
    expect(options.systemPrompt).toEqual({
      type: "preset",
      preset: "claude_code",
      append: `${KOYO_PROMPT}\n\n${KOYO_TOOLS_PROMPT}\n\n${PROGRESS_PROMPT}`,
    });
  });

  test("isn't there while it's off: no server, tools, hooks or prompt of it", async () => {
    const { options } = await opened(false);
    expect(options.mcpServers).toBeUndefined();
    expect(options.allowedTools).toBeUndefined();
    expect(options.disallowedTools).toBeUndefined();
    expect(options.hooks).toBeUndefined();
    expect(options.systemPrompt)
      .toEqual({ type: "preset", preset: "claude_code", append: KOYO_PROMPT });
  });

  test("is loaded with the prompt, not behind tool search, and described for the agent",
    async () => {
      const client = await progressClient((await opened()).options);
      const { tools } = await client.listTools();
      expect(tools).toMatchObject([{
        name: "update",
        description: PROGRESS_DESCRIPTION,
        _meta: { "anthropic/alwaysLoad": true },
      }]);
      await client.close();
    });

  test("takes a call to the chat's plan and answers with it; a refusal is the call's error",
    async () => {
      const { options, sink } = await opened();
      const client = await progressClient(options);
      const kept = await client.callTool({ name: "update", arguments: PLAN });
      const current = sink.plan.current();
      expect(sink.planned).toHaveLength(1);
      expect(current?.phases.map((phase) => phase.id)).toEqual(["1", "2"]);
      expect(kept).toEqual({
        content: [{ type: "text", text: current === null ? "" : report(current) }],
      });
      const refused = await client.callTool({
        name: "update",
        arguments: { steps: [{ id: "3", status: "running" }] },
      });
      expect(refused).toMatchObject({
        isError: true,
        content: [{ type: "text", text: "Step 3 is new and needs a title." }],
      });
      await client.close();
    });

  test("is new with each process the session starts", async () => {
    const made = bench(spec({ progress: true }), { resumable: true });
    const first = await made.turn();
    first.query.send(init("default", { echo: [first.promptId] }), success([first.promptId]));
    await settle();
    first.query.refuses = true;
    made.session.configure({ model: "claude-opus-5-5" });
    await settle();
    const second = await made.turn("Go on.");
    const server = (query: typeof first.query): unknown =>
      query.options.mcpServers?.[PROGRESS_SERVER];
    expect(server(second.query)).toBeDefined();
    expect(server(second.query)).not.toBe(server(first.query));
  });
});

describe("a subagent's call of the progress tool", () => {
  test("is refused, telling it to report instead; the main agent's goes through", async () => {
    const { options } = await opened();
    const guard = hook(options, "PreToolUse");
    expect(await guard(preToolUse("agent-7"), "toolu_1", SIGNAL)).toEqual({
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason:
          "Only the chat's main agent keeps the plan; say what changed in your report to it.",
      },
    });
    expect(await guard(preToolUse(), "toolu_1", SIGNAL)).toEqual({});
  });
});

describe("the plan's reminders", () => {
  test("come with a message the person sends while the plan has work open", async () => {
    const { options, sink } = await opened();
    const submitted = hook(options, "UserPromptSubmit");
    expect(await submitted(promptSubmit("sdk"), undefined, SIGNAL)).toEqual({});
    await sink.plan.update(PLAN);
    const reminder = promptReminder(sink.plan.current());
    expect(reminder).not.toBeNull();
    expect(context(await submitted(promptSubmit("sdk"), undefined, SIGNAL))).toBe(reminder);
    expect(context(await submitted(promptSubmit(undefined), undefined, SIGNAL))).toBe(reminder);
    expect(await submitted(promptSubmit("system"), undefined, SIGNAL)).toEqual({});
  });

  test("ask for an update once thirty of the main agent's calls went by without one",
    async () => {
      const { options, sink } = await opened();
      await sink.plan.update(PLAN);
      const used = hook(options, "PostToolUse");
      const call = async (name = "Bash", agentId?: string): Promise<string | null> =>
        context(await used(postToolUse(name, agentId), "toolu_2", SIGNAL));
      for (let count = 1; count < STALE_CALLS; count += 1) {
        expect(await call()).toBeNull();
      }
      expect(await call("Read", "agent-7")).toBeNull();
      expect(await call(PROGRESS_TOOL)).toBeNull();
      expect(await call()).toStartWith("30 tool calls went by since the chat's plan last changed.");
      expect(await call()).toBeNull();
    });

  test("count again from a plan update the tool made", async () => {
    const { options, sink } = await opened();
    const client = await progressClient(options);
    await client.callTool({ name: "update", arguments: PLAN });
    const used = hook(options, "PostToolUse");
    const call = async (): Promise<string | null> =>
      context(await used(postToolUse("Bash"), "toolu_2", SIGNAL));
    for (let count = 1; count < STALE_CALLS; count += 1) {
      await call();
    }
    const running = { steps: [{ id: "2", status: "running" }] };
    await client.callTool({ name: "update", arguments: running });
    expect(await call()).toBeNull();
    for (let count = 2; count < STALE_CALLS; count += 1) {
      await call();
    }
    expect(await call()).toContain("Running: 1 \"Reproduce the flaky ledger test\", 2 ");
    expect(sink.planned).toHaveLength(2);
    await client.close();
  });
});
