import type { ModelInfo, RewindFilesResult } from "@anthropic-ai/claude-agent-sdk";
import { describe, expect, test } from "bun:test";

import { HostError } from "../../src/protocol/wire";
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
    conversation: async () => await Promise.resolve([]),
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
    // The probe starts the CLI and nothing more: no transcript, hooks or MCP servers, Koyo's
    // tools included.
    expect(queries[0]?.options).toMatchObject({
      persistSession: false,
      settings: { disableAllHooks: true },
      mcpServers: {},
      strictMcpConfig: true,
    });
    expect(queries[0]?.options.allowedTools).toBeUndefined();
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
    claude.setConfig({ claudePath: "/Users/dev/bin/claude", defaultMode: null, koyoTools: null });
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
    claude.setConfig({ claudePath: null, defaultMode: "auto", koyoTools: null });
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

/** A provider whose queries answer a rewind of the files with `rewound`. */
function answering(rewound: RewindFilesResult): { claude: ClaudeProvider; queries: FakeQuery[] } {
  const queries: FakeQuery[] = [];
  const { claude } = provider({
    start: (params) => {
      const query = new FakeQuery(params);
      query.rewound = rewound;
      queries.push(query);
      return query;
    },
  });
  return { claude, queries };
}

function only(queries: readonly FakeQuery[]): FakeQuery {
  const [query] = queries;
  if (query === undefined) {
    throw new Error("No query started.");
  }
  return query;
}

const NO_RECORD = { canRewind: false, error: "No file checkpoint found for this message." };

describe("a conversation's files", () => {
  test("are asked after with nothing said to the model, and the process goes", async () => {
    const { claude, queries } = answering({
      canRewind: true,
      filesChanged: ["/work/a"],
      insertions: 1,
      deletions: 2,
    });
    expect(await claude.filesAt("s", "/work", "p")).toEqual({
      files: ["/work/a"],
      insertions: 1,
      deletions: 2,
    });
    const query = only(queries);
    expect(query.rewinds).toEqual([{ promptId: "p", dryRun: true }]);
    expect(query.options).toMatchObject({ resume: "s", enableFileCheckpointing: true });
    expect(query.options.settings).toEqual({ disableAllHooks: true });
    expect(query.options).toMatchObject({ mcpServers: {}, strictMcpConfig: true });
    expect(query.prompts).toEqual([]);
    expect(query.closed).toBe(true);
  });

  test("have nothing to say when it kept no record of them", async () => {
    const { claude } = answering(NO_RECORD);
    expect(await claude.filesAt("s", "/work", "p")).toBeNull();
  });

  test("go back for real only when asked to, and say when they can't", async () => {
    const { claude, queries } = answering(NO_RECORD);
    expect(claude.restoreFiles("s", "/work", "p")).rejects.toThrow("No file checkpoint found");
    await settle();
    const query = only(queries);
    expect(query.rewinds).toEqual([{ promptId: "p", dryRun: false }]);
    expect(query.closed).toBe(true);
  });

  test("can't be asked after without Claude Code", async () => {
    const { claude, queries } = provider({
      find: () => {
        // It isn't anywhere.
      },
    });
    expect(claude.filesAt("s", "/work", "p")).rejects.toThrow(HostError);
    expect(claude.restoreFiles("s", "/work", "p")).rejects.toThrow(HostError);
    await settle();
    expect(queries).toEqual([]);
  });
});

describe("going back", () => {
  const conversation = async (): Promise<string[]> => await Promise.resolve(["a", "b"]);

  test("finds the entry before a message", async () => {
    const { claude } = provider({ conversation });
    expect(await claude.entryBefore("s", "/work", "b")).toBe("a");
  });

  test("has nothing before the message that started it", async () => {
    const { claude } = provider({ conversation });
    expect(await claude.entryBefore("s", "/work", "a")).toBeNull();
  });

  test("refuses a message it doesn't have", async () => {
    const { claude } = provider({ conversation });
    expect(claude.entryBefore("s", "/work", "c")).rejects.toThrow(HostError);
    await settle();
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
    expect(query?.options.mcpServers).toBeUndefined();
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
