// The options each kind of query runs with (docs/chat.md 4.7): a chat's live session, the
// initialization-only probe the catalog reads (4.5.9), and the one-shot title query (4.5.7);
// and the names Claude gives Koyo's modes (C12).

import type {
  CanUseTool,
  Options,
  PermissionMode,
  SpawnedProcess,
  SpawnOptions,
} from "@anthropic-ai/claude-agent-sdk";

import type { Mode } from "../../protocol/model";
import type { SessionSpec } from "../provider";
import type { Env } from "./environment";

/** Claude's permission mode for each of Koyo's modes, which are named for people (C12). */
export const PERMISSION: Readonly<Record<Mode, PermissionMode>> = {
  ask: "default",
  accept_edits: "acceptEdits",
  auto: "auto",
  full_access: "bypassPermissions",
};

/** The Koyo mode `mode` is; `null` for plan mode and the modes Koyo doesn't offer. */
export function modeOf(mode: PermissionMode): Mode | null {
  switch (mode) {
    case "default": {
      return "ask";
    }
    case "acceptEdits": {
      return "accept_edits";
    }
    case "auto": {
      return "auto";
    }
    case "bypassPermissions": {
      return "full_access";
    }
    case "plan":
    case "dontAsk": {
      return null;
    }
  }
}

/** The mode to ask for, the plan toggle first; `undefined` leaves it to Claude's settings. */
function requested(plan: boolean, mode: Mode | null): PermissionMode | undefined {
  if (plan) {
    return "plan";
  }
  return mode === null ? undefined : PERMISSION[mode];
}

/**
 * Whether a session opened in `mode`, planning or not, approves tools through the person: in
 * every mode but Full access, and in plan mode always (T3 adapter pitfalls 14, 15).
 */
export function asks(plan: boolean, mode: Mode | null): boolean {
  return plan || mode !== "full_access";
}

/** Where a query runs, and the program that runs it. */
export interface Place {
  readonly executable: string;
  readonly env: Env;
  readonly cwd: string;
  readonly spawn: (options: SpawnOptions) => SpawnedProcess;
}

/** What keeps a CLI that's only asked something from starting anything else. */
const ALONE = {
  settings: { disableAllHooks: true },
  mcpServers: {},
  strictMcpConfig: true,
} satisfies Options;

/** Where every query runs, and how its CLI is started. */
function placed(place: Place): Pick<
  Options,
  "pathToClaudeCodeExecutable" | "cwd" | "env" | "spawnClaudeCodeProcess"
> {
  return {
    pathToClaudeCodeExecutable: place.executable,
    cwd: place.cwd,
    env: place.env,
    spawnClaudeCodeProcess: place.spawn,
  };
}

/**
 * What Claude Code's own system prompt is told of Koyo: the files a reply names are links,
 * which Koyo opens in a tab of their own when they're clicked, at the line a link names.
 */
export const KOYO_PROMPT =
  "Koyo shows your replies as Markdown, and opens a link to a file in a tab when it's "
  + "clicked. When you name a file, or a line in one, write it as a Markdown link whose target "
  + "is its path, relative to the working directory or absolute, with #L and the line number "
  + "for a line: [viewer/mod.rs](src/viewer/mod.rs), [mod.rs:42](src/viewer/mod.rs#L42). "
  + "Don't put these links in backticks.";

/**
 * What Claude Code is told while it has Koyo's tools (docs/chat.md O1): they show their work in
 * the window the person looks at, so the person's other browsers and terminals wait to be named.
 */
export const KOYO_TOOLS_PROMPT =
  "Koyo's own tools, the koyo MCP server, drive the window the person is looking at: use them "
  + "rather than other tools unless the person names another.\n"
  + "- A worktree: mcp__koyo__worktree_create, which makes it where Koyo keeps worktrees, on a "
  + "branch of its name, as Koyo's New Worktree does. Never git worktree or EnterWorktree. Your "
  + "working folder stays where it is: to work in the new worktree, start a chat there with "
  + "mcp__koyo__chat_create.\n"
  + "- A web page: mcp__koyo__tab_open with `kind: \"browser\"`, the `url`, and `select: true` "
  + "when they asked to see it, then the mcp__koyo__browser_* tools.\n"
  + "- A server, an app or a command the person asks you to run: mcp__koyo__terminal_open with a "
  + "`name` and the `command`, then mcp__koyo__terminal_wait and mcp__koyo__terminal_read, not "
  + "Bash in the background. Bash stays for your own short commands.";

/**
 * The tools of Koyo's MCP server that only read, its `read` and `terminal.read` scopes', which
 * run without asking as Claude Code's own reads do. A test holds them to the catalogs.
 */
const KOYO_READS = [
  "app_status",
  "project_list",
  "worktree_branches",
  "worktree_list",
  "tab_list",
  "chat_list",
  "chat_choices",
  "chat_read",
  "settings_get",
  "activity_list",
  "claude_accounts_list",
  "browser_list",
  "browser_read",
  "browser_snapshot",
  "browser_screenshot",
  "browser_wait",
  "desktop_windows",
  "desktop_screenshot",
  "desktop_snapshot",
  "desktop_wait",
  "device_list",
  "device_screenshot",
  "device_snapshot",
  "device_wait",
  "device_apps",
  "terminal_list",
  "terminal_read",
  "terminal_wait",
  "terminal_control",
  "terminal_log",
];

/**
 * Koyo's tools for chat `threadId`: `koyo mcp` from the app's own `koyo`, naming the chat, which
 * the app trusts as it trusts an agent in its terminals; its reads allowed; and what a chat
 * shouldn't reach for taken away: Claude's own worktrees, which move the session's folder where
 * the chat can't follow, deleting a worktree, which an agent isn't granted, and launching Koyo,
 * which a chat would do only once the person quit it.
 */
function koyoTools(koyo: string, threadId: string): Pick<
  Options,
  "mcpServers" | "allowedTools" | "disallowedTools"
> {
  return {
    mcpServers: {
      koyo: { type: "stdio", command: koyo, args: ["mcp"], env: { KOYO_CHAT: threadId } },
    },
    allowedTools: KOYO_READS.map((name) => `mcp__koyo__${name}`),
    disallowedTools: [
      "EnterWorktree",
      "ExitWorktree",
      "mcp__koyo__worktree_delete",
      "mcp__koyo__app_launch",
    ],
  };
}

/** What a chat's session is opened with beside its spec. */
interface Opening {
  /** Whether the conversation has turns to continue. */
  readonly resume: boolean;
  /** How tools are approved, when the session asks the person (`asks`). */
  readonly canUseTool: CanUseTool;
}

/**
 * A chat session's options: Claude Code's own system prompt (an omitted one is empty) with
 * what it's told of Koyo (`KOYO_PROMPT`), and of Koyo's tools while it has them, text
 * as it streams, a subagent's words as well as its tools, what the model thought (left out
 * unless asked for, and asked for by the flag: the `thinking` option would also turn thinking on
 * for someone who has it off), and a permission mode only when the chat or Koyo names one, so
 * Claude's own settings choose otherwise (C12).
 */
export function sessionOptions(spec: SessionSpec, place: Place, opening: Opening): Options {
  const mode = spec.permission;
  const permissionMode = requested(spec.plan, mode);
  const koyo = spec.koyoTools;
  const append = koyo === null ? KOYO_PROMPT : `${KOYO_PROMPT}\n\n${KOYO_TOOLS_PROMPT}`;
  return {
    ...placed(place),
    ...(opening.resume ? { resume: spec.sessionId } : { sessionId: spec.sessionId }),
    ...(opening.resume && spec.resumeAt !== null ? { resumeSessionAt: spec.resumeAt } : {}),
    systemPrompt: { type: "preset", preset: "claude_code", append },
    ...(koyo === null ? {} : koyoTools(koyo, spec.threadId)),
    includePartialMessages: true,
    forwardSubagentText: true,
    extraArgs: { "thinking-display": "summarized" },
    ...(asks(spec.plan, mode) ? { canUseTool: opening.canUseTool } : {}),
    ...(spec.model === null ? {} : { model: spec.model }),
    ...(spec.effort === null ? {} : { effort: spec.effort }),
    ...(spec.fast ? { settings: { fastMode: true } } : {}),
    ...(permissionMode === undefined ? {} : { permissionMode }),
    ...(mode === "full_access" ? { allowDangerouslySkipPermissions: true } : {}),
    toolConfig: { askUserQuestion: { previewFormat: "markdown" } },
    enableFileCheckpointing: true,
  };
}

/**
 * The options of a session opened only to ask about a conversation's files: no hooks, no MCP
 * servers and nothing said, so the CLI makes no API call and the transcript doesn't grow.
 */
export function filesOptions(place: Place, sessionId: string): Options {
  return { ...placed(place), ...ALONE, resume: sessionId, enableFileCheckpointing: true };
}

/**
 * The initialization-only probe's options: no transcript, no hooks, no MCP servers, so the
 * CLI only starts, answers its initialize request and is closed, with no API call.
 */
export function probeOptions(place: Place): Options {
  return {
    ...placed(place),
    ...ALONE,
    persistSession: false,
  };
}

/** The title query's options: the smallest model, one turn, no tools, settings or transcript. */
export function titleOptions(place: Place): Options {
  return {
    ...placed(place),
    model: "haiku",
    tools: [],
    settingSources: [],
    persistSession: false,
    maxTurns: 1,
  };
}
