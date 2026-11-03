// What Claude asks the person before a tool runs (docs/chat.md 4.7, C14), through the SDK's
// `canUseTool`: an approval, a question (AskUserQuestion), or a proposed plan (ExitPlanMode),
// which is captured and denied at once so the agent stops for the person. The SDK's abort
// signal goes with a request to the actor, which withdraws it once the CLI no longer needs
// the answer (4.5.5); a request refused or withdrawn denies the tool.

import type {
  CanUseTool,
  PermissionResult,
  PermissionUpdate,
} from "@anthropic-ai/claude-agent-sdk";
import { z } from "zod";

import type { Question, RequestPayload, Response } from "../../protocol/model";
import type { DriverEvent, SessionSink } from "../provider";
import {
  bounded,
  canonical,
  detail,
  family,
  type Fields,
  type Json,
  label,
  mcpParts,
} from "./tools";

/** What the SDK tells `canUseTool` beside the tool and its input. */
export type ToolRequest = Parameters<CanUseTool>[2];

/** The message ExitPlanMode is denied with, which stops the agent at its plan (pitfall 11). */
export const PLAN_CAPTURED =
  "The client captured your proposed plan. Stop here and wait for the user's decision.";

const DECLINED = "The user declined this tool call.";
const CANCELLED = "The user stopped this turn.";
const WITHDRAWN = "The request was withdrawn.";

const Plan = z.object({ plan: z.string() });
const Questions = z.object({
  questions: z.array(z.object({
    question: z.string(),
    header: z.string(),
    multiSelect: z.boolean(),
    options: z.array(z.object({
      label: z.string(),
      description: z.string(),
      preview: z.string().optional(),
    })),
  })).min(1),
});

/** Where a session's requests go, and how it hears what became of a tool call. */
export interface Asking {
  readonly ask: SessionSink["ask"];
  readonly event: (event: DriverEvent) => void;
  /**
   * A tool call seen first in its permission request (T3 adapter pitfall 33), with its input
   * as an item keeps it, `kept`.
   */
  readonly started: (toolUseId: string, name: string, input: Fields, kept: Json) => void;
  /** The person declined a call, whose result then reads as declined. */
  readonly declined: (toolUseId: string) => void;
}

/** Answers the CLI's request to run `name` with `input`, asking the person as it must. */
export async function answerTool(
  asking: Asking,
  name: string,
  input: Fields,
  request: ToolRequest,
): Promise<PermissionResult> {
  const tool = canonical(name);
  if (tool === "ExitPlanMode") {
    const plan = Plan.safeParse(input);
    asking.event({
      type: "request",
      request: { kind: "plan", markdown: plan.success ? plan.data.plan : "" },
    });
    return { behavior: "deny", message: PLAN_CAPTURED };
  }
  if (tool === "AskUserQuestion") {
    return await askQuestions(asking, input, request.signal);
  }
  // The request stands in the timeline for these two; every other call has an item.
  const kept = bounded(input);
  asking.started(request.toolUseID, name, input, kept);
  const response = await answer(asking.ask(approval(name, input, request, kept), request.signal));
  const result = approved(response, input, request.suggestions ?? []);
  if (result.behavior === "deny") {
    asking.declined(request.toolUseID);
  }
  return result;
}

/** What the person is asked to approve; `kept` is the input as an item keeps it. */
export function approval(
  name: string,
  input: Fields,
  request: ToolRequest,
  kept: Json = bounded(input),
): RequestPayload {
  const suggestions = request.suppressAlwaysAllowRule === true ? [] : request.suggestions ?? [];
  return {
    kind: "approval",
    tool: canonical(name),
    family: family(name),
    title: request.title ?? title(name, input, request.displayName),
    detail: detail(name, input, kept),
    input: kept,
    // Claude's reasons sometimes carry terminal escapes.
    reason: request.decisionReason === undefined ? null : Bun.stripANSI(request.decisionReason),
    allowSession: suggestions.length === 0 ? null : { label: sessionLabel(suggestions) },
  };
}

/**
 * How the CLI is answered: allowed with the input as asked; allowed for the chat, with the
 * SDK's suggestions kept for this session only, never written to settings (pitfall 17);
 * declined with the person's words; or declined and the turn stopped.
 */
export function approved(
  response: Response | null,
  input: Fields,
  suggestions: readonly PermissionUpdate[],
): PermissionResult {
  if (response?.kind !== "approval") {
    return { behavior: "deny", message: WITHDRAWN };
  }
  switch (response.decision) {
    case "allow": {
      return { behavior: "allow", updatedInput: input };
    }
    case "allow_session": {
      return {
        behavior: "allow",
        updatedInput: input,
        updatedPermissions: suggestions.map((update) => ({ ...update, destination: "session" })),
      };
    }
    case "deny": {
      const message = response.message?.trim() ?? "";
      return { behavior: "deny", message: message === "" ? DECLINED : message };
    }
    case "cancel": {
      return { behavior: "deny", message: CANCELLED, interrupt: true };
    }
  }
}

/** AskUserQuestion: the questions asked as is, their answers given back by question. */
async function askQuestions(
  asking: Asking,
  input: Fields,
  signal: AbortSignal,
): Promise<PermissionResult> {
  const parsed = Questions.safeParse(input);
  if (!parsed.success) {
    return { behavior: "deny", message: "The questions couldn't be read." };
  }
  const questions: Question[] = parsed.data.questions.map((each) => ({
    // The question's text is its id: the CLI takes the answers keyed by it.
    id: each.question,
    header: each.header,
    question: each.question,
    multiSelect: each.multiSelect,
    options: each.options.map((option) => ({
      label: option.label,
      description: option.description,
      preview: option.preview ?? null,
    })),
  }));
  const response = await answer(asking.ask({ kind: "question", questions, live: true }, signal));
  return answered(response, input, questions);
}

/** The answers to AskUserQuestion as the CLI takes them; several choices joined with ", ". */
function answered(
  response: Response | null,
  input: Fields,
  questions: readonly Question[],
): PermissionResult {
  if (response?.kind !== "question") {
    return { behavior: "deny", message: WITHDRAWN };
  }
  const answers: Record<string, string> = {};
  for (const question of questions) {
    answers[question.question] = (response.answers[question.id] ?? []).join(", ");
  }
  return { behavior: "allow", updatedInput: { questions: input["questions"], answers } };
}

/**
 * The person's answer, or null when none comes: the request was refused, or withdrawn as the
 * SDK stopped waiting for it, which the actor watches the signal for (T3 adapter pitfall 18).
 */
async function answer(asked: Promise<Response>): Promise<Response | null> {
  try {
    return await asked;
  } catch {
    return null;
  }
}

function title(name: string, fields: Fields, shown: string | undefined): string {
  switch (family(name)) {
    case "command": {
      return "Run a command";
    }
    case "edit": {
      return canonical(name) === "Write" ? "Write a file" : "Edit a file";
    }
    case "read": {
      return "Read a file";
    }
    case "search": {
      return "Search files";
    }
    case "web": {
      return canonical(name) === "WebSearch" ? "Search the web" : "Fetch a page";
    }
    case "mcp": {
      const parts = mcpParts(name);
      return parts === undefined ? `Use ${name}` : `Use ${parts.tool} from ${parts.server}`;
    }
    case "agent": {
      return "Start a subagent";
    }
    case "skill": {
      return `Use the skill ${label(name, fields)}`;
    }
    case "todo":
    case "other": {
      return `Use ${shown ?? name}`;
    }
  }
}

/** What "Allow for this chat" adds, in words. */
function sessionLabel(suggestions: readonly PermissionUpdate[]): string {
  const parts = suggestions.flatMap((update) => {
    switch (update.type) {
      case "addRules":
      case "replaceRules": {
        return update.rules.map((rule) =>
          rule.ruleContent === undefined ? rule.toolName : `${rule.toolName}(${rule.ruleContent})`);
      }
      case "setMode": {
        return [update.mode === "acceptEdits" ? "every edit" : `${update.mode} mode`];
      }
      case "addDirectories": {
        return update.directories;
      }
      case "removeRules":
      case "removeDirectories": {
        return [];
      }
    }
  });
  return parts.length === 0 ? "Allow for this chat" : `Allow ${parts.join(", ")} for this chat`;
}
