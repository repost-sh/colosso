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

/** What a chat's session is opened with beside its spec. */
interface Opening {
  /** Whether the conversation has turns to continue. */
  readonly resume: boolean;
  /** How tools are approved, when the session asks the person (`asks`). */
  readonly canUseTool: CanUseTool;
}

/**
 * A chat session's options: Claude Code's own system prompt (an omitted one is empty) with
 * what it's told of Koyo (`KOYO_PROMPT`), text
 * as it streams, a subagent's words as well as its tools, what the model thought (left out
 * unless asked for, and asked for by the flag: the `thinking` option would also turn thinking on
 * for someone who has it off), and a permission mode only when the chat or Koyo names one, so
 * Claude's own settings choose otherwise (C12).
 */
export function sessionOptions(spec: SessionSpec, place: Place, opening: Opening): Options {
  const mode = spec.permission;
  const permissionMode = requested(spec.plan, mode);
  return {
    ...placed(place),
    ...(opening.resume ? { resume: spec.sessionId } : { sessionId: spec.sessionId }),
    ...(opening.resume && spec.resumeAt !== null ? { resumeSessionAt: spec.resumeAt } : {}),
    systemPrompt: { type: "preset", preset: "claude_code", append: KOYO_PROMPT },
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
