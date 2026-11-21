import {
  createSdkMcpServer,
  type HookCallback,
  type HookCallbackMatcher,
  type HookEvent,
  type HookInput,
  type HookJSONOutput,
  type McpSdkServerConfigWithInstance,
  tool,
} from "@anthropic-ai/claude-agent-sdk";

import { UPDATE_INPUT } from "../../progress/input";
import {
  PROGRESS_DESCRIPTION,
  PROGRESS_SERVER,
  PROGRESS_TOOL,
  PROGRESS_TOOL_NAME,
} from "../../progress/prompt";
import { promptReminder, staleReminder } from "../../progress/reminders";
import type { PlanSink } from "../provider";

/** The person's own Claude Code mod's plan tool, which Koyo's takes the place of in a chat. */
export const MOD_TOOL = "mcp__progress__update";

const SUBAGENT =
  "Only the chat's main agent keeps the plan; say what changed in your report to it.";

/** What keeps the chat's plan in a session's process (docs/chat.md 5.5). */
export interface ProgressTools {
  /** `koyo_progress`, in the host's process: its one tool's calls go to the chat's actor. */
  readonly server: McpSdkServerConfigWithInstance;
  /** Those that refuse a subagent's call and remind the agent of the plan. */
  readonly hooks: Partial<Record<HookEvent, HookCallbackMatcher[]>>;
}

/** A hook that answers at once with what `answer` makes of its input. */
function hook(answer: (input: HookInput) => HookJSONOutput): HookCallback {
  return async (input) => await Promise.resolve(answer(input));
}

/** A subagent's call of the progress tool refused; the chat's main agent's let through. */
function mainAgentOnly(input: HookInput): HookJSONOutput {
  if (input.agent_id === undefined) {
    return {};
  }
  return {
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason: SUBAGENT,
    },
  };
}

/** Whether a prompt is the person's: a message they sent, not a turn the CLI started itself. */
function fromPerson(input: HookInput): boolean {
  if (input.hook_event_name !== "UserPromptSubmit") {
    return false;
  }
  const { source } = input;
  return source === undefined || source === "user" || source === "sdk";
}

/** Reminds the agent of the plan's open work as the person sends a message. */
function onPrompt(input: HookInput, plan: PlanSink): HookJSONOutput {
  const reminder = fromPerson(input) ? promptReminder(plan.current()) : null;
  return reminder === null
    ? {}
    : { hookSpecificOutput: { hookEventName: "UserPromptSubmit", additionalContext: reminder } };
}

/**
 * The tool and hooks a session's process keeps the plan with, new for each process: the tool's
 * calls go to `plan`, and the main agent's other calls are counted from the plan's last change,
 * to remind it once a stretch. The tool touches nothing of the person's, so it's read-only, and
 * plan mode lets it run without asking.
 */
export function progressTools(plan: PlanSink): ProgressTools {
  let calls: number | null = 0;
  const update = tool(PROGRESS_TOOL_NAME, PROGRESS_DESCRIPTION, UPDATE_INPUT, async (args) => {
    const answer = await plan.update(args);
    if (!answer.refused) {
      calls = 0;
    }
    return {
      content: [{ type: "text", text: answer.text }],
      ...(answer.refused ? { isError: true } : {}),
    };
  }, { annotations: { readOnlyHint: true } });
  const counted = (input: HookInput): HookJSONOutput => {
    const counts = input.hook_event_name === "PostToolUse" && input.agent_id === undefined
      && input.tool_name !== PROGRESS_TOOL;
    if (!counts || calls === null) {
      return {};
    }
    calls += 1;
    const reminder = staleReminder(plan.current(), calls);
    if (reminder === null) {
      return {};
    }
    calls = null;
    return { hookSpecificOutput: { hookEventName: "PostToolUse", additionalContext: reminder } };
  };
  return {
    server: createSdkMcpServer({ name: PROGRESS_SERVER, alwaysLoad: true, tools: [update] }),
    hooks: {
      PreToolUse: [{ matcher: PROGRESS_TOOL, hooks: [hook(mainAgentOnly)] }],
      PostToolUse: [{ hooks: [hook(counted)] }],
      UserPromptSubmit: [{ hooks: [hook((input) => onPrompt(input, plan))] }],
    },
  };
}
