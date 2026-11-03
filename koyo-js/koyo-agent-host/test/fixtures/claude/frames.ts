// Claude's frames for the driver's tests, typed by the SDK's own types. Each is built as the
// sessions recorded against the real CLI (Claude Code 2.1.317, SDK 0.3.317) had it: the
// fields the CLI sets, in the order it sends them, with the recordings' ids and paths
// replaced. Recorded JSON can't be read back as these types without a cast, so the frames
// are written out here instead.

import { randomUUID } from "node:crypto";

import type {
  PermissionMode,
  SDKAPIRetryMessage,
  SDKAssistantMessage,
  SDKCompactBoundaryMessage,
  SDKInformationalMessage,
  SDKMessageOrigin,
  SDKPartialAssistantMessage,
  SDKPermissionDeniedMessage,
  SDKRateLimitEvent,
  SDKRateLimitInfo,
  SDKResultError,
  SDKResultSuccess,
  SDKStatusMessage,
  SDKSystemMessage,
  SDKTaskNotificationMessage,
  SDKUserMessage,
  SDKUserMessageReplay,
  TerminalReason,
} from "@anthropic-ai/claude-agent-sdk";

const SESSION = "d23655c0-e6b4-4398-9f86-ab4313bd2a24";
export const MODEL = "claude-haiku-5-5";
/** The recorded model's window. */
export const WINDOW = 1_000_000;

type Message = SDKAssistantMessage["message"];
type Block = Message["content"][number];
type StreamEvent = SDKPartialAssistantMessage["event"];
type StartBlock = Extract<StreamEvent, { type: "content_block_start" }>["content_block"];
type DeltaUsage = Extract<StreamEvent, { type: "message_delta" }>["usage"];
type ResultUsage = SDKResultSuccess["usage"];

/** A model message's token counts. */
interface Counts {
  readonly input?: number;
  readonly output?: number;
  readonly cacheRead?: number;
  readonly cacheCreation?: number;
}

/** Which prompts a frame says it answers, and the tool call it's inside of. */
interface Stamp {
  readonly echo?: readonly string[];
  readonly parent?: string;
}

function stamped(stamp: Stamp): { user_message_uuids?: string[] } {
  return stamp.echo === undefined ? {} : { user_message_uuids: [...stamp.echo] };
}

function usage(counts: Counts = {}): Message["usage"] {
  return {
    cache_creation: null,
    cache_creation_input_tokens: counts.cacheCreation ?? 0,
    cache_read_input_tokens: counts.cacheRead ?? 0,
    fallback_credit: null,
    inference_geo: "global",
    input_tokens: counts.input ?? 0,
    iterations: null,
    output_tokens: counts.output ?? 0,
    output_tokens_details: null,
    server_tool_use: null,
    service_tier: "standard",
    speed: "standard",
  };
}

export function message(id: string, content: readonly Block[], model: string = MODEL): Message {
  return {
    id,
    container: null,
    content: [...content],
    context_management: null,
    diagnostics: null,
    model,
    role: "assistant",
    stop_details: null,
    stop_reason: null,
    stop_sequence: null,
    type: "message",
    usage: usage({ input: 2, output: 1, cacheCreation: 28_562 }),
  };
}

// Blocks.

export function textBlock(text: string): Extract<Block, { type: "text" }> {
  return { type: "text", text, citations: null };
}

/** A thinking block; the CLI withholds the text of the recorded model's, so it's empty. */
export function thinkingBlock(thinking = ""): Extract<Block, { type: "thinking" }> {
  return { type: "thinking", thinking, signature: "CAQSsQcKEAgTGAI4AUII" };
}

export function toolUse(id: string, name: string, input: unknown): Extract<
  Block,
  { type: "tool_use" }
> {
  return { type: "tool_use", id, name, input, caller: { type: "direct" } };
}

// The stream of one model message.

export function messageStart(id: string, counts: Counts = {}): StreamEvent {
  return { type: "message_start", message: { ...message(id, []), usage: usage(counts) } };
}

export function blockStart(index: number, block: StartBlock): StreamEvent {
  return { type: "content_block_start", index, content_block: block };
}

export function textDelta(index: number, text: string): StreamEvent {
  return { type: "content_block_delta", index, delta: { type: "text_delta", text } };
}

export function thinkingDelta(index: number, thinking: string): StreamEvent {
  return {
    type: "content_block_delta",
    index,
    delta: { type: "thinking_delta", thinking, estimated_tokens: null },
  };
}

export function blockStop(index: number): StreamEvent {
  return { type: "content_block_stop", index };
}

function deltaUsage(output: number, input: number | null = null): DeltaUsage {
  return {
    cache_creation_input_tokens: null,
    cache_read_input_tokens: null,
    fallback_credit: null,
    input_tokens: input,
    iterations: null,
    output_tokens: output,
    output_tokens_details: null,
    server_tool_use: null,
  };
}

export function messageDelta(
  output: number,
  stop: "end_turn" | "tool_use" = "end_turn",
): StreamEvent {
  return {
    type: "message_delta",
    context_management: null,
    delta: { container: null, stop_details: null, stop_reason: stop, stop_sequence: null },
    usage: deltaUsage(output),
  };
}

export function messageStop(): StreamEvent {
  return { type: "message_stop" };
}

// Frames.

export function stream(event: StreamEvent, stamp: Stamp = {}): SDKPartialAssistantMessage {
  return {
    type: "stream_event",
    event,
    parent_tool_use_id: stamp.parent ?? null,
    uuid: randomUUID(),
    session_id: SESSION,
    ...stamped(stamp),
  };
}

export function assistant(
  snapshot: Message,
  stamp: Stamp & { readonly error?: SDKAssistantMessage["error"] } = {},
): SDKAssistantMessage {
  return {
    type: "assistant",
    message: snapshot,
    parent_tool_use_id: stamp.parent ?? null,
    uuid: randomUUID(),
    session_id: SESSION,
    ...(stamp.error === undefined ? {} : { error: stamp.error }),
    ...stamped(stamp),
  };
}

/** What a tool returned, as the CLI hands it back to the model. */
interface Returned {
  readonly id: string;
  readonly content: string;
  readonly isError?: boolean;
}

/** A user frame of tool results, with the CLI's structured twin of them. */
export function toolResults(
  results: readonly Returned[],
  structured?: unknown,
  parent?: string,
): SDKUserMessage {
  return {
    type: "user",
    message: {
      role: "user",
      content: results.map((result) => ({
        type: "tool_result",
        tool_use_id: result.id,
        content: result.content,
        is_error: result.isError ?? false,
      })),
    },
    parent_tool_use_id: parent ?? null,
    ...(structured === undefined ? {} : { tool_use_result: structured }),
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

/** The CLI's record of what was said, such as a compaction's summary. */
export function replay(text: string): SDKUserMessageReplay {
  return {
    type: "user",
    message: { role: "user", content: text },
    parent_tool_use_id: null,
    uuid: randomUUID(),
    session_id: SESSION,
    isReplay: true,
  };
}

export function init(mode: PermissionMode, stamp: Stamp = {}): SDKSystemMessage {
  return {
    type: "system",
    subtype: "init",
    apiKeySource: "none",
    claude_code_version: "2.1.317",
    cwd: "/work/rec-cwd",
    tools: ["Bash", "Edit", "Read", "Write"],
    mcp_servers: [],
    model: MODEL,
    permissionMode: mode,
    slash_commands: ["compact", "review"],
    output_style: "default",
    skills: [],
    plugins: [],
    uuid: randomUUID(),
    session_id: SESSION,
    ...stamped(stamp),
  };
}

export function status(
  state: SDKStatusMessage["status"],
  more: Partial<Pick<SDKStatusMessage, "compact_result" | "compact_error" | "permissionMode">> = {},
): SDKStatusMessage {
  return {
    type: "system",
    subtype: "status",
    status: state,
    ...more,
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

export function boundary(pre: number, post: number): SDKCompactBoundaryMessage {
  return {
    type: "system",
    subtype: "compact_boundary",
    compact_metadata: { trigger: "manual", pre_tokens: pre, post_tokens: post, duration_ms: 7763 },
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

export function apiRetry(attempt: number): SDKAPIRetryMessage {
  return {
    type: "system",
    subtype: "api_retry",
    attempt,
    max_retries: 10,
    retry_delay_ms: 500,
    error_status: 529,
    error: "overloaded",
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

export function informational(
  level: SDKInformationalMessage["level"],
  content: string,
): SDKInformationalMessage {
  return {
    type: "system",
    subtype: "informational",
    content,
    level,
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

export function taskNotification(
  toolUseId: string,
  summary: string,
  state: SDKTaskNotificationMessage["status"] = "completed",
): SDKTaskNotificationMessage {
  return {
    type: "system",
    subtype: "task_notification",
    task_id: "bd0ghgd67",
    tool_use_id: toolUseId,
    status: state,
    output_file: "",
    summary,
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

export function permissionDenied(toolUseId: string): SDKPermissionDeniedMessage {
  return {
    type: "system",
    subtype: "permission_denied",
    tool_name: "Bash",
    tool_use_id: toolUseId,
    message: "Permission to use Bash was denied.",
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

export function rateLimit(info: SDKRateLimitInfo): SDKRateLimitEvent {
  return {
    type: "rate_limit_event",
    rate_limit_info: info,
    uuid: randomUUID(),
    session_id: SESSION,
  };
}

// Results.

function resultUsage(counts: Counts = {}): ResultUsage {
  return {
    cache_creation: {
      ephemeral_1h_input_tokens: counts.cacheCreation ?? 0,
      ephemeral_5m_input_tokens: 0,
    },
    cache_creation_input_tokens: counts.cacheCreation ?? 0,
    cache_read_input_tokens: counts.cacheRead ?? 0,
    fallback_credit: null,
    inference_geo: "global",
    input_tokens: counts.input ?? 0,
    iterations: [],
    output_tokens: counts.output ?? 0,
    output_tokens_details: { thinking_tokens: 0 },
    server_tool_use: { web_fetch_requests: 0, web_search_requests: 0 },
    service_tier: "standard",
    speed: "standard",
  };
}

function modelUsage(window: number): SDKResultSuccess["modelUsage"] {
  return {
    [MODEL]: {
      inputTokens: 1184,
      outputTokens: 121,
      cacheReadInputTokens: 0,
      cacheCreationInputTokens: 28_562,
      webSearchRequests: 0,
      costUSD: 0.005_891_3,
      contextWindow: window,
      maxOutputTokens: 128_000,
    },
  };
}

/** How a result reads, beside its prompts. */
interface ResultShape {
  readonly text?: string;
  readonly isError?: boolean;
  readonly terminal?: TerminalReason;
  readonly apiStatus?: number;
  readonly origin?: SDKMessageOrigin;
  readonly counts?: Counts;
  readonly cost?: number;
  readonly window?: number;
}

export function success(echo: readonly string[], shape: ResultShape = {}): SDKResultSuccess {
  return {
    type: "result",
    subtype: "success",
    duration_ms: 2342,
    duration_api_ms: 2111,
    is_error: shape.isError ?? false,
    api_error_status: shape.apiStatus ?? null,
    num_turns: 1,
    result: shape.text ?? "done",
    stop_reason: "end_turn",
    total_cost_usd: shape.cost ?? 0.005_891_3,
    usage: resultUsage(shape.counts ?? { input: 2, output: 104, cacheCreation: 28_562 }),
    modelUsage: modelUsage(shape.window ?? WINDOW),
    permission_denials: [],
    terminal_reason: shape.terminal ?? "completed",
    ...(shape.origin === undefined ? {} : { origin: shape.origin }),
    uuid: randomUUID(),
    session_id: SESSION,
    ...stamped({ echo }),
  };
}

export function failed(
  echo: readonly string[],
  errors: readonly string[],
  terminal?: TerminalReason,
): SDKResultError {
  return {
    type: "result",
    subtype: "error_during_execution",
    duration_ms: 3011,
    duration_api_ms: 2400,
    is_error: true,
    num_turns: 2,
    stop_reason: null,
    total_cost_usd: 0.004,
    usage: resultUsage({ input: 2, output: 40 }),
    modelUsage: modelUsage(WINDOW),
    permission_denials: [],
    errors: [...errors],
    ...(terminal === undefined ? {} : { terminal_reason: terminal }),
    uuid: randomUUID(),
    session_id: SESSION,
    ...stamped({ echo }),
  };
}
