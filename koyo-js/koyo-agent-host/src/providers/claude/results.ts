// How a turn ended and what it used (docs/chat.md 4.7). A result is classified on what it
// carries, never on `subtype: "success"`, which the SDK also gives API failures (T3 adapter
// pitfall 21): an error flag, an API status of 429 or 529, or an error terminal reason fail
// the run, and an aborted one interrupts it.

import type {
  ModelUsage,
  SDKResultMessage,
  TerminalReason,
} from "@anthropic-ai/claude-agent-sdk";

import type { Failure, Usage } from "../../protocol/model";
import type { Outcome } from "../provider";

/** What Claude Code holds back of a model's window before it compacts (pitfall 38). */
const COMPACT_RESERVE = 33_000;

/** What the turn's other frames said that bears on how it ended. */
export interface Signs {
  /** An assistant frame said the login failed. */
  readonly authFailed: boolean;
  /** An assistant frame said the API was overloaded. */
  readonly overloaded: boolean;
  /** A usage limit was hit; when it resets, if a rate-limit event said. */
  readonly limited: { readonly retryAt: string | null } | null;
  /** The person asked the turn to stop. */
  readonly interrupted: boolean;
}

export interface Ending {
  readonly outcome: Outcome;
  readonly failure: Failure | null;
}

/** The terminal reasons that fail a run, whatever the result's subtype. */
const FAILING: ReadonlySet<TerminalReason> = new Set<TerminalReason>([
  "api_error",
  "malformed_tool_use_exhausted",
  "budget_exhausted",
  "structured_output_retry_exhausted",
  "tool_deferred_unavailable",
  "turn_setup_failed",
  "blocking_limit",
  "rapid_refill_breaker",
  "prompt_too_long",
  "image_error",
  "model_error",
]);

const LIMITING: ReadonlySet<TerminalReason> = new Set<TerminalReason>([
  "blocking_limit",
  "rapid_refill_breaker",
]);

/** What a usage limit reads, as a notice when it's hit and as the failure it ends a turn in. */
export const LIMITED = "Claude's usage limit is reached.";
const SIGN_IN = "Sign in to Claude again.";
const OVERLOADED = "Claude is overloaded right now. Try again in a moment.";

/** How `result` ended its turn. */
export function classify(result: SDKResultMessage, signs: Signs): Ending {
  const reason = result.terminal_reason;
  if (reason === "aborted_streaming" || reason === "aborted_tools") {
    return { outcome: "interrupted", failure: null };
  }
  const status = result.subtype === "success" ? result.api_error_status ?? null : null;
  const failing = result.is_error || result.subtype !== "success" || status === 429
    || status === 529 || (reason !== undefined && FAILING.has(reason));
  if (!failing) {
    return { outcome: "completed", failure: null };
  }
  // A stop the person asked for ends the turn with an error result of its own.
  if (signs.interrupted && result.subtype === "error_during_execution") {
    return { outcome: "interrupted", failure: null };
  }
  return { outcome: "failed", failure: failure(result, signs, status) };
}

function failure(result: SDKResultMessage, signs: Signs, status: number | null): Failure {
  if (signs.authFailed || status === 401) {
    return { kind: "auth", message: SIGN_IN, retryAt: null };
  }
  const reason = result.terminal_reason;
  if (signs.limited !== null || status === 429 || (reason !== undefined && LIMITING.has(reason))) {
    return { kind: "usage_limit", message: LIMITED, retryAt: signs.limited?.retryAt ?? null };
  }
  if (signs.overloaded || status === 529) {
    return { kind: "overloaded", message: OVERLOADED, retryAt: null };
  }
  return { kind: "api", message: errorText(result), retryAt: null };
}

/** What the result says went wrong, without the CLI's diagnostic lines. */
function errorText(result: SDKResultMessage): string {
  const lines = result.subtype === "success" ? [result.result] : result.errors;
  const said = lines
    .flatMap((line) => line.split("\n"))
    .filter((line) => !line.startsWith("[ede_diagnostic]") && line.trim() !== "")
    .join("\n");
  if (said !== "") {
    return said;
  }
  const reason = result.terminal_reason ?? result.subtype;
  return `Claude stopped: ${reason.replaceAll("_", " ")}.`;
}

/** The context window of the turn's model, less what's held back for compacting. */
export function compactPoint(
  modelUsage: Readonly<Record<string, ModelUsage>>,
  model: string | null,
): number | null {
  const entries = Object.entries(modelUsage);
  const own = entries.find(([name]) => name === model)?.[1];
  const window = own?.contextWindow
    ?? Math.max(0, ...entries.map(([, usage]) => usage.contextWindow));
  return window > COMPACT_RESERVE ? window - COMPACT_RESERVE : null;
}

/** What a turn used, from its result: tokens and cost, and the context as it stands. */
export function resultUsage(
  result: SDKResultMessage,
  context: { readonly tokens: number | null; readonly window: number | null },
): Usage {
  const { usage } = result;
  return {
    inputTokens: usage.input_tokens,
    outputTokens: usage.output_tokens,
    cacheReadTokens: usage.cache_read_input_tokens,
    cacheCreationTokens: usage.cache_creation_input_tokens,
    costUsd: result.total_cost_usd,
    contextTokens: context.tokens,
    contextWindow: context.window,
  };
}

/** Two usages of one run, as one: a turn a steer continued ends with a result of each. */
export function summed(before: Usage | null, after: Usage): Usage {
  if (before === null) {
    return after;
  }
  return {
    inputTokens: before.inputTokens + after.inputTokens,
    outputTokens: before.outputTokens + after.outputTokens,
    cacheReadTokens: before.cacheReadTokens + after.cacheReadTokens,
    cacheCreationTokens: before.cacheCreationTokens + after.cacheCreationTokens,
    costUsd: before.costUsd === null || after.costUsd === null
      ? after.costUsd
      : before.costUsd + after.costUsd,
    contextTokens: after.contextTokens,
    contextWindow: after.contextWindow,
  };
}
