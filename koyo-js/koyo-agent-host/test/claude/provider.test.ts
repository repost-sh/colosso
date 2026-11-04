import type { ModelInfo } from "@anthropic-ai/claude-agent-sdk";
import { describe, expect, test } from "bun:test";

import { models } from "../../src/providers/claude/catalog";
import { type ClaudeDeps, ClaudeProvider } from "../../src/providers/claude/provider";
import { failed, success } from "../fixtures/claude/frames";
import { settle } from "../support/wait";
import { ENV, FakeQuery, INITIALIZED, Sink, spec } from "./support";

/** Where `claude` is on a machine without it. */
const INSTALLED: string[] = [];

/** A provider on fake queries, with where it looked for `claude`. */
function provider(deps: Partial<ClaudeDeps> = {}): {
  claude: ClaudeProvider;
  queries: FakeQuery[];
  looked: (string | null)[];
} {
  const queries: FakeQuery[] = [];
  const looked: (string | null)[] = [];
  const claude = new ClaudeProvider({
    environment: async () => await Promise.resolve(ENV),
    start: (params) => {
      const query = new FakeQuery(params);
      queries.push(query);
      return query;
    },
    find: (setting) => {
      looked.push(setting);
      return setting ?? "/opt/homebrew/bin/claude";
    },
    version: async () => await Promise.resolve("2.1.317"),
    settingsMode: async () => await Promise.resolve("acceptEdits"),
    resumable: async () => await Promise.resolve(false),
    stopMs: 50,
    ...deps,
  });
  return { claude, queries, looked };
}

describe("Claude's status", () => {
  test("is ready, with the account, once the CLI answers its probe", async () => {
    const { claude, queries } = provider();
    expect(await claude.status(false)).toEqual({
      provider: "claude",
      state: "ready",
      executable: "/opt/homebrew/bin/claude",
      version: "2.1.317",
      account: { email: "dev@example.com", plan: "max" },
      message: null,
    });
    // The probe starts the CLI and nothing more: no transcript, hooks or MCP servers.
    expect(queries[0]?.options).toMatchObject({
      persistSession: false,
      settings: { disableAllHooks: true },
      mcpServers: {},
      strictMcpConfig: true,
    });
    expect(queries[0]?.closed).toBe(true);
    await claude.status(false);
    expect(queries).toHaveLength(1);
  });

  test("is missing when no claude is found", async () => {
    const { claude } = provider({ find: () => INSTALLED[0] });
    expect(await claude.status(false))
      .toMatchObject({ state: "missing", message: "Install Claude Code." });
  });

  test("looks for claude where Koyo's setting says first", async () => {
    const { claude, looked } = provider();
    claude.setConfig({ claudePath: "/Users/dev/bin/claude", defaultMode: null });
    expect(await claude.status(false)).toMatchObject({ executable: "/Users/dev/bin/claude" });
    expect(looked).toEqual(["/Users/dev/bin/claude"]);
  });

  test("is signed out when the CLI has no account, key or gateway", async () => {
    const { claude, queries } = provider({
      start: (params) => {
        const query = new FakeQuery(params);
        const account = { tokenSource: "none", apiKeySource: "none" };
        query.initialized = { ...INITIALIZED, account };
        queries.push(query);
        return query;
      },
    });
    expect(await claude.status(false)).toMatchObject({
      state: "signed_out",
      account: null,
      message: "Sign in to Claude.",
    });
  });
});

describe("the catalog", () => {
  test("has the CLI's models, the folder's commands, and its settings' default mode", async () => {
    const { claude } = provider();
    expect(await claude.catalog("/work/a")).toEqual({
      models: [{
        id: "default",
        name: "Default",
        label: "Default",
        runs: null,
        efforts: ["low", "medium", "high"],
        fast: true,
        auto: true,
      }],
      commands: [{ name: "compact", description: "Compact the conversation", argumentHint: null }],
      defaultMode: "accept_edits",
    });
  });

  test("has the Default row go by the name of what it runs", () => {
    const opus = { value: "opus", displayName: "Opus 5.5", description: "", resolvedModel: "o-5" };
    const row = { value: "default", displayName: "Default (recommended)", description: "" };
    const labels = (list: ModelInfo[]): string[] => models(list).map((model) => model.label);

    expect(labels([{ ...row, resolvedModel: "o-5" }, opus])).toEqual(["Opus 5.5", "Opus 5.5"]);
    expect(labels([{ ...row, resolvedModel: "other" }, opus]))
      .toEqual(["other", "Opus 5.5"]);
    expect(labels([row, opus])).toEqual(["Default (recommended)", "Opus 5.5"]);
  });

  test("reports Claude's own default mode, whatever Koyo's settings name", async () => {
    const { claude } = provider();
    claude.setConfig({ claudePath: null, defaultMode: "auto" });
    expect((await claude.catalog("/work/a")).defaultMode).toBe("accept_edits");
  });

  test("shares a probe already running for the folder", async () => {
    const { claude, queries } = provider();
    const both = [claude.catalog("/work/a"), claude.catalog("/work/a")];
    await Promise.all(both);
    expect(queries).toHaveLength(1);
  });

  test("probes a folder again once the probe before it settled", async () => {
    const { claude, queries } = provider();
    await claude.status(true);
    await claude.status(true);
    expect(queries).toHaveLength(2);
  });
});

describe("a session", () => {
  test("with no claude to run ends saying so", async () => {
    const { claude } = provider({ find: () => INSTALLED[0] });
    const sink = new Sink();
    claude.open(spec(), sink);
    await settle();
    expect(sink.of("session.ended")).toEqual([{
      type: "session.ended",
      error: "Claude Code isn't installed. Install it, then send the message again.",
    }]);
  });

  test("resumed asks for a conversation's turns only until it's found to have them", async () => {
    const asked: string[] = [];
    const answers = [false, true, false];
    const { claude } = provider({
      resumable: async (sessionId) => {
        asked.push(sessionId);
        return await Promise.resolve(answers.shift() === true);
      },
    });
    const resumed = spec({ resume: true });
    const opened = [];
    for (let time = 0; time < 3; time += 1) {
      const sink = new Sink();
      opened.push({ session: claude.open(resumed, sink), sink });
      await settle();
    }
    expect(asked).toEqual([resumed.sessionId, resumed.sessionId]);
    expect(opened.map(({ sink }) => sink.of("notice").length)).toEqual([1, 0, 0]);
    await Promise.all(opened.map(async ({ session }) => {
      await session.close();
    }));
  });
});

describe("a title", () => {
  test("comes from the smallest model, in a turn with no tools, settings or record", async () => {
    const { claude, queries } = provider();
    const prompt = "The test in a.test.ts fails one run in ten; find out why.";
    const titled = claude.title(prompt, "/work/a");
    await settle();
    const query = queries[0];
    const reply = "\"Flaky Test Investigation.\"\n";
    query?.send(success([], { text: reply }));
    // As the model replied: the actor makes the title of it.
    expect(await titled).toBe(reply);
    expect(query?.options).toMatchObject({
      model: "haiku",
      tools: [],
      settingSources: [],
      persistSession: false,
      maxTurns: 1,
    });
    expect(query?.closed).toBe(true);
  });

  test("isn't one when the model's turn failed", async () => {
    const { claude, queries } = provider();
    const titled = claude.title("Hi.", "/work/a");
    await settle();
    queries[0]?.send(failed([], ["API Error: 529"]));
    expect(await titled).toBeUndefined();
  });
});
